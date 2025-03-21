use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};
use thiserror::Error;

use crate::{
    agentic::{
        symbol::identifier::LLMProperties,
        tool::{
            code_symbol::important::{
                CodeSymbolImportantResponse, CodeSymbolWithSteps, CodeSymbolWithThinking,
            },
            errors::ToolError,
            input::ToolInput,
            kw_search::tag_search::TagSearch,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
        },
    },
    repomap::tag::{Tag, TagIndex},
};

use super::{
    google_studio::GoogleStudioKeywordSearch,
    types::{KeywordsReply, KeywordsReplyError},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeywordSearchQuery {
    user_query: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    repo_name: String,
    root_request_id: String,
    case_sensitive: bool,
    tag_index: TagIndex,
}

impl KeywordSearchQuery {
    pub fn new(
        user_query: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        repo_name: String,
        root_request_id: String,
        case_sensitive: bool,
        tag_index: TagIndex,
    ) -> Self {
        Self {
            user_query,
            llm,
            provider,
            api_keys,
            repo_name,
            root_request_id,
            case_sensitive,
            tag_index,
        }
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

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn tag_index(&self) -> &TagIndex {
        &self.tag_index
    }
}

pub struct KeywordSearchQueryResponse {
    words: Vec<String>,
}

impl KeywordSearchQueryResponse {
    pub fn words(&self) -> &[String] {
        self.words.as_slice()
    }
}

#[derive(Debug, Error)]
pub enum KeywordSearchQueryError {
    #[error("Wrong LLM for input: {0}")]
    WrongLLM(LLMType),
}

#[async_trait]
pub trait KeywordSearch {
    async fn get_keywords(
        &self,
        request: &KeywordSearchQuery,
    ) -> Result<KeywordsReply, KeywordsReplyError>;
}

pub struct KeywordSearchQueryBroker {
    llms: HashMap<LLMType, Box<dyn KeywordSearch + Send + Sync>>,
}

impl KeywordSearchQueryBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        let mut llms: HashMap<LLMType, Box<dyn KeywordSearch + Send + Sync>> = Default::default();

        // flash all the wayyyy
        llms.insert(
            LLMType::GeminiProFlash,
            Box::new(GoogleStudioKeywordSearch::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );

        // even when Sonnet is passed in user_query, we still want to use GeminiProFlash
        llms.insert(
            LLMType::ClaudeSonnet,
            Box::new(GoogleStudioKeywordSearch::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );

        Self { llms }
    }
}

#[async_trait]
impl Tool for KeywordSearchQueryBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request = input.keyword_search_query()?;
        if let Some(implementation) = self.llms.get(request.llm()) {
            let response = implementation
                .get_keywords(&request)
                .await
                .map_err(|e| ToolError::KeywordSearchError(e))?;

            let tag_searcher = TagSearch::new();

            let key_tags: Vec<&Tag> = response
                .keywords()
                .into_iter()
                .flat_map(|kw| match tag_searcher.search(request.tag_index(), kw) {
                    Ok(tag_set) => tag_set.into_iter().collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                })
                .collect();

            let symbols = key_tags
                .iter()
                .map(|tag| {
                    CodeSymbolWithThinking::new(
                        tag.name.to_string(),
                        "".to_string(),
                        tag.fname.display().to_string(),
                    )
                })
                .collect();

            let ordered_symbols = key_tags
                .iter()
                .map(|tag| {
                    CodeSymbolWithSteps::new(
                        tag.name.to_string(),
                        vec![],
                        false,
                        tag.fname.display().to_string(),
                    )
                })
                .collect();

            let response = CodeSymbolImportantResponse::new(symbols, ordered_symbols);

            Ok(ToolOutput::KeywordSearch(response))
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
