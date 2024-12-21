use std::collections::HashMap;

use async_openai::types::CreateChatCompletionStreamResponse;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    clients::open_router::OpenRouterResponse,
    provider::{CodeStoryLLMTypes, LLMProvider, LLMProviderAPIKeys},
};

use super::{
    open_router::OpenRouterRequest,
    togetherai::TogetherAIClient,
    types::{
        LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
        LLMClientCompletionStringRequest, LLMClientError, LLMClientRole, LLMType,
    },
};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct LMStudioResponse {
    model: String,
    choices: Vec<Choice>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Choice {
    text: String,
}

pub struct CodeStoryClient {
    client: reqwest::Client,
    api_base: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CodeStoryMessage {
    role: String,
    content: String,
    cache_point: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CodeStoryRequestOptions {
    temperature: f32,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CodeStoryRequest {
    messages: Vec<CodeStoryMessage>,
    options: CodeStoryRequestOptions,
    model: String,
    system: Option<String>,
    max_tokens: Option<usize>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CodeStoryRequestPrompt {
    prompt: String,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_tokens: Option<Vec<String>>,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CodeStoryChoice {
    pub text: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CodeStoryPromptResponse {
    choices: Vec<CodeStoryChoice>,
}

impl CodeStoryRequestPrompt {
    fn from_string_request(
        request: LLMClientCompletionStringRequest,
    ) -> Result<Self, LLMClientError> {
        let model = TogetherAIClient::model_str(request.model());
        match model {
            Some(model) => Ok(Self {
                prompt: request.prompt().to_owned(),
                model,
                temperature: request.temperature(),
                stop_tokens: request.stop_words().map(|stop_tokens| stop_tokens.to_vec()),
                max_tokens: request.get_max_tokens(),
            }),
            None => Err(LLMClientError::OpenAIDoesNotSupportCompletion),
        }
    }
}

impl CodeStoryRequest {
    fn from_chat_request(request: LLMClientCompletionRequest, model: String) -> Self {
        let llm_type = request.model().clone();
        Self {
            messages: request
                .messages()
                .into_iter()
                .filter_map(|message| match message.role() {
                    LLMClientRole::System => {
                        if llm_type.is_anthropic() {
                            None
                        } else {
                            Some(CodeStoryMessage {
                                role: "system".to_owned(),
                                content: message.content().to_owned(),
                                cache_point: message.is_cache_point(),
                            })
                        }
                    }
                    LLMClientRole::User => Some(CodeStoryMessage {
                        role: "user".to_owned(),
                        content: message.content().to_owned(),
                        cache_point: message.is_cache_point(),
                    }),
                    LLMClientRole::Function => {
                        if llm_type.is_anthropic() {
                            None
                        } else {
                            Some(CodeStoryMessage {
                                role: "function".to_owned(),
                                content: message.content().to_owned(),
                                cache_point: message.is_cache_point(),
                            })
                        }
                    }
                    LLMClientRole::Assistant => Some(CodeStoryMessage {
                        role: "assistant".to_owned(),
                        content: message.content().to_owned(),
                        cache_point: message.is_cache_point(),
                    }),
                })
                .collect(),
            options: CodeStoryRequestOptions {
                temperature: request.temperature(),
            },
            system: request
                .messages()
                .iter()
                .filter(|message| message.role().is_system())
                .next()
                .map(|message| message.content().to_owned()),
            model,
            max_tokens: request.get_max_tokens(),
        }
    }
}

impl CodeStoryClient {
    pub fn new(api_base: &str) -> Self {
        Self {
            api_base: api_base.to_owned(),
            client: reqwest::Client::new(),
        }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn gpt3_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/chat-3")
    }

    pub fn gpt4_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/chat-4")
    }

    pub fn gpt4_preview_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/chat-4-turbo")
    }

    pub fn o1_preview_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/chat-o1") // new endpoint on anton
    }

    pub fn together_api_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/together-api")
    }

    pub fn anthropic_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/claude-api")
    }

    pub fn gemini_endpoint(&self, api_base: &str) -> String {
        format!("{api_base}/google-ai")
    }

    pub fn rerank_endpoint(&self) -> String {
        let api_base = &self.api_base;
        format!("{api_base}/rerank")
    }

    pub fn model_name(&self, model: &LLMType) -> Result<String, LLMClientError> {
        match model {
            LLMType::GPT3_5_16k => Ok("gpt-3.5-turbo-16k-0613".to_owned()),
            LLMType::Gpt4 => Ok("gpt-4-0613".to_owned()),
            LLMType::Gpt4OMini => Ok("gpt-4o-mini".to_owned()),
            LLMType::Gpt4Turbo => Ok("gpt-4-vision-preview".to_owned()),
            LLMType::CodeLlama13BInstruct => Ok("codellama/CodeLlama-13b-Instruct-hf".to_owned()),
            LLMType::CodeLlama7BInstruct => Ok("codellama/CodeLlama-7b-Instruct-hf".to_owned()),
            LLMType::DeepSeekCoder33BInstruct => {
                Ok("deepseek-ai/deepseek-coder-33b-instruct".to_owned())
            }
            LLMType::ClaudeSonnet => Ok("claude-3-5-sonnet-20241022".to_owned()), // updated to latest sonnet
            LLMType::ClaudeHaiku => Ok("claude-3-5-haiku-20241022".to_owned()), // updated to latest haiku
            LLMType::GeminiPro => Ok("gemini-1.5-pro".to_owned()),
            LLMType::GeminiProFlash => Ok("gemini-1.5-flash".to_owned()),
            LLMType::O1Preview => Ok("o1-preview".to_owned()), // o1 baby
            _ => Err(LLMClientError::UnSupportedModel),
        }
    }

    pub fn model_endpoint_tool_use(&self, _model: &LLMType) -> Result<String, LLMClientError> {
        Ok(format!("{}/claude-api-tool-use", self.api_base))
    }

    pub fn model_endpoint(&self, model: &LLMType) -> Result<String, LLMClientError> {
        match model {
            LLMType::GPT3_5_16k => Ok(self.gpt3_endpoint(&self.api_base)),
            LLMType::Gpt4 => Ok(self.gpt4_endpoint(&self.api_base)),
            LLMType::Gpt4Turbo => Ok(self.gpt4_preview_endpoint(&self.api_base)),
            LLMType::Gpt4OMini => Ok(self.gpt4_preview_endpoint(&self.api_base)),
            LLMType::O1Preview => Ok(self.o1_preview_endpoint(&self.api_base)),
            LLMType::CodeLlama13BInstruct
            | LLMType::CodeLlama7BInstruct
            | LLMType::DeepSeekCoder33BInstruct => Ok(self.together_api_endpoint(&self.api_base)),
            LLMType::ClaudeSonnet | LLMType::ClaudeHaiku => {
                Ok(self.anthropic_endpoint(&self.api_base))
            }
            LLMType::GeminiPro | LLMType::GeminiProFlash => {
                Ok(self.gemini_endpoint(&self.api_base))
            }
            // we do not allow this to be overriden yet
            LLMType::CohereRerankV3 => Ok(self.rerank_endpoint()),
            _ => Err(LLMClientError::UnSupportedModel),
        }
    }

    pub fn model_prompt_endpoint(&self, model: &LLMType) -> Result<String, LLMClientError> {
        match model {
            LLMType::GPT3_5_16k | LLMType::Gpt4 | LLMType::Gpt4Turbo | LLMType::Gpt4_32k => {
                Err(LLMClientError::UnSupportedModel)
            }
            _ => Ok(self.together_api_endpoint(&self.api_base)),
        }
    }

    // returns codestory access token
    fn access_token(&self, api_key: LLMProviderAPIKeys) -> Result<String, LLMClientError> {
        match api_key {
            LLMProviderAPIKeys::CodeStory(api_key) => Ok(api_key.access_token),
            _ => Err(LLMClientError::WrongAPIKeyType),
        }
    }

    pub async fn stream_completion_with_tool(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        _metadata: HashMap<String, String>,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<(String, Vec<(String, (String, String))>), LLMClientError> {
        let model = self.model_name(request.model())?;
        let endpoint = self.model_endpoint_tool_use(request.model())?;
        println!("endpoint::{}", &endpoint);

        // get access token from api_key
        let access_token = self.access_token(api_key)?;

        let request = OpenRouterRequest::from_chat_request(request, model.to_owned());
        let mut response_stream = self
            .client
            .post(endpoint)
            .header("X-Accel-Buffering", "no")
            .header("Authorization", format!("Bearer {}", access_token))
            .json(&request)
            .send()
            .await?
            .bytes_stream()
            .eventsource();
        let mut buffered_stream = "".to_owned();
        // controls which tool we will be using if any
        let mut tool_use_indication: Vec<(String, (String, String))> = vec![];

        // handle all the tool parameters that are coming
        // we will use a global tracker over here
        // format to support: https://gist.github.com/theskcd/4d5b0f1a859be812bffbb0548e733233
        let mut curernt_tool_use: Option<String> = None;
        let current_tool_use_ref = &mut curernt_tool_use;
        let mut current_tool_use_id: Option<String> = None;
        let current_tool_use_id_ref = &mut current_tool_use_id;
        let mut running_tool_input = "".to_owned();
        let running_tool_input_ref = &mut running_tool_input;

        while let Some(event) = response_stream.next().await {
            match event {
                Ok(event) => {
                    if &event.data == "[DONE]" {
                        continue;
                    }
                    println!("stream_completion_with_tool:({:?})", &event.data);
                    let value = serde_json::from_str::<OpenRouterResponse>(&event.data)?;
                    let first_choice = &value.choices[0];
                    if let Some(content) = first_choice.delta.content.as_ref() {
                        buffered_stream = buffered_stream + &content;
                        sender.send(LLMClientCompletionResponse::new(
                            buffered_stream.to_owned(),
                            Some(content.to_owned()),
                            model.to_owned(),
                        ))?;
                    }

                    if let Some(finish_reason) = first_choice.finish_reason.as_ref() {
                        if finish_reason == "tool_calls" {
                            if let (Some(current_tool_use), Some(current_tool_use_id)) = (
                                current_tool_use_ref.clone(),
                                current_tool_use_id_ref.clone(),
                            ) {
                                tool_use_indication.push((
                                    current_tool_use.to_owned(),
                                    (
                                        current_tool_use_id.to_owned(),
                                        running_tool_input_ref.to_owned(),
                                    ),
                                ));
                            }
                            // now empty the tool use tracked
                            *current_tool_use_ref = None;
                            *running_tool_input_ref = "".to_owned();
                            *current_tool_use_id_ref = None;
                        }
                    }
                    if let Some(tool_calls) = first_choice.delta.tool_calls.as_ref() {
                        tool_calls.into_iter().for_each(|tool_call| {
                            let _tool_call_index = tool_call.index;
                            if let Some(function_details) = tool_call.function_details.as_ref() {
                                if let Some(tool_id) = tool_call.id.clone() {
                                    *current_tool_use_id_ref = Some(tool_id.to_owned());
                                }
                                if let Some(name) = function_details.name.clone() {
                                    *current_tool_use_ref = Some(name.to_owned());
                                }
                                if let Some(arguments) = function_details.arguments.clone() {
                                    *running_tool_input_ref =
                                        running_tool_input_ref.to_owned() + &arguments;
                                }
                            }
                        })
                    }
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }
        Ok((buffered_stream, tool_use_indication))
    }
}

#[async_trait]
impl LLMClient for CodeStoryClient {
    fn client(&self) -> &LLMProvider {
        &LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None })
    }

    async fn completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        self.stream_completion(api_key, request, sender).await
    }

    // codestory stream woooo
    async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let model = self.model_name(request.model())?;
        let endpoint = self.model_endpoint(request.model())?;

        // get access token from api_key
        let access_token = self.access_token(api_key)?;

        let request = CodeStoryRequest::from_chat_request(request, model.to_owned());
        let mut response_stream = self
            .client
            .post(endpoint)
            .header("X-Accel-Buffering", "no")
            .header("Authorization", format!("Bearer {}", access_token))
            .json(&request)
            .send()
            .await?
            .bytes_stream()
            .eventsource();

        let mut buffered_stream = "".to_owned();
        while let Some(event) = response_stream.next().await {
            match event {
                Ok(event) => {
                    if &event.data == "[DONE]" {
                        continue;
                    }
                    // we just proxy back the openai response back here
                    let response =
                        serde_json::from_str::<CreateChatCompletionStreamResponse>(&event.data);
                    match response {
                        Ok(response) => {
                            let delta = response
                                .choices
                                .get(0)
                                .map(|choice| choice.delta.content.to_owned())
                                .flatten()
                                .unwrap_or("".to_owned());
                            buffered_stream.push_str(&delta);
                            sender.send(LLMClientCompletionResponse::new(
                                buffered_stream.to_owned(),
                                Some(delta),
                                model.to_owned(),
                            ))?;
                        }
                        Err(e) => {
                            dbg!(e);
                        }
                    }
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }

        Ok(buffered_stream)
    }

    async fn stream_prompt_completion(
        &self,
        _api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionStringRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let llm_model = request.model();
        let endpoint = self.model_prompt_endpoint(&llm_model)?;
        let code_story_request = CodeStoryRequestPrompt::from_string_request(request)?;
        let model = code_story_request.model.to_owned();
        let mut response_stream = self
            .client
            .post(endpoint)
            .json(&code_story_request)
            .send()
            .await?
            .bytes_stream()
            .eventsource();
        let mut buffered_stream = "".to_owned();
        while let Some(event) = response_stream.next().await {
            match event {
                Ok(event) => {
                    if &event.data == "[DONE]" {
                        continue;
                    }
                    // we just proxy back the openai response back here
                    let response = serde_json::from_str::<CodeStoryPromptResponse>(&event.data);
                    match response {
                        Ok(response) => {
                            let delta = response
                                .choices
                                .get(0)
                                .map(|choice| choice.text.to_owned())
                                .unwrap_or("".to_owned());
                            buffered_stream.push_str(&delta);
                            sender.send(LLMClientCompletionResponse::new(
                                buffered_stream.to_owned(),
                                Some(delta),
                                model.to_owned(),
                            ))?;
                        }
                        Err(e) => {
                            dbg!(e);
                        }
                    }
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }
        Ok(buffered_stream)
    }
}
