use std::path::Path;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tracing::{debug, warn};

const MAX_LOG_AGE_DAYS: u64 = 7;

pub async fn cleanup_old_logs(log_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    let log_dir = log_dir.as_ref();
    if !log_dir.exists() {
        return Ok(());
    }

    let max_age = Duration::from_secs(MAX_LOG_AGE_DAYS * 24 * 60 * 60);
    let now = SystemTime::now();

    let mut read_dir = fs::read_dir(log_dir).await?;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if let Ok(metadata) = fs::metadata(&path).await {
            if let Ok(modified_time) = metadata.modified() {
                if let Ok(age) = now.duration_since(modified_time) {
                    if age > max_age {
                        if let Err(e) = fs::remove_file(&path).await {
                            warn!("Failed to remove old log file {:?}: {}", path, e);
                        } else {
                            debug!("Removed old log file: {:?}", path);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
