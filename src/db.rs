use anyhow::{Context, Result};
use turso::{Builder, Connection, Database};
use std::path::Path;

use crate::config::{BackendMode, Config};

pub struct Db {
    pub conn: Connection,
    _db: DbHandle,
}

/// Holds either a local or sync database to keep it alive.
#[allow(dead_code)]
enum DbHandle {
    Local(Database),
    Sync(turso::sync::Database),
}

impl Db {
    pub async fn open(config: &Config) -> Result<Self> {
        match config.backend.mode {
            BackendMode::Local => {
                let path = config.db_path();
                ensure_parent_dir(&path)?;
                let path_str = path.to_str()
                    .with_context(|| format!("DB path is not valid UTF-8: {}", path.display()))?;
                let db = Builder::new_local(path_str)
                    .experimental_index_method(true)
                    .build()
                    .await
                    .with_context(|| format!("Failed to open local DB at {}", path.display()))?;

                let conn = db.connect().context("Failed to connect to database")?;

                // busy_timeout: makes writers retry instead of immediately returning SQLITE_BUSY.
                // WAL pragma omitted: PRAGMA journal_mode returns a result row which turso's
                // execute_batch does not support. WAL is also unnecessary since turso/limbo
                // does not support concurrent multi-process access.
                conn.execute("PRAGMA busy_timeout=5000", turso::params![])
                    .await
                    .context("Failed to set busy_timeout")?;

                Ok(Self { conn, _db: DbHandle::Local(db) })
            }
            BackendMode::Replica => {
                let path = config.db_path();
                ensure_parent_dir(&path)?;
                let path_str = path.to_str()
                    .context("replica DB path is not valid UTF-8")?;
                let url = config
                    .backend
                    .remote_url
                    .as_deref()
                    .context("replica mode requires backend.remote_url")?;
                let token = config
                    .backend
                    .auth_token
                    .as_deref()
                    .context("replica mode requires backend.auth_token")?;

                let db = turso::sync::Builder::new_remote(path_str)
                    .with_remote_url(url)
                    .with_auth_token(token)
                    .build()
                    .await
                    .with_context(|| format!("Failed to open replica DB at {}", path.display()))?;

                // Initial pull from remote.
                db.pull()
                    .await
                    .context("Failed to pull initial data from remote")?;

                let conn = db.connect()
                    .await
                    .context("Failed to connect to replica database")?;

                Ok(Self { conn, _db: DbHandle::Sync(db) })
            }
        }
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    Ok(())
}
