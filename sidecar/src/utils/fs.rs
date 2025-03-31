use std::path::Path;
use tokio::fs;
use std::io;

pub const MAX_RETRIES: u32 = 3;
pub const RETRY_DELAY_MS: u64 = 100;

/// Creates a directory and all its parent directories with retry logic
pub async fn create_dir_all_with_retry(path: impl AsRef<Path>) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match fs::create_dir_all(path.as_ref()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                }
            }
        }
    }
    Err(last_error.unwrap())
}

/// Writes content to a file with retry logic
pub async fn write_file_with_retry(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match fs::write(path.as_ref(), contents.as_ref()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                }
            }
        }
    }
    Err(last_error.unwrap())
}

/// Creates a directory with retry logic
pub async fn create_dir_with_retry(path: impl AsRef<Path>) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match fs::create_dir(path.as_ref()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                }
            }
        }
    }
    Err(last_error.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs as std_fs;

    #[tokio::test]
    async fn test_create_dir_all_with_retry_success() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("test/nested/dir");
        
        let result = create_dir_all_with_retry(&test_dir).await;
        assert!(result.is_ok());
        assert!(test_dir.exists());
    }

    #[tokio::test]
    async fn test_write_file_with_retry_success() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        
        let result = write_file_with_retry(&test_file, "test content").await;
        assert!(result.is_ok());
        assert!(test_file.exists());
        
        let content = std_fs::read_to_string(test_file).unwrap();
        assert_eq!(content, "test content");
    }

    #[tokio::test]
    async fn test_create_dir_with_retry_success() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("test_dir");
        
        let result = create_dir_with_retry(&test_dir).await;
        assert!(result.is_ok());
        assert!(test_dir.exists());
    }
}