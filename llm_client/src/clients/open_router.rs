use std::collections::HashMap;

use crate::provider::{LLMProvider, LLMProviderAPIKeys};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
    LLMClientCompletionStringRequest, LLMClientError, LLMClientMessageImage, LLMClientRole,
    LLMType,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum OpenRouterCacheType {
    #[serde(rename = "ephemeral")]
    Ephemeral,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterCacheControl {
    r#type: OpenRouterCacheType,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename = "image_url")]
struct OpenRouterImageSource {
    url: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct OpenRouterRequestMessageToolCall {
    id: String,
    r#type: String,
    function: ToolFunction,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum OpenRouterRequestMessageType {
    #[serde(rename = "text")]
    Text {
        text: String,
        cache_control: Option<OpenRouterCacheControl>,
    },
    #[serde(rename = "image_url")]
    Image { image_url: OpenRouterImageSource },
    #[serde(rename = "tool_result")]
    ToolReturn {
        tool_use_id: String,
        content: String,
    },
}

impl OpenRouterRequestMessageType {
    pub fn text(message: String) -> Self {
        Self::Text {
            text: message,
            cache_control: None,
        }
    }

    pub fn tool_return(tool_use_id: String, content: String) -> Self {
        Self::ToolReturn {
            tool_use_id,
            content,
        }
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

    pub fn set_cache_control(mut self) -> Self {
        if let Self::Text {
            text: _,
            ref mut cache_control,
        } = self
        {
            *cache_control = Some(OpenRouterCacheControl {
                r#type: OpenRouterCacheType::Ephemeral,
            });
        }
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterRequestMessageToolUse {
    r#type: String,
    function: serde_json::Value,
}

impl OpenRouterRequestMessageToolUse {
    pub fn from_llm_tool_use(mut llm_tool: serde_json::Value) -> OpenRouterRequestMessageToolUse {
        if let Some(obj) = llm_tool.as_object_mut() {
            // If "input_schema" exists, remove it and reinsert it as "parameters".
            // this is since the tool format is set to what anthropic preferes
            if let Some(input_schema) = obj.remove("input_schema") {
                obj.insert("parameters".to_string(), input_schema);
            } else {
                if let Some(name) = obj.get("name") {
                    if name == "str_replace_editor" {
                        obj.insert("parameters".to_owned(), serde_json::json!({
                            "type": "object",
                            "properties": {
                                "command": {
                                    "type": "string",
                                    "enum": ["view", "create", "str_replace", "insert", "undo_edit"],
                                    "description": "The commands to run. Allowed options are: `view`, `create`, `str_replace`, `insert`, `undo_edit`."
                                },
                                "file_text": {
                                    "description": "Required parameter of `create` command, with the content of the file to be created.",
                                    "type": "string"
                                },
                                "insert_line": {
                                    "description": "Required parameter of `insert` command. The `new_str` will be inserted AFTER the line `insert_line` of `path`.",
                                    "type": "integer"
                                },
                                "new_str": {
                                    "description": "Required parameter of `str_replace` command containing the new string. Required parameter of `insert` command containing the string to insert.",
                                    "type": "string"
                                },
                                "old_str": {
                                    "description": "Required parameter of `str_replace` command containing the string in `path` to replace.",
                                    "type": "string"
                                },
                                "path": {
                                    "description": "Absolute path to file or directory, e.g. `/repo/file.py` or `/repo`.",
                                    "type": "string"
                                },
                                "view_range": {
                                    "description": "Optional parameter of `view` command when `path` points to a file. If none is given, the full file is shown. If provided, the file will be shown in the indicated line number range, e.g. [11, 12] will show lines 11 and 12. Indexing at 1 to start. Setting `[start_line, -1]` shows all lines from `start_line` to the end of the file.",
                                    "items": {
                                        "type": "integer"
                                    },
                                    "type": "array"
                                }
                            },
                            "required": ["command", "path"]
                        }));
                        obj.insert("description".to_owned(), serde_json::Value::String(r#"Custom editing tool for viewing, creating and editing files
* State is persistent across command calls and discussions with the user
* If `path` is a file, `view` displays the result of applying `cat -n`. If `path` is a directory, `view` lists non-hidden files and directories up to 2 levels deep
* The `create` command cannot be used if the specified `path` already exists as a file
* If a `command` generates a long output, it will be truncated and marked with `<response clipped>` 
* The `undo_edit` command will revert the last edit made to the file at `path`

Notes for using the `str_replace` command:
* The `old_str` parameter should match EXACTLY one or more consecutive lines from the original file. Be mindful of whitespaces!
* If the `old_str` parameter is not unique in the file, the replacement will not be performed. Make sure to include enough context in `old_str` to make it unique
* The `new_str` parameter should contain the edited lines that should replace the `old_str`"#.to_owned()));
                    }
                }
            }
        }

        Self {
            r#type: "function".to_owned(),
            function: llm_tool,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterRequestMessage {
    role: String,
    content: Vec<OpenRouterRequestMessageType>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<OpenRouterRequestMessageToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    // this is the tool name which we are using
    name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterRequest {
    model: String,
    temperature: f32,
    messages: Vec<OpenRouterRequestMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenRouterRequestMessageToolUse>,
    stream: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolFunction {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionCall {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub(crate) index: i32,
    pub(crate) id: Option<String>,

    #[serde(rename = "type")]
    pub(crate) call_type: Option<String>,

    #[serde(rename = "function")]
    pub(crate) function_details: Option<ToolFunction>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterResponseDelta {
    #[serde(rename = "role")]
    pub(crate) role: Option<String>,

    #[serde(rename = "content")]
    pub(crate) content: Option<String>,

    #[serde(rename = "function_call")]
    pub(crate) function_call: Option<FunctionCall>,

    #[serde(rename = "tool_calls")]
    pub(crate) tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterResponseChoice {
    pub(crate) delta: OpenRouterResponseDelta,
    pub(crate) finish_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterResponse {
    pub(crate) model: Option<String>,
    pub(crate) choices: Vec<OpenRouterResponseChoice>,
}

impl OpenRouterRequest {
    pub fn from_chat_request(request: LLMClientCompletionRequest, model: String) -> Self {
        let tools = request
            .messages()
            .into_iter()
            .map(|message| message.tools())
            .flatten()
            .map(|tool| OpenRouterRequestMessageToolUse::from_llm_tool_use(tool.clone()))
            .collect::<Vec<_>>();

        // now we also want to generate the final value here after getting the tool
        Self {
            model,
            temperature: request.temperature(),
            messages: request
                .messages()
                .into_iter()
                .map(|message| {
                    let role = message.role().to_string();
                    println!("message::role::{role}");
                    // if !message.tool_return_value().is_empty() {
                    //     role = "tool".to_owned();
                    // }
                    println!("message::role::tool-return-role({role})");

                    // get the tool call id
                    let tool_call_id = message
                        .tool_return_value()
                        .into_iter()
                        .map(|tool_return| tool_return.tool_use_id().to_owned())
                        .collect::<Vec<_>>()
                        .first()
                        .map(|tool_return_id| tool_return_id.to_owned());

                    // get the tool_return_values over here
                    let tool_return_values = message
                        .tool_return_value()
                        .into_iter()
                        .map(|tool_return| tool_return.content().to_owned())
                        .collect::<Vec<_>>()
                        .first()
                        .map(|tool_return_content| tool_return_content.to_owned());

                    // get the tool name
                    let tool_return_name = message
                        .tool_return_value()
                        .into_iter()
                        .map(|tool_return| tool_return.tool_name().to_owned())
                        .collect::<Vec<_>>()
                        .first()
                        .map(|tool_return_content| tool_return_content.to_owned());
                    let open_router_message =
                        OpenRouterRequestMessage {
                            role,
                            content: {
                                if tool_call_id.is_some() && tool_return_values.is_some() {
                                    vec![OpenRouterRequestMessageType::text(
                                        // tool_call_id.clone().expect("is_some to hold").to_owned(),
                                        tool_return_values.expect("is_some to hold").to_owned(),
                                    )]
                                } else {
                                    let content = message.content();
                                    let images = message.images();

                                    // enable cache point if its set, open-router requires
                                    // this for anthropic models, we would need to toggle it
                                    // for openai-models later on
                                    let is_cache_enabled = message.is_cache_point();
                                    let mut content_messaage =
                                        OpenRouterRequestMessageType::text(content.to_owned());
                                    if is_cache_enabled {
                                        content_messaage = content_messaage.set_cache_control();
                                    }
                                    vec![content_messaage]
                                        .into_iter()
                                        .chain(images.into_iter().map(|image| {
                                            OpenRouterRequestMessageType::image(image)
                                        }))
                                        .collect()
                                }
                            },
                            tool_calls: {
                                if message.role() == &LLMClientRole::Assistant {
                                    message
                                        .tool_use_value()
                                        .into_iter()
                                        .map(|tool_use| OpenRouterRequestMessageToolCall {
                                            id: tool_use.id().to_owned(),
                                            r#type: "function".to_owned(),
                                            function: ToolFunction {
                                                name: Some(tool_use.name().to_owned()),
                                                arguments: Some(tool_use.input().to_string()),
                                            },
                                        })
                                        .collect()
                                } else {
                                    vec![]
                                }
                            },
                            tool_call_id,
                            // this is the tool return name
                            name: tool_return_name,
                        };
                    open_router_message
                })
                .collect(),
            tools,
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
        println!("tool_use_request::({:?})", &serde_json::to_string(&request));
        let mut response_stream = self
            .client
            .post(base_url)
            .bearer_auth(auth_key)
            .header("HTTP-Referer", "https://aide.dev/")
            .header("X-Title", "aide")
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
        let mut response_stream = self
            .client
            .post(base_url)
            .bearer_auth(auth_key)
            .header("HTTP-Referer", "https://aide.dev/")
            .header("X-Title", "aide")
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
