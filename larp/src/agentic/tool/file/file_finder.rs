use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

use super::{
    important::FileImportantResponse, models::anthropic::AnthropicFileFinder,
    types::FileImportantError,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportantFilesFinderQuery {
    tree: String,
    user_query: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    repo_name: String,
    root_request_id: String,
}

impl ImportantFilesFinderQuery {
    pub fn new(
        tree: String,
        user_query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        repo_name: String,
        root_request_id: String,
    ) -> Self {
        Self {
            tree,
            user_query,
            llm,
            provider,
            api_keys,
            repo_name,
            root_request_id,
        }
    }

    pub fn tree(&self) -> &String {
        &self.tree
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
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

    pub fn repo_name(&self) -> &str {
        &self.repo_name
    }
}

#[async_trait]
pub trait ImportantFilesFinder {
    async fn find_important_files(
        &self,
        request: ImportantFilesFinderQuery,
    ) -> Result<FileImportantResponse, FileImportantError>;
}

pub struct ImportantFilesFinderBroker {
    llms: HashMap<LLMType, Box<dyn ImportantFilesFinder + Send + Sync>>,
}

impl ImportantFilesFinderBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        let mut llms: HashMap<LLMType, Box<dyn ImportantFilesFinder + Send + Sync>> =
            Default::default();

        // only smart models allowed
        llms.insert(
            LLMType::ClaudeSonnet,
            Box::new(AnthropicFileFinder::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );
        llms.insert(
            LLMType::GeminiPro,
            Box::new(AnthropicFileFinder::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );
        llms.insert(
            LLMType::GeminiProFlash,
            Box::new(AnthropicFileFinder::new(llm_client.clone(), fail_over_llm)),
        );
        Self { llms }
    }
}

#[async_trait]
impl Tool for ImportantFilesFinderBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request = input.important_files_finder_query()?;
        if let Some(implementation) = self.llms.get(request.llm()) {
            let output = implementation
                .find_important_files(request)
                .await
                .map_err(|e| ToolError::FileImportantError(e))?;

            Ok(ToolOutput::ImportantSymbols(output.into()))
        } else {
            Err(ToolError::LLMNotSupported)
        }
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
