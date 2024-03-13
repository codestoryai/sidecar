use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::provider::LLMProviderAPIKeys;

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
    LLMClientCompletionStringRequest, LLMClientError, LLMType,
};

#[derive(serde::Serialize, Debug, Clone)]
struct FireworksAIRequestString {
    prompt: String,
    model: String,
    temperature: f32,
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<String>,
    stream: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct FireworksAIChatCompletion {
    choices: Vec<ChoiceCompletionChat>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct FireworksAIRequestCompletion {
    choices: Vec<ChoiceCompletion>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct ChoiceCompletionDelta {
    content: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct ChoiceCompletionChat {
    delta: ChoiceCompletionDelta,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct ChoiceCompletion {
    text: String,
}

#[derive(serde::Serialize, Debug, Clone)]
struct FireworksAIMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize, Debug, Clone)]
struct FireworksAIRequestChat {
    messages: Vec<FireworksAIMessage>,
    model: String,
    temperature: f32,
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<String>,
    stream: bool,
}

impl FireworksAIRequestChat {
    fn from_message(request: LLMClientCompletionRequest) -> FireworksAIRequestChat {
        FireworksAIRequestChat {
            messages: request
                .messages()
                .into_iter()
                .map(|message| FireworksAIMessage {
                    role: message.role().to_string(),
                    content: message.content().to_owned(),
                })
                .collect(),
            model: FireworksAIClient::model_str(request.model()).expect("to be present"),
            temperature: request.temperature(),
            max_tokens: request.get_max_tokens(),
            stop: request
                .stop_words()
                .map(|stop_words| stop_words.into_iter().map(|s| s.to_owned()).collect()),
            stream: true,
        }
    }
}

impl FireworksAIRequestString {
    fn from_string_message(
        string_request: LLMClientCompletionStringRequest,
    ) -> FireworksAIRequestString {
        FireworksAIRequestString {
            prompt: string_request.prompt().to_owned(),
            model: FireworksAIClient::model_str(string_request.model()).expect("to be present"),
            temperature: string_request.temperature(),
            max_tokens: string_request.get_max_tokens(),
            stop: string_request.stop_words().map(|stop_words| {
                stop_words
                    .into_iter()
                    .map(|s| s.to_owned())
                    .take(4)
                    .collect()
            }),
            stream: true,
        }
    }
}

pub struct FireworksAIClient {
    client: reqwest::Client,
    base_url: String,
}

impl FireworksAIClient {
    pub fn new() -> Self {
        let client = reqwest::Client::new();
        Self {
            client,
            base_url: "https://api.fireworks.ai/inference/v1".to_owned(),
        }
    }

    fn completion_endpoint(&self) -> String {
        format!("{}/completions", self.base_url)
    }

    fn chat_endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn model_str(model: &LLMType) -> Option<String> {
        match model {
            LLMType::CodeLlama13BInstruct => {
                Some("accounts/fireworks/models/llama-v2-13b-code".to_owned())
            }
            _ => None,
        }
    }

    fn generate_fireworks_ai_bearer_token(
        &self,
        api_key: LLMProviderAPIKeys,
    ) -> Result<String, LLMClientError> {
        match api_key {
            LLMProviderAPIKeys::FireworksAI(api_key) => Ok(api_key.api_key),
            _ => Err(LLMClientError::WrongAPIKeyType),
        }
    }
}

#[async_trait]
impl LLMClient for FireworksAIClient {
    fn client(&self) -> &crate::provider::LLMProvider {
        &crate::provider::LLMProvider::FireworksAI
    }

    async fn completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        self.stream_completion(api_key, request, sender).await
    }

    async fn stream_prompt_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionStringRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let model = FireworksAIClient::model_str(request.model())
            .ok_or(LLMClientError::UnSupportedModel)?;
        let bearer_token = self.generate_fireworks_ai_bearer_token(api_key)?;
        let request = FireworksAIRequestString::from_string_message(request);
        let mut response_stream = self
            .client
            .post(&self.completion_endpoint())
            .bearer_auth(bearer_token)
            .json(&request)
            .send()
            .await?
            .bytes_stream()
            .eventsource();

        let mut buffered_string = "".to_owned();
        while let Some(event) = response_stream.next().await {
            match event {
                Ok(event) => {
                    if &event.data == "[DONE]" {
                        continue;
                    }
                    let value = serde_json::from_str::<FireworksAIRequestCompletion>(&event.data)?;
                    buffered_string.push_str(&value.choices[0].text);
                    sender.send(LLMClientCompletionResponse::new(
                        buffered_string.to_owned(),
                        Some(value.choices[0].text.to_owned()),
                        model.to_owned(),
                    ))?;
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }

        Ok(buffered_string)
    }

    async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let model = FireworksAIClient::model_str(request.model())
            .ok_or(LLMClientError::UnSupportedModel)?;
        let bearer_token = self.generate_fireworks_ai_bearer_token(api_key)?;
        let request = FireworksAIRequestChat::from_message(request);
        let mut response_stream = self
            .client
            .post(&self.chat_endpoint())
            .bearer_auth(bearer_token)
            .json(&request)
            .send()
            .await?
            .bytes_stream()
            .eventsource();

        let mut buffered_string = "".to_owned();
        while let Some(event) = response_stream.next().await {
            match event {
                Ok(event) => {
                    if &event.data == "[DONE]" {
                        continue;
                    }
                    let value = serde_json::from_str::<FireworksAIChatCompletion>(&event.data)?;
                    buffered_string.push_str(&value.choices[0].delta.content);
                    sender.send(LLMClientCompletionResponse::new(
                        buffered_string.to_owned(),
                        Some(value.choices[0].delta.content.to_owned()),
                        model.to_owned(),
                    ))?;
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }

        Ok(buffered_string)
    }
}
