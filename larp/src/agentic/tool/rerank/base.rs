//! We have a basic structure for how reranking should work
//! The idea on a very high level is this:
//! fn rerank(input: Vec<Information>) -> Vec<Information>;
//! Information here can be code snippets or documentation or anything else
//!
//! We could use an embedding function + semantic-search, but the idea is to make
//! it composable to rerank using different methods

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientError, LLMType},
    provider::{LLMProvider, LLMProviderAPIKeys},
};
use thiserror::Error;

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::text_document::Range,
};

use super::listwise::anthropic::AnthropicReRank;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReRankCodeSnippet {
    fs_file_path: String,
    range: Range,
    content: String,
    language: String,
}

impl ReRankCodeSnippet {
    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReRankDocument {
    document_name: String,
    document_path: String,
    content: String,
}

impl ReRankDocument {
    pub fn document_name(&self) -> &str {
        &self.document_name
    }

    pub fn document_path(&self) -> &str {
        &self.document_path
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReRankWebExtract {
    url: String,
    content: String,
}

impl ReRankWebExtract {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ReRankEntry {
    CodeSnippet(ReRankCodeSnippet),
    Document(ReRankDocument),
    WebExtract(ReRankWebExtract),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReRankEntries {
    id: i64,
    entry: ReRankEntry,
}

impl ReRankEntries {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn entry(&self) -> &ReRankEntry {
        &self.entry
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReRankEntriesForBroker {
    entries: Vec<ReRankEntries>,
    metadata: ReRankRequestMetadata,
}

impl ReRankEntriesForBroker {
    pub fn new(entries: Vec<ReRankEntries>, metadata: ReRankRequestMetadata) -> Self {
        Self { entries, metadata }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReRankRequestMetadata {
    model: LLMType,
    query: String,
    provider_keys: LLMProviderAPIKeys,
    provider: LLMProvider,
}

impl ReRankRequestMetadata {
    pub fn new(
        model: LLMType,
        query: String,
        provider_keys: LLMProviderAPIKeys,
        provider: LLMProvider,
    ) -> Self {
        Self {
            model,
            query,
            provider_keys,
            provider,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn model(&self) -> &LLMType {
        &self.model
    }

    pub fn provider(&self) -> &LLMProvider {
        &self.provider
    }

    pub fn provider_keys(&self) -> &LLMProviderAPIKeys {
        &self.provider_keys
    }
}

#[derive(Debug, Error)]
pub enum ReRankError {
    #[error("LLMError: {0}")]
    LlmClientError(LLMClientError),
}

#[async_trait]
pub trait ReRank {
    async fn rerank(
        &self,
        input: Vec<ReRankEntries>,
        metadata: ReRankRequestMetadata,
    ) -> Result<Vec<ReRankEntries>, ReRankError>;
}

pub struct ReRankBroker {
    rerankers: HashMap<LLMType, Box<dyn ReRank + Send + Sync>>,
}

impl ReRankBroker {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        let mut rerankers: HashMap<LLMType, Box<dyn ReRank + Send + Sync>> = Default::default();
        rerankers.insert(
            LLMType::ClaudeHaiku,
            Box::new(AnthropicReRank::new(llm_client.clone())),
        );
        rerankers.insert(
            LLMType::ClaudeSonnet,
            Box::new(AnthropicReRank::new(llm_client.clone())),
        );
        rerankers.insert(
            LLMType::ClaudeOpus,
            Box::new(AnthropicReRank::new(llm_client)),
        );
        Self { rerankers }
    }
}

#[async_trait]
impl Tool for ReRankBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let rerank_input = input.is_rerank()?;
        let entries = rerank_input.entries;
        let metadata = rerank_input.metadata;
        if let Some(reranker) = self.rerankers.get(&metadata.model) {
            reranker
                .rerank(entries, metadata.clone())
                .await
                .map_err(|e| ToolError::ReRankingError(e))
                .map(|output| {
                    ToolOutput::rerank_entries(ReRankEntriesForBroker::new(output, metadata))
                })
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
