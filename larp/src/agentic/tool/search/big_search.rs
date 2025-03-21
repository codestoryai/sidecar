use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::{
    agentic::{
        symbol::{events::message_event::SymbolEventMessageProperties, identifier::LLMProperties},
        tool::{
            code_symbol::{important::CodeSymbolImportantResponse, types::CodeSymbolError},
            errors::ToolError,
            input::ToolInput,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
            search::{
                google_studio::GoogleStudioLLM,
                iterative::{IterativeSearchContext, IterativeSearchSystem},
                repository::Repository,
            },
        },
    },
    repomap::tag::TagIndex,
    tree_printer::tree::TreePrinter,
};

use super::iterative::UIEventHandler;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SearchType {
    Both,
}

#[derive(Debug, Clone)]
pub struct BigSearchRequest {
    user_query: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    root_directory: Option<String>,
    root_request_id: String,
    search_type: SearchType,
    message_properties: SymbolEventMessageProperties,
}

impl BigSearchRequest {
    pub fn new(
        user_query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        root_directory: Option<String>,
        root_request_id: String,
        search_type: SearchType,
        message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            user_query,
            llm,
            provider,
            api_keys,
            root_directory,
            root_request_id,
            search_type,
            message_properties,
        }
    }

    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn llm(&self) -> &LLMType {
        &self.llm
    }

    pub fn provider(&self) -> &LLMProvider {
        &self.provider
    }

    pub fn api_keys(&self) -> &LLMProviderAPIKeys {
        &self.api_keys
    }

    pub fn root_directory(&self) -> Option<&str> {
        self.root_directory.as_deref()
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }

    pub fn search_type(&self) -> &SearchType {
        &self.search_type
    }

    pub fn message_properties(&self) -> &SymbolEventMessageProperties {
        &self.message_properties
    }
}

#[async_trait]
pub trait BigSearch {
    async fn search(
        &self,
        input: BigSearchRequest,
    ) -> Result<CodeSymbolImportantResponse, CodeSymbolError>;
}

pub struct BigSearchBroker {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl BigSearchBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }

    pub fn llm_client(&self) -> Arc<LLMBroker> {
        self.llm_client.clone()
    }

    pub fn fail_over_llm(&self) -> LLMProperties {
        self.fail_over_llm.clone()
    }

    fn validate_root_directory(&self, request: &BigSearchRequest) -> Result<String, ToolError> {
        request
            .root_directory()
            .ok_or_else(|| ToolError::BigSearchError("Root directory is required".to_string()))
            .map(|s| s.to_string())
    }

    async fn create_repository(&self, root_directory: &str) -> Result<Repository, ToolError> {
        let tree = TreePrinter::to_string_stacked(Path::new(root_directory)).unwrap_or_default();
        let tag_index = TagIndex::from_path(Path::new(root_directory)).await;

        Ok(Repository::new(
            tree,
            "outline".to_owned(),
            tag_index,
            PathBuf::from(root_directory),
        ))
    }

    fn create_search_system(
        &self,
        repository: Repository,
        request: &BigSearchRequest,
    ) -> Result<IterativeSearchSystem<GoogleStudioLLM>, ToolError> {
        let iterative_search_context =
            IterativeSearchContext::new(Vec::new(), request.user_query().to_owned(), String::new());

        let google_studio_llm_config = GoogleStudioLLM::new(
            request.root_directory().unwrap_or_default().to_owned(),
            self.llm_client(),
            request.root_request_id().to_owned(),
        );

        let ui_event_handler_for_iterative_search =
            UIEventHandler::new(request.message_properties().to_owned());

        Ok(IterativeSearchSystem::new(
            iterative_search_context,
            repository,
            google_studio_llm_config,
            vec![Box::new(ui_event_handler_for_iterative_search)],
        ))
    }
}

pub enum IterativeSearchSeed {
    Tree(String),
}

#[async_trait]
impl Tool for BigSearchBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();
        let request = input.big_search_query()?;

        let root_directory = self.validate_root_directory(&request)?;

        let repository = self.create_repository(&root_directory).await?;

        let mut system = self.create_search_system(repository, &request)?;

        let results = system
            .run()
            .await
            .map_err(|e| ToolError::IterativeSearchError(e))?;

        let duration = start.elapsed();
        println!("BigSearchBroker::invoke::duration: {:?}", duration);

        Ok(ToolOutput::BigSearch(results))
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
