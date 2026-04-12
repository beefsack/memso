use anyhow::{bail, Context, Result};
use turso::Connection;

use crate::config::Config;
use crate::migrations;

/// Enable remote sync: open the sync DB, do initial pull, run migrations.
pub async fn enable(
    config: &mut Config,
    url: Option<String>,
    token: Option<String>,
    force: bool,
) -> Result<()> {
    let url = url
        .or_else(|| config.backend.remote_url.clone())
        .context("remote URL required (pass --url or set backend.remote_url in .memso.toml)")?;
    let token = token
        .or_else(|| config.backend.auth_token.clone())
        .context("auth token required (pass --token or set MEMSO_REMOTE_AUTH_TOKEN)")?;

    if matches!(config.backend.mode, crate::config::BackendMode::Replica) && !force {
        bail!("already in replica mode. Use --force to re-initialise the replica.");
    }

    config.backend.mode = crate::config::BackendMode::Replica;
    config.backend.remote_url = Some(url.clone());
    config.backend.auth_token = Some(token.clone());

    let path = config.db_path();
    crate::db::ensure_parent_dir(&path)?;
    let path_str = path.to_str().context("replica DB path is not valid UTF-8")?;

    println!("Connecting to {} ...", url);
    let db = turso::sync::Builder::new_remote(path_str)
        .with_remote_url(&url)
        .with_auth_token(&token)
        .build()
        .await
        .context("Failed to open replica database")?;

    println!("Pulling initial data ...");
    db.pull().await.context("Failed to pull initial data from remote")?;

    let conn = db.connect().await.context("Failed to connect to replica")?;
    migrations::run(&conn).await.context("Failed to run migrations on replica")?;

    println!("Remote sync enabled. Update your .memso.toml:\n");
    println!("  [backend]");
    println!("  mode = \"replica\"");
    println!("  remote_url = \"{}\"", url);
    println!("  # auth_token = \"...\" or set MEMSO_REMOTE_AUTH_TOKEN");

    Ok(())
}

/// Push local changes to remote and pull any new remote changes.
/// Returns a human-readable status string.
pub async fn sync(config: &Config, _force: bool) -> Result<String> {
    if !matches!(config.backend.mode, crate::config::BackendMode::Replica) {
        return Ok(
            "Not in replica mode - nothing to sync. \
             Set backend.mode = \"replica\" in .memso.toml."
                .to_string(),
        );
    }

    let path = config.db_path();
    let path_str = path.to_str().context("replica DB path is not valid UTF-8")?;
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
        .context("Failed to open replica database for sync")?;

    db.push().await.context("Failed to push local changes")?;
    let had_changes = db.pull().await.context("Failed to pull remote changes")?;

    let msg = if had_changes {
        "Sync complete: local changes pushed, remote changes pulled."
    } else {
        "Sync complete: local changes pushed, no remote changes."
    };
    Ok(msg.to_string())
}

/// Copy all data from `src` to `dst`. Returns (memories, vectors, captures) counts.
pub async fn copy_all(src: &Connection, dst: &Connection) -> Result<(usize, usize, usize)> {
    let memories = copy_table(src, dst, "memories").await?;
    let vectors = copy_table(src, dst, "memory_vectors").await?;
    let captures = copy_table(src, dst, "raw_captures").await?;
    Ok((memories, vectors, captures))
}

async fn copy_table(src: &Connection, dst: &Connection, table: &str) -> Result<usize> {
    let mut rows = src
        .query(&format!("SELECT * FROM {table}"), turso::params![])
        .await
        .with_context(|| format!("Failed to read {table}"))?;

    let mut count = 0;
    while let Some(row) = rows.next().await? {
        let col_count = row.column_count();
        let values: Vec<turso::Value> = (0..col_count)
            .map(|i| row.get_value(i).unwrap_or(turso::Value::Null))
            .collect();
        let placeholders = (1..=col_count)
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        dst.execute(
            &format!("INSERT OR IGNORE INTO {table} VALUES ({placeholders})"),
            turso::params_from_iter(values),
        )
        .await
        .with_context(|| format!("Failed to insert into {table}"))?;
        count += 1;
    }
    Ok(count)
}
