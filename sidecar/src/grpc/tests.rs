#![cfg(feature = "grpc")]

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::application::application::Application;
    use crate::application::config::Config;
    use tokio::sync::mpsc;
    use tokio_stream::StreamExt;
    use tonic::Request;

    async fn setup_test_server() -> AgentFarmGrpcServer {
        let config = Config::default();
        let app = Application::new(config).await;
        AgentFarmGrpcServer::new(app)
    }

    #[tokio::test]
    async fn test_agent_session_chat() {
        let server = setup_test_server().await;
        let request = AgentSessionRequest {
            session_id: "test_session".to_string(),
            exchange_id: "test_exchange".to_string(),
            editor_url: "test_url".to_string(),
            query: "test query".to_string(),
            user_context: None,
            repo_ref: None,
            root_directory: "/test".to_string(),
            project_labels: vec![],
            codebase_search: false,
            access_token: "test_token".to_string(),
            model_configuration: None,
            all_files: vec![],
            open_files: vec![],
            shell: "bash".to_string(),
            aide_rules: None,
            reasoning: false,
            semantic_search: false,
            is_devtools_context: false,
        };

        let response = server.agent_session_chat(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
            match response.unwrap().response {
                AgentResponseType::Thinking(_) => (),
                AgentResponseType::Action(_) => (),
                AgentResponseType::Error(_) => panic!("Unexpected error response"),
            }
        }
        assert!(got_response, "Expected at least one response");
    }

    #[tokio::test]
    async fn test_agent_session_edit() {
        let server = setup_test_server().await;
        let request = AgentEditRequest {
            file_path: "test.rs".to_string(),
            content: "fn main() {}".to_string(),
            edit_range: Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 0, character: 0 },
            },
            context: None,
        };

        let response = server.agent_session_edit(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
            let edit_response = response.unwrap();
            assert!(!edit_response.edited_content.is_empty());
        }
        assert!(got_response, "Expected at least one response");
    }

    #[tokio::test]
    async fn test_agent_tool_use() {
        let server = setup_test_server().await;
        let request = ToolUseRequest {
            tool_name: "test_tool".to_string(),
            parameters: std::collections::HashMap::new(),
            context: None,
        };

        let response = server.agent_tool_use(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
            let tool_response = response.unwrap();
            assert!(tool_response.success);
        }
        assert!(got_response, "Expected at least one response");
    }

    #[tokio::test]
    async fn test_edit_file() {
        let server = setup_test_server().await;
        let request = EditFileRequest {
            file_path: "test.rs".to_string(),
            content: "fn main() {}".to_string(),
        };

        let response = server.edit_file(Request::new(request)).await.unwrap();
        let response = response.into_inner();
        assert!(response.success, "Expected successful file edit");
    }

    #[tokio::test]
    async fn test_extract_documentation() {
        let server = setup_test_server().await;
        let request = DocumentationRequest {
            file_content: "/// Test doc\nfn main() {}".to_string(),
            language: "rust".to_string(),
        };

        let response = server.extract_documentation(Request::new(request)).await.unwrap();
        let response = response.into_inner();
        assert!(!response.documentation_strings.is_empty(), "Expected documentation strings");
    }

    #[tokio::test]
    async fn test_validate_tree_sitter() {
        let server = setup_test_server().await;
        let request = TreeSitterValidationRequest {
            content: "fn main() {}".to_string(),
            language: "rust".to_string(),
        };

        let response = server.validate_tree_sitter(Request::new(request)).await.unwrap();
        let response = response.into_inner();
        assert!(response.is_valid, "Expected valid tree-sitter parse");
    }

    #[tokio::test]
    async fn test_agent_session_plan() {
        let server = setup_test_server().await;
        let request = AgentSessionRequest {
            session_id: "test_session".to_string(),
            exchange_id: "test_exchange".to_string(),
            editor_url: "test_url".to_string(),
            query: "test query".to_string(),
            user_context: None,
            repo_ref: None,
            root_directory: "/test".to_string(),
            project_labels: vec![],
            codebase_search: false,
            access_token: "test_token".to_string(),
            model_configuration: None,
            all_files: vec![],
            open_files: vec![],
            shell: "bash".to_string(),
            aide_rules: None,
            reasoning: false,
            semantic_search: false,
            is_devtools_context: false,
        };

        let response = server.agent_session_plan(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
        }
        assert!(got_response, "Expected at least one response");
    }

    #[tokio::test]
    async fn test_agent_session_plan_iterate() {
        let server = setup_test_server().await;
        let request = AgentSessionRequest {
            session_id: "test_session".to_string(),
            exchange_id: "test_exchange".to_string(),
            editor_url: "test_url".to_string(),
            query: "test query".to_string(),
            user_context: None,
            repo_ref: None,
            root_directory: "/test".to_string(),
            project_labels: vec![],
            codebase_search: false,
            access_token: "test_token".to_string(),
            model_configuration: None,
            all_files: vec![],
            open_files: vec![],
            shell: "bash".to_string(),
            aide_rules: None,
            reasoning: false,
            semantic_search: false,
            is_devtools_context: false,
        };

        let response = server.agent_session_plan_iterate(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
        }
        assert!(got_response, "Expected at least one response");
    }

    #[tokio::test]
    async fn test_agent_session_edit_anchored() {
        let server = setup_test_server().await;
        let request = AgentSessionRequest {
            session_id: "test_session".to_string(),
            exchange_id: "test_exchange".to_string(),
            editor_url: "test_url".to_string(),
            query: "test query".to_string(),
            user_context: None,
            repo_ref: None,
            root_directory: "/test".to_string(),
            project_labels: vec![],
            codebase_search: false,
            access_token: "test_token".to_string(),
            model_configuration: None,
            all_files: vec![],
            open_files: vec![],
            shell: "bash".to_string(),
            aide_rules: None,
            reasoning: false,
            semantic_search: false,
            is_devtools_context: false,
        };

        let response = server.agent_session_edit_anchored(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
        }
        assert!(got_response, "Expected at least one response");
    }

    #[tokio::test]
    async fn test_agent_session_edit_agentic() {
        let server = setup_test_server().await;
        let request = AgentSessionRequest {
            session_id: "test_session".to_string(),
            exchange_id: "test_exchange".to_string(),
            editor_url: "test_url".to_string(),
            query: "test query".to_string(),
            user_context: None,
            repo_ref: None,
            root_directory: "/test".to_string(),
            project_labels: vec![],
            codebase_search: false,
            access_token: "test_token".to_string(),
            model_configuration: None,
            all_files: vec![],
            open_files: vec![],
            shell: "bash".to_string(),
            aide_rules: None,
            reasoning: false,
            semantic_search: false,
            is_devtools_context: false,
        };

        let response = server.agent_session_edit_agentic(Request::new(request)).await.unwrap();
        let mut stream = response.into_inner();
        
        let mut got_response = false;
        while let Some(response) = stream.next().await {
            got_response = true;
            assert!(response.is_ok());
        }
        assert!(got_response, "Expected at least one response");
    }
}