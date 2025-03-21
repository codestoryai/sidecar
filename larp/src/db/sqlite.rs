use anyhow::Context;
use anyhow::Result;
use sqlx::SqlitePool;
use tracing::debug;

use std::{path::Path, sync::Arc};

use crate::application::config::configuration::Configuration;

// Arcing it up so we can share it across threads
pub type SqlDb = Arc<SqlitePool>;

pub async fn init(config: Arc<Configuration>) -> Result<SqlitePool> {
    let data_dir = config.index_dir.to_string_lossy().to_owned();

    match connect(&data_dir).await {
        Ok(pool) => Ok(pool),
        Err(e) => {
            debug!("failed to connect to db: {:#}", e);
            debug!("resetting db");
            // do not let reset fail the whole thing
            let _ = reset(&data_dir);
            connect(&data_dir).await
        }
    }
}

async fn connect(data_dir: &str) -> Result<SqlitePool> {
    let url = format!("sqlite://{data_dir}/codestory.data?mode=rwc");
    debug!("loading db from {url}");
    let pool = SqlitePool::connect(&url).await?;

    if let Err(e) = sqlx::migrate!().run(&pool).await {
        // We manually close the pool here to ensure file handles are properly cleaned up on
        // Windows.
        pool.close().await;
        Err(e)?
    } else {
        Ok(pool)
    }
}

fn reset(data_dir: &str) -> Result<()> {
    let db_path = Path::new(data_dir).join("codestory.data");
    let bk_path = db_path.with_extension("codestory.bk");
    std::fs::rename(db_path, bk_path).context("failed to backup old database")
}
