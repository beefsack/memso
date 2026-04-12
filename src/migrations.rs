use anyhow::Result;
use turso::Connection;

pub async fn run(conn: &Connection) -> Result<()> {
    // Base schema: run each statement individually.
    //
    // execute_batch() has a bug in turso/limbo 0.5.3: it throws
    // "Parse error: table X already exists" even for CREATE TABLE IF NOT EXISTS
    // when the table is already present. Running statements one at a time and
    // swallowing "already exists" errors works around this.
    for stmt in SCHEMA_STATEMENTS {
        if let Err(e) = conn.execute(stmt, turso::params![]).await {
            let msg = e.to_string();
            if !msg.contains("already exists") {
                return Err(e).map_err(|e| anyhow::anyhow!("Schema error in statement: {stmt}\n{e}"));
            }
        }
    }

    let version = get_version(conn).await?;

    // v0 -> v1: rename legacy 'agent' source value to 'realtime'.
    if version < 1 {
        conn.execute(
            "UPDATE memories SET source = 'realtime' WHERE source = 'agent'",
            turso::params![],
        )
        .await?;
        set_version(conn, 1).await?;
    }

    // v1 -> v2: replace libsql-specific constructs with turso equivalents.
    //   - DROP memory_vectors (was F32_BLOB(384) + libsql_vector_idx)
    //   - DROP FTS5 virtual table (triggers omitted: DROP TRIGGER is experimental)
    //   - Recreate memory_vectors with plain BLOB
    //   - Create turso native FTS index (auto-maintained, no triggers needed)
    if version < 2 {
        for stmt in MIGRATE_V2_STATEMENTS {
            if let Err(e) = conn.execute(stmt, turso::params![]).await {
                let msg = e.to_string();
                if !msg.contains("already exists") {
                    return Err(e).map_err(|e| anyhow::anyhow!("Migration v2 error in statement: {stmt}\n{e}"));
                }
            }
        }
        set_version(conn, 2).await?;
    }

    // Add future migrations here:
    // if version < 3 { ... set_version(conn, 3).await?; }

    Ok(())
}

async fn get_version(conn: &Connection) -> Result<i64> {
    let row = conn
        .query("SELECT version FROM schema_version LIMIT 1", turso::params![])
        .await?
        .next()
        .await?;
    Ok(row.map(|r| r.get::<i64>(0).unwrap_or(0)).unwrap_or(0))
}

async fn set_version(conn: &Connection, version: i64) -> Result<()> {
    conn.execute("DELETE FROM schema_version", turso::params![]).await?;
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        turso::params![version],
    )
    .await?;
    Ok(())
}

const SCHEMA_STATEMENTS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS memories (
        id            TEXT PRIMARY KEY,
        project_id    TEXT NOT NULL,
        topic_key     TEXT,
        type          TEXT NOT NULL,
        title         TEXT NOT NULL,
        content       TEXT NOT NULL,
        facts         TEXT,
        tags          TEXT,
        importance    REAL    DEFAULT 0.5,
        confidence    REAL    DEFAULT 1.0,
        access_count  INTEGER DEFAULT 0,
        last_accessed TEXT,
        pinned        INTEGER DEFAULT 0,
        status        TEXT    DEFAULT 'active',
        supersedes    TEXT,
        session_id    TEXT,
        source        TEXT    NOT NULL DEFAULT 'realtime',
        created_at    TEXT    NOT NULL,
        updated_at    TEXT    NOT NULL,
        content_hash  TEXT    NOT NULL
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS memories_topic_key
        ON memories (project_id, topic_key)
        WHERE topic_key IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS memories_project_status
        ON memories (project_id, status)",
    "CREATE INDEX IF NOT EXISTS memories_content_hash
        ON memories (content_hash, session_id)",
    "CREATE TABLE IF NOT EXISTS sessions (
        id         TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        started_at TEXT NOT NULL,
        ended_at   TEXT,
        status     TEXT DEFAULT 'active',
        agent      TEXT
    )",
    "CREATE TABLE IF NOT EXISTS memory_vectors (
        memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
        embedding BLOB NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS raw_captures (
        id           TEXT PRIMARY KEY,
        project_id   TEXT NOT NULL,
        captured_at  TEXT NOT NULL,
        tool_name    TEXT NOT NULL,
        summary      TEXT NOT NULL,
        raw_data     TEXT NOT NULL,
        presented_at TEXT
    )",
    "CREATE INDEX IF NOT EXISTS raw_captures_pending
        ON raw_captures (project_id, presented_at)",
];

// Migration v2: replace libsql-specific constructs with turso equivalents.
//
// Drops:
//   - memory_vectors (was F32_BLOB(384) + libsql_vector_idx ANN index)
//   - memories_fts FTS5 virtual table (triggers omitted: DROP TRIGGER is
//     experimental in turso/limbo and these DBs never had triggers)
//
// Recreates:
//   - memory_vectors with plain BLOB column (vector32() handles encoding)
//   - memories_fts as a turso native FTS index (auto-maintained, no triggers)
const MIGRATE_V2_STATEMENTS: &[&str] = &[
    "DROP TABLE IF EXISTS memory_vectors",
    "DROP TABLE IF EXISTS memories_fts",
    "CREATE TABLE IF NOT EXISTS memory_vectors (
        memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
        embedding BLOB NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS memories_fts
        ON memories USING fts (title, content, facts)
        WITH (weights = 'title=3.0,content=1.0,facts=1.5')",
];

// Note: the `sessions` table, and the `confidence` and `supersedes` columns on
// `memories`, are defined in the schema but not yet used by any code path.
// They are reserved for post-v1 features (session tracking, supersedes chaining).
// Do not remove them from the schema without a versioned migration.
