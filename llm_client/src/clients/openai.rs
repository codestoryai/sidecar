//! Client which can help us talk to openai

use async_openai::{
    config::{AzureConfig, OpenAIConfig},
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestDeveloperMessageArgs,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs, FunctionCall,
        ReasoningEffort, ResponseFormat,
    },
    Client,
};
use async_trait::async_trait;
use futures::StreamExt;
use tracing::{debug, error};

use crate::provider::LLMProviderAPIKeys;

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse, LLMClientError,
    LLMClientMessage, LLMClientRole, LLMType,
};

enum OpenAIClientType {
    AzureClient(Client<AzureConfig>),
    OpenAIClient(Client<OpenAIConfig>),
}

pub struct OpenAIClient {}

impl OpenAIClient {
    pub fn new() -> Self {
        Self {}
    }

    pub fn model(&self, model: &LLMType) -> Option<String> {
        match model {
            LLMType::GPT3_5_16k => Some("gpt-3.5-turbo-16k-0613".to_owned()),
            LLMType::Gpt4 => Some("gpt-4-0613".to_owned()),
            LLMType::Gpt4Turbo => Some("gpt-4-1106-preview".to_owned()),
            LLMType::Gpt4_32k => Some("gpt-4-32k-0613".to_owned()),
            LLMType::Gpt4O => Some("gpt-4o".to_owned()),
            LLMType::Gpt4OMini => Some("gpt-4o-mini".to_owned()),
            LLMType::DeepSeekCoder33BInstruct => Some("deepseek-coder-33b".to_owned()),
            LLMType::Llama3_1_8bInstruct => Some("llama-3.1-8b-instant".to_owned()),
            LLMType::O1Preview => Some("o1-preview".to_owned()),
            LLMType::O1 => Some("o1".to_owned()),
            LLMType::O3MiniHigh => Some("o3-mini".to_owned()),
            _ => None,
        }
    }

    pub fn o1_preview_messages(
        &self,
        messages: &[LLMClientMessage],
    ) -> Result<Vec<ChatCompletionRequestMessage>, LLMClientError> {
        let formatted_messages = messages
            .into_iter()
            .map(|message| {
                let role = message.role();
                match role {
                    LLMClientRole::User => ChatCompletionRequestUserMessageArgs::default()
                        .content(message.content().to_owned())
                        .build()
                        .map(|message| ChatCompletionRequestMessage::User(message))
                        .map_err(|e| LLMClientError::OpenAPIError(e)),
                    // system messages for reasoning models are developer messages
                    LLMClientRole::System => ChatCompletionRequestDeveloperMessageArgs::default()
                        .content(message.content().to_owned())
                        .build()
                        .map(|message| ChatCompletionRequestMessage::Developer(message))
                        .map_err(|e| LLMClientError::OpenAPIError(e)),
                    LLMClientRole::Assistant => match message.get_function_call() {
                        Some(function_call) => ChatCompletionRequestAssistantMessageArgs::default()
                            .function_call(FunctionCall {
                                name: function_call.name().to_owned(),
                                arguments: function_call.arguments().to_owned(),
                            })
                            .build()
                            .map(|message| ChatCompletionRequestMessage::Assistant(message))
                            .map_err(|e| LLMClientError::OpenAPIError(e)),
                        None => ChatCompletionRequestAssistantMessageArgs::default()
                            .content(message.content().to_owned())
                            .build()
                            .map(|message| ChatCompletionRequestMessage::Assistant(message))
                            .map_err(|e| LLMClientError::OpenAPIError(e)),
                    },
                    LLMClientRole::Function => match message.get_function_call() {
                        Some(function_call) => ChatCompletionRequestAssistantMessageArgs::default()
                            .content(message.content().to_owned())
                            .function_call(FunctionCall {
                                name: function_call.name().to_owned(),
                                arguments: function_call.arguments().to_owned(),
                            })
                            .build()
                            .map(|message| ChatCompletionRequestMessage::Assistant(message))
                            .map_err(|e| LLMClientError::OpenAPIError(e)),
                        None => Err(LLMClientError::FunctionCallNotPresent),
                    },
                }
            })
            .collect::<Vec<_>>();
        formatted_messages
            .into_iter()
            .collect::<Result<Vec<ChatCompletionRequestMessage>, LLMClientError>>()
    }

    pub fn messages(
        &self,
        messages: &[LLMClientMessage],
    ) -> Result<Vec<ChatCompletionRequestMessage>, LLMClientError> {
        let formatted_messages = messages
            .into_iter()
            .map(|message| {
                let role = message.role();
                match role {
                    LLMClientRole::User => ChatCompletionRequestUserMessageArgs::default()
                        .content(message.content().to_owned())
                        .build()
                        .map(|message| ChatCompletionRequestMessage::User(message))
                        .map_err(|e| LLMClientError::OpenAPIError(e)),
                    LLMClientRole::System => ChatCompletionRequestSystemMessageArgs::default()
                        .content(message.content().to_owned())
                        .build()
                        .map(|message| ChatCompletionRequestMessage::System(message))
                        .map_err(|e| LLMClientError::OpenAPIError(e)),
                    // TODO(skcd): This might be wrong, but for now its okay as we
                    // do not use these branches at all
                    LLMClientRole::Assistant => match message.get_function_call() {
                        Some(function_call) => ChatCompletionRequestAssistantMessageArgs::default()
                            .function_call(FunctionCall {
                                name: function_call.name().to_owned(),
                                arguments: function_call.arguments().to_owned(),
                            })
                            .build()
                            .map(|message| ChatCompletionRequestMessage::Assistant(message))
                            .map_err(|e| LLMClientError::OpenAPIError(e)),
                        None => ChatCompletionRequestAssistantMessageArgs::default()
                            .content(message.content().to_owned())
                            .build()
                            .map(|message| ChatCompletionRequestMessage::Assistant(message))
                            .map_err(|e| LLMClientError::OpenAPIError(e)),
                    },
                    LLMClientRole::Function => match message.get_function_call() {
                        Some(function_call) => ChatCompletionRequestAssistantMessageArgs::default()
                            .content(message.content().to_owned())
                            .function_call(FunctionCall {
                                name: function_call.name().to_owned(),
                                arguments: function_call.arguments().to_owned(),
                            })
                            .build()
                            .map(|message| ChatCompletionRequestMessage::Assistant(message))
                            .map_err(|e| LLMClientError::OpenAPIError(e)),
                        None => Err(LLMClientError::FunctionCallNotPresent),
                    },
                }
            })
            .collect::<Vec<_>>();
        formatted_messages
            .into_iter()
            .collect::<Result<Vec<ChatCompletionRequestMessage>, LLMClientError>>()
    }

    fn generate_openai_client(
        &self,
        api_key: LLMProviderAPIKeys,
        llm_model: &LLMType,
    ) -> Result<OpenAIClientType, LLMClientError> {
        // special escape hatch for deepseek-coder-33b
        if matches!(llm_model, LLMType::DeepSeekCoder33BInstruct) {
            // if we have deepseek coder 33b right now, then we should return an openai
            // client right here, this is a hack to get things working and the provider
            // needs to be updated to support this
            return match api_key {
                LLMProviderAPIKeys::OpenAIAzureConfig(api_key) => {
                    let config = OpenAIConfig::new()
                        .with_api_key(api_key.api_key)
                        .with_api_base(api_key.api_base);
                    Ok(OpenAIClientType::OpenAIClient(Client::with_config(config)))
                }
                _ => Err(LLMClientError::WrongAPIKeyType),
            };
        }
        match api_key {
            LLMProviderAPIKeys::OpenAI(api_key) => {
                let config = OpenAIConfig::new().with_api_key(api_key.api_key);
                Ok(OpenAIClientType::OpenAIClient(Client::with_config(config)))
            }
            LLMProviderAPIKeys::OpenAIAzureConfig(azure_config) => {
                let config = AzureConfig::new()
                    .with_api_base(azure_config.api_base)
                    .with_api_key(azure_config.api_key)
                    .with_deployment_id(azure_config.deployment_id)
                    .with_api_version(azure_config.api_version);
                Ok(OpenAIClientType::AzureClient(Client::with_config(config)))
            }
            _ => Err(LLMClientError::WrongAPIKeyType),
        }
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    fn client(&self) -> &crate::provider::LLMProvider {
        &crate::provider::LLMProvider::OpenAI
    }

    async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<LLMClientCompletionResponse, LLMClientError> {
        let llm_model = request.model();
        let model = self.model(llm_model);
        if model.is_none() {
            return Err(LLMClientError::UnSupportedModel);
        }
        let model = model.unwrap();
        let messages = if llm_model == &LLMType::O1Preview
            || llm_model == &LLMType::O1
            || llm_model == &LLMType::O1Mini
            || llm_model == &LLMType::O3MiniHigh
        {
            self.o1_preview_messages(request.messages())?
        } else {
            self.messages(request.messages())?
        };
        let mut request_builder_args = CreateChatCompletionRequestArgs::default();
        let mut request_builder = request_builder_args
            .model(model.to_owned())
            .messages(messages);

        // o1 and o3-mini do not support streaming on the api
        if llm_model != &LLMType::O1 {
            request_builder = request_builder.stream(true);
        }
        // set response format to text
        request_builder.response_format(ResponseFormat::Text);

        // we cannot set temperature for o1 and o3-mini-high
        if llm_model != &LLMType::O1 && llm_model != &LLMType::O3MiniHigh {
            request_builder = request_builder.temperature(request.temperature());
        }

        if let Some(reasoning_effort) = request.reasoning_effort() {
            match reasoning_effort {
                crate::clients::types::ReasoningEffort::Low => {
                    request_builder = request_builder.reasoning_effort(ReasoningEffort::Low);
                }
                crate::clients::types::ReasoningEffort::Medium => {
                    request_builder = request_builder.reasoning_effort(ReasoningEffort::Medium);
                }
                crate::clients::types::ReasoningEffort::High => {
                    request_builder = request_builder.reasoning_effort(ReasoningEffort::High);
                }
            }
        } else if llm_model == &LLMType::O1 || llm_model == &LLMType::O3MiniHigh {
            request_builder = request_builder.reasoning_effort(ReasoningEffort::High);
        }

        if let Some(frequency_penalty) = request.frequency_penalty() {
            request_builder = request_builder.frequency_penalty(frequency_penalty);
        }
        let request = request_builder.build()?;
        let mut buffer = String::new();
        let client = self.generate_openai_client(api_key, llm_model)?;

        // TODO(skcd): Bad code :| we are repeating too many things but this
        // just works and we need it right now
        match client {
            OpenAIClientType::AzureClient(client) => {
                let stream_maybe = client.chat().create_stream(request).await;
                if stream_maybe.is_err() {
                    return Err(LLMClientError::OpenAPIError(stream_maybe.err().unwrap()));
                } else {
                    debug!("Stream created successfully");
                }
                let mut stream = stream_maybe.unwrap();
                while let Some(response) = stream.next().await {
                    match response {
                        Ok(response) => {
                            let delta = response
                                .choices
                                .get(0)
                                .map(|choice| choice.delta.content.to_owned())
                                .flatten()
                                .unwrap_or("".to_owned());
                            let _value = response
                                .choices
                                .get(0)
                                .map(|choice| choice.delta.content.as_ref())
                                .flatten();
                            buffer.push_str(&delta);
                            if let Err(e) = sender.send(LLMClientCompletionResponse::new(
                                buffer.to_owned(),
                                Some(delta),
                                model.to_owned(),
                            )) {
                                error!("Failed to send completion response: {}", e);
                                return Err(LLMClientError::SendError(e));
                            }
                        }
                        Err(err) => {
                            error!("Azure stream error: {:?}", err);
                            break;
                        }
                    }
                }
            }
            OpenAIClientType::OpenAIClient(client) => {
                if llm_model == &LLMType::O1 {
                    let completion = client.chat().create(request).await?;
                    let response = completion
                        .choices
                        .get(0)
                        .ok_or(LLMClientError::FailedToGetResponse)?;
                    let content = response
                        .message
                        .content
                        .as_ref()
                        .ok_or(LLMClientError::FailedToGetResponse)?;
                    if let Err(e) = sender.send(LLMClientCompletionResponse::new(
                        content.to_owned(),
                        Some(content.to_owned()),
                        model.to_owned(),
                    )) {
                        error!("Failed to send completion response: {}", e);
                        return Err(LLMClientError::SendError(e));
                    }
                    buffer = content.to_owned();
                } else {
                    if llm_model == &LLMType::O3MiniHigh {
                        debug!("o3-mini-high");
                    }
                    let mut stream = client.chat().create_stream(request).await?;
                    while let Some(response) = stream.next().await {
                        debug!("OpenAI stream response: {:?}", &response);
                        match response {
                            Ok(response) => {
                                let response = response
                                    .choices
                                    .get(0)
                                    .ok_or(LLMClientError::FailedToGetResponse)?;
                                let text = response.delta.content.to_owned();
                                if let Some(text) = text {
                                    buffer.push_str(&text);
                                    if let Err(e) = sender.send(LLMClientCompletionResponse::new(
                                        buffer.to_owned(),
                                        Some(text),
                                        model.to_owned(),
                                    )) {
                                        error!("Failed to send completion response: {}", e);
                                        return Err(LLMClientError::SendError(e));
                                    }
                                }
                            }
                            Err(err) => {
                                error!("OpenAI stream error: {:?}", err);
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(LLMClientCompletionResponse::new(
            buffer,
            None,
            model.to_owned(),
        ))
    }

    async fn completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let result = self.stream_completion(api_key, request, sender).await?;
        Ok(result.answer_up_until_now().to_owned())
    }

    async fn stream_prompt_completion(
        &self,
        _api_key: LLMProviderAPIKeys,
        _request: super::types::LLMClientCompletionStringRequest,
        _sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        Err(LLMClientError::OpenAIDoesNotSupportCompletion)
    }
}
