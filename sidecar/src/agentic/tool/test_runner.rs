use std::collections::HashMap;
use std::time::Instant;
use anyhow::Result;
use tokio::time::{timeout, Duration};
use tokio::sync::mpsc;
use uuid::Uuid;
use crate::agent::{Agent, AgentState, ConversationMessage};
use crate::agentic::tool::plan::plan::Plan;

#[derive(Debug)]
pub struct TestCase {
    pub id: String,
    pub input: String,
    pub expected_state: AgentState,
    pub expected_output: Option<String>,
}

#[derive(Debug)]
pub struct TestResult {
    pub test_id: String,
    pub passed: bool,
    pub actual_state: AgentState,
    pub actual_output: Option<String>,
    pub error: Option<String>,
}

pub struct AgentTestRunner {
    agents: HashMap<Uuid, Agent>,
    results: Vec<TestResult>,
}

impl AgentTestRunner {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            results: Vec::new(),
        }
    }

    pub fn add_agent(&mut self, agent: Agent) -> Uuid {
        let id = Uuid::new_v4();
        self.agents.insert(id, agent);
        id
    }

    pub async fn run_test(&mut self, agent_id: Uuid, test: TestCase) -> Result<TestResult> {
        let agent = self.agents.get_mut(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        // Create channels for agent communication
        let (tx, _rx) = mpsc::channel(32);
        
        // Initialize test conversation
        let conversation = ConversationMessage::general_question(
            agent.session_id,
            test.expected_state.clone(),
            test.input.clone(),
        );
        agent.conversation_messages.push(conversation);

        // Run test plan
        let plan = Plan::new(test.input.clone());
        let result = agent.get_last_conversation_message().clone();

        let passed = match (&test.expected_output, &result.answer()) {
            (Some(expected), Some(actual)) => {
                expected.trim() == actual.answer_up_until_now.trim() 
                && test.expected_state == result.agent_state
            },
            (None, None) => test.expected_state == result.agent_state,
            _ => false,
        };

        let test_result = TestResult {
            test_id: test.id,
            passed,
            actual_state: result.agent_state,
            actual_output: result.answer().map(|a| a.answer_up_until_now),
            error: None,
        };

        self.results.push(test_result.clone());
        Ok(test_result)
    }

    pub fn get_results(&self) -> &[TestResult] {
        &self.results
    }

    pub fn clear_results(&mut self) {
        self.results.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentState, Answer};
    use crate::application::application::Application;
    use crate::repo::types::RepoRef;
    use crate::webserver::model_selection::LLMClientConfig;
    use llm_client::broker::LLMBroker;
    use llm_client::tokenizer::tokenizer::LLMTokenizer;
    use llm_prompts::chat::broker::LLMChatModelBroker;
    use llm_prompts::reranking::broker::ReRankBroker;
    use std::sync::Arc;
    use tokio::sync::mpsc::channel;

    async fn setup_test_agent() -> Agent {
        let (tx, _) = channel(32);
        let application = Application::new_test();
        let reporef = RepoRef::new("test".to_string());
        let session_id = Uuid::new_v4();
        let llm_broker = Arc::new(LLMBroker::new_test());
        let sql_db = application.sql_db.clone();
        let model_config = LLMClientConfig::default();
        let llm_tokenizer = Arc::new(LLMTokenizer::new());
        let chat_broker = Arc::new(LLMChatModelBroker::new());
        let reranker = Arc::new(ReRankBroker::new());

        Agent {
            application,
            reporef,
            session_id,
            conversation_messages: Vec::new(),
            llm_broker,
            sql_db,
            sender: tx,
            user_context: None,
            project_labels: Vec::new(),
            editor_parsing: Default::default(),
            model_config,
            llm_tokenizer,
            chat_broker,
            reranker,
            system_instruction: None,
        }
    }

    #[tokio::test]
    async fn test_basic_agent_test() {
        // Setup test case
        let test_case = TestCase {
            id: "test1".to_string(),
            input: "search for main function".to_string(),
            expected_state: AgentState::Search,
            expected_output: Some("Searching for main function...".to_string()),
        };

        // Create test runner
        let mut runner = AgentTestRunner::new();
        
        // Setup and add test agent
        let agent = setup_test_agent().await;
        let agent_id = runner.add_agent(agent);

        // Run test
        let result = runner.run_test(agent_id, test_case).await.unwrap();

        // Verify results
        assert!(result.passed);
        assert_eq!(result.actual_state, AgentState::Search);
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_with_custom_config() {
        let config = TestConfig::new()
            .with_timeout(10000)
            .with_max_retries(5)
            .with_mock_responses(true);

        let mut runner = AgentTestRunner::with_config(config);
        let agent = setup_test_agent().await;
        let agent_id = runner.add_agent(agent);

        let test_case = TestCase {
            id: "config_test".to_string(),
            input: "test with config".to_string(),
            expected_state: AgentState::Search,
            expected_output: None,
        };

        let result = runner.run_test(agent_id, test_case).await.unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn test_multiple_agents() {
        let mut runner = AgentTestRunner::new();
        
        // Create multiple agents
        let agent1 = setup_test_agent().await;
        let agent2 = setup_test_agent().await;
        
        let agent1_id = runner.add_agent(agent1);
        let agent2_id = runner.add_agent(agent2);

        // Test cases for each agent
        let test_cases = vec![
            TestCase {
                id: "agent1_test".to_string(),
                input: "search code".to_string(),
                expected_state: AgentState::Search,
                expected_output: None,
            },
            TestCase {
                id: "agent2_test".to_string(),
                input: "explain function".to_string(),
                expected_state: AgentState::Explain,
                expected_output: None,
            },
        ];

        // Run tests
        let result1 = runner.run_test(agent1_id, test_cases[0].clone()).await.unwrap();
        let result2 = runner.run_test(agent2_id, test_cases[1].clone()).await.unwrap();

        // Verify results
        assert_eq!(runner.get_results().len(), 2);
        assert_eq!(result1.actual_state, AgentState::Search);
        assert_eq!(result2.actual_state, AgentState::Explain);
    }
}