#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tokio::fs as tokio_fs;

    #[tokio::test]
    async fn test_write_file_retry_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let editor = AnthropicCodeEditor::new("test".to_string());
        
        // Create a file that will be temporarily locked
        let _file = fs::File::create(&file_path).unwrap();
        
        // Simulate file being temporarily unavailable then becoming available
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            fs::remove_file(&file_path).unwrap();
        });

        let result = editor.write_file(&file_path, "test content").await;
        assert!(result.is_ok());
        
        // Verify content was written
        let content = tokio_fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "test content");
    }

    #[tokio::test]
    async fn test_write_file_retry_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let editor = AnthropicCodeEditor::new("test".to_string());
        
        // Create and lock file for longer than retry period
        let _file = fs::File::create(&file_path).unwrap();
        
        let result = editor.write_file(&file_path, "test content").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_with_retry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let editor = AnthropicCodeEditor::new("test".to_string());
        
        let params = CodeEditorParameters {
            path: file_path.to_str().unwrap().to_string(),
            command: EditorCommand::Create,
            file_text: Some("test content".to_string()),
            ..Default::default()
        };

        // Simulate temporary directory lock
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            fs::remove_file(&file_path).unwrap();
        });

        let result = editor.run_command(params).await;
        assert!(result.is_ok());
    }
}