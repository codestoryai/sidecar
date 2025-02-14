//! Client which can help us talk to openai

use async_openai::{
    config::OpenAIConfig,
    error::OpenAIError,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs, Choice,
        CreateChatCompletionRequestArgs, CreateCompletionRequestArgs, FunctionCall,
    },
    Client,
};
use async_trait::async_trait;
use futures::StreamExt;
use tracing::error;

use crate::provider::LLMProviderAPIKeys;

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
    LLMClientCompletionStringRequest, LLMClientError, LLMClientMessage, LLMClientRole, LLMType,
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PartialOpenAIResponse {
    choices: Vec<Choice>,
}

enum OpenAIClientType {
    OpenAIClient(Client<OpenAIConfig>),
}

pub struct OpenAICompatibleClient {}

impl OpenAICompatibleClient {
    pub fn new() -> Self {
        Self {}
    }

    pub fn model(&self, model: &LLMType) -> Option<String> {
        match model {
            LLMType::GPT3_5_16k => Some("gpt-3.5-turbo-16k-0613".to_owned()),
            LLMType::Gpt4 => Some("gpt-4-0613".to_owned()),
            LLMType::Gpt4Turbo => Some("gpt-4-1106-preview".to_owned()),
            LLMType::Gpt4_32k => Some("gpt-4-32k-0613".to_owned()),
            LLMType::DeepSeekCoder33BInstruct => Some("deepseek-coder-33b".to_owned()),
            LLMType::DeepSeekCoder6BInstruct => Some("deepseek-coder-6b".to_owned()),
            LLMType::DeepSeekR1 => Some("deepseek-r1".to_owned()),
            LLMType::CodeLlama13BInstruct => Some("codellama-13b".to_owned()),
            LLMType::Llama3_1_8bInstruct => Some("llama-3.1-8b-instant".to_owned()),
            LLMType::Llama3_1_70bInstruct => Some("llama-3.1-70b-versatile".to_owned()),
            LLMType::Custom(name) => Some(name.to_owned()),
            _ => None,
        }
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
        _llm_model: &LLMType,
    ) -> Result<OpenAIClientType, LLMClientError> {
        match api_key {
            LLMProviderAPIKeys::OpenAICompatible(openai_compatible) => {
                let config = OpenAIConfig::new()
                    .with_api_key(openai_compatible.api_key)
                    .with_api_base(openai_compatible.api_base);
                Ok(OpenAIClientType::OpenAIClient(Client::with_config(config)))
            }
            LLMProviderAPIKeys::Deepseek(deepseek_config) => {
                let config = OpenAIConfig::new()
                    .with_api_key(deepseek_config.api_key)
                    .with_api_base(deepseek_config.api_base);
                Ok(OpenAIClientType::OpenAIClient(Client::with_config(config)))
            }
            _ => Err(LLMClientError::WrongAPIKeyType),
        }
    }

    fn generate_completion_openai_client(
        &self,
        api_key: LLMProviderAPIKeys,
        _llm_model: &LLMType,
    ) -> Result<Client<OpenAIConfig>, LLMClientError> {
        match api_key {
            LLMProviderAPIKeys::OpenAICompatible(openai_compatible) => {
                let config = OpenAIConfig::new()
                    .with_api_key(openai_compatible.api_key)
                    .with_api_base(openai_compatible.api_base);
                Ok(Client::with_config(config))
            }
            LLMProviderAPIKeys::Deepseek(deepseek_config) => {
                let config = OpenAIConfig::new()
                    .with_api_key(deepseek_config.api_key)
                    .with_api_base(deepseek_config.api_base);
                Ok(Client::with_config(config))
            }
            _ => Err(LLMClientError::WrongAPIKeyType),
        }
    }
}

#[async_trait]
impl LLMClient for OpenAICompatibleClient {
    fn client(&self) -> &crate::provider::LLMProvider {
        &crate::provider::LLMProvider::OpenAICompatible
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
        let messages = self.messages(request.messages())?;
        let mut request_builder_args = CreateChatCompletionRequestArgs::default();
        let mut request_builder = request_builder_args
            .model(model.to_owned())
            .messages(messages)
            .temperature(request.temperature())
            .stream(true);
        if let Some(frequency_penalty) = request.frequency_penalty() {
            request_builder = request_builder.frequency_penalty(frequency_penalty);
        }
        let request = request_builder.build()?;
        let mut buffer = String::new();
        let client = self.generate_openai_client(api_key, llm_model)?;

        // TODO(skcd): Bad code :| we are repeating too many things but this
        // just works and we need it right now
        match client {
            // OpenAIClientType::AzureClient(client) => {
            //     let stream_maybe = client.chat().create_stream(request).await;
            //     if stream_maybe.is_err() {
            //         return Err(LLMClientError::OpenAPIError(stream_maybe.err().unwrap()));
            //     } else {
            //         dbg!("no error here");
            //     }
            //     let mut stream = stream_maybe.unwrap();
            //     while let Some(response) = stream.next().await {
            //         match response {
            //             Ok(response) => {
            //                 let delta = response
            //                     .choices
            //                     .get(0)
            //                     .map(|choice| choice.delta.content.to_owned())
            //                     .flatten()
            //                     .unwrap_or("".to_owned());
            //                 let _value = response
            //                     .choices
            //                     .get(0)
            //                     .map(|choice| choice.delta.content.as_ref())
            //                     .flatten();
            //                 buffer.push_str(&delta);
            //                 let _ = sender.send(LLMClientCompletionResponse::new(
            //                     buffer.to_owned(),
            //                     Some(delta),
            //                     model.to_owned(),
            //                 ));
            //             }
            //             Err(err) => {
            //                 dbg!(err);
            //                 break;
            //             }
            //         }
            //     }
            // }
            OpenAIClientType::OpenAIClient(client) => {
                let mut stream = client.chat().create_stream(request).await?;
                while let Some(response) = stream.next().await {
                    match response {
                        Ok(response) => {
                            let response = response
                                .choices
                                .get(0)
                                .ok_or(LLMClientError::FailedToGetResponse)?;
                            let text = response.delta.content.to_owned();
                            if let Some(text) = text {
                                buffer.push_str(&text);
                                let _ = sender.send(LLMClientCompletionResponse::new(
                                    buffer.to_owned(),
                                    Some(text),
                                    model.to_owned(),
                                ));
                            }
                        }
                        Err(err) => {
                            error!("Stream error in OpenAI completion: {:?}", err);
                            break;
                        }
                    }
                }
            }
        }

        Ok(LLMClientCompletionResponse::new(buffer, None, model))
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
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionStringRequest,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let llm_model = request.model();
        let model = self.model(llm_model);
        if model.is_none() {
            return Err(LLMClientError::UnSupportedModel);
        }
        let model = model.unwrap();
        let mut request_builder_args = CreateCompletionRequestArgs::default();
        let mut request_builder = request_builder_args
            .model(model.to_owned())
            .prompt(request.prompt())
            .temperature(request.temperature())
            .stream(true);
        if let Some(frequency_penalty) = request.frequency_penalty() {
            request_builder = request_builder.frequency_penalty(frequency_penalty);
        }
        if let Some(stop_tokens) = request.stop_words() {
            request_builder = request_builder.stop(stop_tokens.to_vec());
        }
        if let Some(max_tokens) = request.get_max_tokens() {
            request_builder = request_builder.max_tokens(max_tokens as u16);
        }
        let request = request_builder.build()?;
        let mut buffer = String::new();
        let client = self.generate_completion_openai_client(api_key, llm_model)?;
        let mut stream = client.completions().create_stream(request).await?;
        while let Some(response) = stream.next().await {
            match response {
                Ok(response) => {
                    let response = response
                        .choices
                        .get(0)
                        .ok_or(LLMClientError::FailedToGetResponse)?;
                    let text = response.text.to_owned();
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
                Err(err) => {
                    match err {
                        OpenAIError::JSONDeserialize(error) => {
                            let string_error = error.to_string();
                            // now that we have the string error, we can see
                            // if we can parse it properly by chopping off the
                            // prefix here: `failed deserialization of: `
                            if let Some(stripped_string) =
                                string_error.strip_prefix("failed deserialization of:")
                            {
                                let choices =
                                    serde_json::from_str::<PartialOpenAIResponse>(stripped_string);
                                match choices {
                                    Ok(choices) => {
                                        let response = choices
                                            .choices
                                            .get(0)
                                            .ok_or(LLMClientError::FailedToGetResponse)?;
                                        let text = response.text.to_owned();
                                        buffer.push_str(&text);
                                        if let Err(e) =
                                            sender.send(LLMClientCompletionResponse::new(
                                                buffer.to_owned(),
                                                Some(text),
                                                model.to_owned(),
                                            ))
                                        {
                                            error!("Failed to send completion response: {}", e);
                                            return Err(LLMClientError::SendError(e));
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to parse partial response: {:?}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        _ => {
                            error!("OpenAI-compatible stream error: {:?}", err);
                            break;
                        }
                    }
                    break;
                }
            }
        }
        Ok(buffer)
    }
}