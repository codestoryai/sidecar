#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::fs as tokio_fs;

    async fn setup_test_tree() -> (SearchTree, TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_dir = temp_dir.path().to_str().unwrap().to_string();
        
        let tree = SearchTree::new(
            1, // max_expansions
            1, // max_depth
            1, // max_iterations
            None, // max_finished_nodes
            None, // reward_threshold
            None, // min_finished_nodes
            None, // max_search_try
            "/tmp".to_string(), // root_directory
            "test_repo".to_string(), // repo_name
            "test_hash".to_string(), // repo_base_commit_hash
            "test".to_string(), // problem_statement
            Selector::default(), // selector
            vec![], // tools
            Arc::new(ToolBox::default()), // tool_box
            Arc::new(LLMBroker::default()), // llm_client
            log_dir, // log_directory
            AgentSettings::new(true, true), // agent_settings
        );
        
        (tree, temp_dir)
    }

    #[tokio::test]
    async fn test_write_file_with_retry_success() {
        let (tree, temp_dir) = setup_test_tree().await;
        let file_path = temp_dir.path().join("test.txt");
        
        // Create a file that will be temporarily locked
        let _file = fs::File::create(&file_path).unwrap();
        
        // Simulate file being temporarily unavailable then becoming available
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            fs::remove_file(&file_path).unwrap();
        });

        let result = SearchTree::write_file_with_retry(
            file_path.to_str().unwrap(),
            "test content"
        ).await;
        assert!(result.is_ok());
        
        // Verify content was written
        let content = tokio_fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "test content");
    }

    #[tokio::test]
    async fn test_write_file_with_retry_failure() {
        let (tree, temp_dir) = setup_test_tree().await;
        let file_path = temp_dir.path().join("test.txt");
        
        // Create and lock file for longer than retry period
        let _file = fs::File::create(&file_path).unwrap();
        
        let result = SearchTree::write_file_with_retry(
            file_path.to_str().unwrap(),
            "test content"
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_save_serialised_graph() {
        let (tree, temp_dir) = setup_test_tree().await;
        let log_dir = temp_dir.path().join("logs");
        
        // Test with directory temporarily locked
        fs::create_dir(&log_dir).unwrap();
        let log_dir_clone = log_dir.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            fs::remove_dir(&log_dir_clone).unwrap();
        });

        tree.save_serialised_graph(
            log_dir.to_str().unwrap(),
            "test_request"
        ).await;
        
        // Verify file was created
        let file_path = log_dir.join("mcts-test_request.json");
        assert!(file_path.exists());
    }
}