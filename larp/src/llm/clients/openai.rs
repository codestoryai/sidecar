//! Client which can help us talk to openai

use async_openai::{
    config::OpenAIConfig,
    types::{ChatCompletionRequestMessage, CreateChatCompletionRequestArgs},
    Client,
};
use async_trait::async_trait;

use crate::llm::provider::{LLMProvider, OpenAIProvider};

use super::types::{
    LLMClientCompletionRequest, LLMClientCompletionResponse, LLMClientError, LLMClientMessage,
    LLMType,
};

/// A client structure for interacting with the OpenAI model using `async-openai` library
pub struct OpenAIClient {
    /// Underlying OpenAI client instance
    client: Client<OpenAIConfig>,
}

impl OpenAIClient {
    pub fn new(provider: OpenAIProvider) -> Self {
        let config = OpenAIConfig::new().with_api_key(provider.api_key);
        Self {
            client: Client::with_config(config),
        }
    }

    /// Function to get OpenAI model name based on given LLMType
    pub fn model(&self, model: &LLMType) -> Option<String> {
        match model {
            LLMType::Gpt3_5_16k => Some("gpt-3.5-turbo-16k-0613".to_owned()), // GPT 3.5 with 16K context window and token usage
            LLMType::Gpt4 => Some("gpt-4-0613".to_owned()), // GPT 4 with standard context window and token usage
            LLMType::Gpt4Turbo => Some("gpt-4-1106-preview".to_owned()), // GPT 4 Turbo with larger context window and increased throughput
            LLMType::Gpt4_32k => Some("gpt-4-32k-0613".to_owned()), // GPT 4 with extended context window of 32K tokens
            _ => None, // For other unsupported models, we return None
        }
    }

#[async_trait]
impl LLMProvider for OpenAIClient {
    async fn stream_completion(
        &self,
        request: LLMClientCompletionRequest,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let model = self.model(request.model());
        if model.is_none() {
            return Err(LLMClientError::ModelNotSupported);
        }
        let model = model.unwrap();
        let messages = self.messages(request.messages());
        let mut request_builder_args = CreateChatCompletionRequestArgs::default();
        let mut request_builder = request_builder_args
            .model(model)
            .messages(messages)
            .temperature(request.temperature())
            .stream(true);
        if let Some(frequency_penalty) = request.frequency_penalty() {
            request_builder = request_builder.frequency_penalty(frequency_penalty);
        }
        let request = request_builder.build()?;
        let mut buffer = String::new();
        let mut stream = self.client.chat().create_stream(request).await?;

        while let Some(response) = stream.next().await {
            match response {
                Ok(response) => {
                    let response = response.choices().get(0).unwrap();
                    let text = response.text().to_owned();
                    buffer.push_str(&text);
                    let _ = sender.send(LLMClientCompletionResponse::new(
                        buffer.to_owned(),
                        Some(text),
                        model.to_owned(),
                    ));
                }
                Err(err) => {
                    dbg!(err);
                    break;
                }
            }
        }
    }

    async fn completion(
        &self,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let result = self.stream_completion(request, sender).await?;
        Ok(result)
    }
}
