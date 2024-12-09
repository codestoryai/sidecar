use std::collections::HashMap;

use crate::provider::{LLMProvider, LLMProviderAPIKeys};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
    LLMClientCompletionStringRequest, LLMClientError, LLMClientMessageImage, LLMType,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename = "image_url")]
struct OpenRouterImageSource {
    url: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum OpenRouterRequestMessageType {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    Image { image_url: OpenRouterImageSource },
}

impl OpenRouterRequestMessageType {
    pub fn text(message: String) -> Self {
        Self::Text { text: message }
    }

    pub fn image(image: &LLMClientMessageImage) -> Self {
        Self::Image {
            image_url: OpenRouterImageSource {
                url: format!(
                    r#"data:{};{},{}"#,
                    image.media(),
                    image.r#type(),
                    image.data()
                ),
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterRequestMessageToolUse {
    schema: serde_json::Value,
}

impl OpenRouterRequestMessageToolUse {
    pub fn from_llm_tool_use(mut llm_tool: serde_json::Value) -> serde_json::Value {
        if let Some(obj) = llm_tool.as_object_mut() {
            // If "input_schema" exists, remove it and reinsert it as "parameters".
            // this is since the tool format is set to what anthropic preferes
            if let Some(input_schema) = obj.remove("input_schema") {
                obj.insert("parameters".to_string(), input_schema);
            }
        }

        llm_tool
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterRequestMessage {
    role: String,
    content: Vec<OpenRouterRequestMessageType>,
    tools: Vec<OpenRouterRequestMessageToolUse>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterRequest {
    model: String,
    temperature: f32,
    messages: Vec<OpenRouterRequestMessage>,
    stream: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ToolFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FunctionCall {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ToolCall {
    index: i32,
    id: Option<String>,

    #[serde(rename = "type")]
    call_type: Option<String>,

    #[serde(rename = "function")]
    function_details: Option<ToolFunction>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OpenRouterResponseDelta {
    #[serde(rename = "role")]
    role: Option<String>,

    #[serde(rename = "content")]
    content: Option<String>,

    #[serde(rename = "function_call")]
    function_call: Option<FunctionCall>,

    #[serde(rename = "tool_calls")]
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterResponseChoice {
    delta: OpenRouterResponseDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OpenRouterResponse {
    model: String,
    choices: Vec<OpenRouterResponseChoice>,
}

impl OpenRouterRequest {
    pub fn from_chat_request(request: LLMClientCompletionRequest, model: String) -> Self {
        Self {
            model,
            temperature: request.temperature(),
            messages: request
                .messages()
                .into_iter()
                .map(|message| OpenRouterRequestMessage {
                    role: message.role().to_string(),
                    content: {
                        let content = message.content();
                        let images = message.images();
                        vec![OpenRouterRequestMessageType::text(content.to_owned())]
                            .into_iter()
                            .chain(
                                images
                                    .into_iter()
                                    .map(|image| OpenRouterRequestMessageType::image(image)),
                            )
                            .collect()
                    },
                    tools: vec![],
                })
                .collect(),
            stream: true,
        }
    }
}

pub struct OpenRouterClient {
    client: reqwest::Client,
}

impl OpenRouterClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub fn model(&self, model: &LLMType) -> Option<String> {
        match model {
            LLMType::ClaudeHaiku => Some("anthropic/claude-3-haiku".to_owned()),
            LLMType::ClaudeSonnet => Some("anthropic/claude-3.5-sonnet:beta".to_owned()),
            LLMType::ClaudeOpus => Some("anthropic/claude-3-opus".to_owned()),
            LLMType::Gpt4 => Some("openai/gpt-4".to_owned()),
            LLMType::Gpt4O => Some("openai/gpt-4o".to_owned()),
            LLMType::DeepSeekCoderV2 => Some("deepseek/deepseek-coder".to_owned()),
            LLMType::Custom(name) => Some(name.to_owned()),
            _ => None,
        }
    }

    fn generate_auth_key(&self, api_key: LLMProviderAPIKeys) -> Result<String, LLMClientError> {
        match api_key {
            LLMProviderAPIKeys::OpenRouter(open_router) => Ok(open_router.api_key),
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
        let base_url = "https://openrouter.ai/api/v1/chat/completions".to_owned();
        // pick this up from here, we need return type for the output we are getting form the stream
        let model = self
            .model(request.model())
            .ok_or(LLMClientError::WrongAPIKeyType)?;
        let auth_key = self.generate_auth_key(api_key)?;
        let request = OpenRouterRequest::from_chat_request(request, model.to_owned());
        println!("{:?}", serde_json::to_string(&request));
        let mut response_stream = dbg!(
            self.client
                .post(base_url)
                .bearer_auth(auth_key)
                .header("HTTP-Referer", "https://aide.dev/")
                .header("X-Title", "aide")
                .json(&request)
                .send()
                .await
        )?
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
                    let value = serde_json::from_str::<OpenRouterResponse>(&event.data)?;
                    let first_choice = &value.choices[0];
                    if let Some(content) = first_choice.delta.content.as_ref() {
                        buffered_stream = buffered_stream + &content;
                        sender.send(LLMClientCompletionResponse::new(
                            buffered_stream.to_owned(),
                            Some(content.to_owned()),
                            value.model,
                        ))?;
                    }

                    if let Some(finish_reason) = first_choice.finish_reason.as_ref() {
                        if finish_reason == "tool_use" {
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
impl LLMClient for OpenRouterClient {
    fn client(&self) -> &LLMProvider {
        &LLMProvider::OpenRouter
    }

    async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let base_url = "https://openrouter.ai/api/v1/chat/completions".to_owned();
        // pick this up from here, we need return type for the output we are getting form the stream
        let model = self
            .model(request.model())
            .ok_or(LLMClientError::WrongAPIKeyType)?;
        let auth_key = self.generate_auth_key(api_key)?;
        let request = OpenRouterRequest::from_chat_request(request, model.to_owned());
        println!("{:?}", serde_json::to_string(&request));
        let mut response_stream = dbg!(
            self.client
                .post(base_url)
                .bearer_auth(auth_key)
                .header("HTTP-Referer", "https://aide.dev/")
                .header("X-Title", "aide")
                .json(&request)
                .send()
                .await
        )?
        .bytes_stream()
        .eventsource();
        let mut buffered_stream = "".to_owned();
        while let Some(event) = response_stream.next().await {
            match event {
                Ok(event) => {
                    if &event.data == "[DONE]" {
                        continue;
                    }
                    let value = serde_json::from_str::<OpenRouterResponse>(&event.data)?;
                    let first_choice = &value.choices[0];
                    if let Some(content) = first_choice.delta.content.as_ref() {
                        buffered_stream = buffered_stream + &content;
                        sender.send(LLMClientCompletionResponse::new(
                            buffered_stream.to_owned(),
                            Some(content.to_owned()),
                            value.model,
                        ))?;
                    }
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }
        Ok(buffered_stream)
    }

    async fn completion(
        &self,
        _api_key: LLMProviderAPIKeys,
        _request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        todo!()
    }

    async fn stream_prompt_completion(
        &self,
        _api_key: LLMProviderAPIKeys,
        _request: LLMClientCompletionStringRequest,
        _sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        todo!()
    }
}
