use std::collections::HashMap;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use logging::new_client;
use logging::parea::{PareaClient, PareaLogCompletion, PareaLogMessage};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info};

use crate::{
    clients::types::LLMClientUsageStatistics,
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use super::types::{
    LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
    LLMClientCompletionStringRequest, LLMClientError, LLMClientMessageImage, LLMClientToolReturn,
    LLMClientToolUse, LLMType,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum AnthropicCacheType {
    #[serde(rename = "ephemeral")]
    Ephemeral,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AnthropicCacheControl {
    r#type: AnthropicCacheType,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
enum AnthropicMessageContent {
    #[serde(rename = "text")]
    Text {
        text: String,
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "image")]
    Image {
        source: AnthropicImageSource,
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        cache_control: Option<AnthropicCacheControl>,
    },
    #[serde(rename = "tool_result")]
    ToolReturn {
        tool_use_id: String,
        content: String,
        cache_control: Option<AnthropicCacheControl>,
    },
}

impl AnthropicMessageContent {
    pub fn text(text: String, cache_control: Option<AnthropicCacheControl>) -> Self {
        Self::Text {
            text,
            cache_control,
        }
    }

    fn cache_control(mut self, cache_control_update: Option<AnthropicCacheControl>) -> Self {
        match &mut self {
            Self::Text { cache_control, .. } |
            Self::Image { cache_control, .. } |
            Self::ToolUse { cache_control, .. } |
            Self::ToolReturn { cache_control, .. } => {
                *cache_control = cache_control_update;
            }
        }
        self
    }

    pub fn image(llm_image: &LLMClientMessageImage) -> Self {
        Self::Image {
            source: AnthropicImageSource {
                type_: "base64".to_owned(),
                media_type: llm_image.media().to_owned(),
                data: llm_image.data().to_owned(),
            },
            cache_control: None,
        }
    }

    pub fn tool_use(llm_tool_use: &LLMClientMessageToolUse) -> Self {
        Self::ToolUse {
            id: llm_tool_use.id().to_owned(),
            name: llm_tool_use.name().to_owned(),
            input: llm_tool_use.input().clone(),
            cache_control: None,
        }
    }

    pub fn tool_return(llm_tool_return: &LLMClientMessageToolReturn) -> Self {
        Self::ToolReturn {
            tool_use_id: llm_tool_return.tool_use_id().to_owned(),
            content: llm_tool_return.content().to_owned(),
            cache_control: None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct AnthropicImageSource {
    r#type: String,     // e.g., "base64"
    media_type: String, // e.g., "image/png"
    data: String,       // base64-encoded image data
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicMessageContent>,
}

impl AnthropicMessage {
    pub fn new(role: String, content: String) -> Self {
        Self {
            role,
            content: vec![AnthropicMessageContent::text(content, None)],
        }
    }
}

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicEvent {
    #[serde(rename = "message_start")]
    MessageStart {
        #[serde(rename = "message")]
        message: MessageData,
    },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        #[serde(rename = "index")]
        _index: u32,
        content_block: ContentBlockStart,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        #[serde(rename = "index")]
        _index: u32,
        delta: ContentBlockDeltaType, // Using the new enum here
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        #[serde(rename = "index")]
        _index: u32,
    },
    #[serde(rename = "message_delta")]
    MessageDelta {
        #[serde(rename = "delta")]
        _delta: MessageDeltaData,
        #[serde(rename = "usage")]
        usage: Usage,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
}

#[derive(Debug, Deserialize)]
struct MessageData {
    // id: String,
    // #[serde(rename = "type")]
    // message_type: String,
    // role: String,
    // content: Vec<String>,
    // model: String,
    // stop_reason: Option<String>,
    // stop_sequence: Option<String>,
    usage: Usage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlockStart {
    #[serde(rename = "tool_use")]
    InputToolUse { name: String, id: String },
    #[serde(rename = "text")]
    TextDelta { text: String },
}

#[derive(Debug, Deserialize)]
struct MessageDeltaData {
    // stop_reason: String,
    // stop_sequence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlockDeltaType {
    #[serde(rename = "input_json_delta")]
    InputJsonDelta {
        #[serde(rename = "partial_json")]
        partial_json: String,
    },
    #[serde(rename = "text_delta")]
    TextDelta {
        text: String, // Reusing your `ContentBlockDelta.text` concept here
    },
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    system: Vec<AnthropicMessageContent>,
    messages: Vec<AnthropicMessage>,
    temperature: f32,
    max_tokens: Option<i32>,
    tools: Vec<LLMClientTool>,
    stream: bool,
    model: String,
}

impl AnthropicRequest {
    pub fn from_client_completion_request(
        completion_request: LLMClientCompletionRequest,
        model_str: String,
    ) -> Self {
        let temperature = completion_request.temperature();
        let max_tokens = match completion_request.get_max_tokens() {
            Some(tokens) => Some(tokens),
            None => Some(8192),
        };
        let messages = completion_request.messages();
        // grab the tools over here ONLY from the system message
        let tools = messages
            .iter()
            .find(|message| message.is_system_message())
            .map(|message| {
                message
                    .tools()
                    .into_iter()
                    .filter_map(|tool| Some(tool.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // First we try to find the system message
        let system_message = messages
            .iter()
            .find(|message| message.role().is_system())
            .map(|message| {
                let mut anthropic_message_content =
                    AnthropicMessageContent::text(message.content().to_owned(), None);
                if message.is_cache_point() {
                    anthropic_message_content =
                        anthropic_message_content.cache_control(Some(AnthropicCacheControl {
                            r#type: AnthropicCacheType::Ephemeral,
                        }));
                }
                vec![anthropic_message_content]
            })
            .unwrap_or_default();

        let messages = messages
            .into_iter()
            .filter(|message| message.role().is_user() || message.role().is_assistant())
            .map(|message| {
                let mut content = Vec::new();
        
                // Add text content if we don't have tool returns
                if message.tool_return_value().is_empty() && !message.content().is_empty() {
                    content.push(AnthropicMessageContent::text(message.content().to_owned(), None));
                }
                
                // Add images, tools and tool returns
                content.extend(message.images().iter().map(AnthropicMessageContent::image));
                content.extend(message.tool_use_value().iter().map(AnthropicMessageContent::tool_use));
                content.extend(message.tool_return_value().iter().map(AnthropicMessageContent::tool_return));

                // Apply cache control to last content if needed
                if message.is_cache_point() && !content.is_empty() {
                    let last_idx = content.len() - 1;
                    content[last_idx] = content[last_idx].clone().cache_control(Some(AnthropicCacheControl {
                        r#type: AnthropicCacheType::Ephemeral,
                    }));
                }

                let final_content = content;
                AnthropicMessage {
                    role: message.role().to_string(),
                    content: final_content,
                }
            })
            .collect::<Vec<_>>();

        AnthropicRequest {
            system: system_message,
            messages,
            temperature,
            max_tokens,
            tools,
            stream: true,
            model: model_str,
        }
    }
}

pub struct AnthropicClient {
    client: reqwest_middleware::ClientWithMiddleware,
    base_url: String,
    chat_endpoint: String,
}

impl AnthropicClient {
    pub fn new() -> Self {
        Self {
            client: new_client(),
            base_url: "https://api.anthropic.com".to_owned(),
            chat_endpoint: "/v1/messages".to_owned(),
        }
    }

    pub fn new_with_custom_urls(base_url: String, chat_endpoint: String) -> Self {
        Self {
            client: new_client(),
            base_url,
            chat_endpoint,
        }
    }

    pub fn chat_endpoint(&self) -> String {
        format!("{}{}", &self.base_url, &self.chat_endpoint)
    }

    fn generate_api_bearer_key(
        &self,
        api_key: LLMProviderAPIKeys,
    ) -> Result<String, LLMClientError> {
        match api_key {
            LLMProviderAPIKeys::Anthropic(api_key) => Ok(api_key.api_key),
            _ => Err(LLMClientError::WrongAPIKeyType),
        }
    }

    fn get_model_string(&self, llm_type: &LLMType) -> Result<String, LLMClientError> {
        match llm_type {
            LLMType::ClaudeOpus => Ok("claude-3-opus-20240229".to_owned()),
            LLMType::ClaudeSonnet => Ok("claude-3-5-sonnet-20241022".to_owned()),
            LLMType::ClaudeHaiku => Ok("claude-3-haiku-20240307".to_owned()),
            LLMType::Custom(model) => Ok(model.to_owned()),
            _ => Err(LLMClientError::UnSupportedModel),
        }
    }

    /// We try to get the completion along with the tool which we are planning on using
    fn from_client_string_request(
        completion_request: LLMClientCompletionStringRequest,
        model_str: String,
    ) -> Self {
        let temperature = completion_request.temperature();
        let max_tokens = completion_request.get_max_tokens();
        let messages = vec![AnthropicMessage::new(
            "user".to_owned(),
            completion_request.prompt().to_owned(),
        )];
        AnthropicRequest {
            system: vec![],
            messages,
            temperature,
            tools: vec![],
            stream: true,
            max_tokens,
            model: model_str,
        }
    }

    pub async fn stream_completion_with_tool(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        metadata: HashMap<String, String>,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<(String, Vec<(String, (String, String))>), LLMClientError> {
        let endpoint = self.chat_endpoint();
        let messages = request
            .messages()
            .into_iter()
            .map(|message| message.clone())
            .collect::<Vec<_>>();
        let model_str = self.get_model_string(request.model())?;
        let message_tokens = request
            .messages()
            .iter()
            .map(|message| message.content().len())
            .collect::<Vec<_>>();
        let mut message_tokens_count = 0;
        message_tokens.into_iter().for_each(|tokens| {
            message_tokens_count += tokens;
        });
        let anthropic_request =
            AnthropicRequest::from_client_completion_request(request, model_str.to_owned());

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let response_stream = self
            .client
            .post(endpoint)
            .header(
                "x-api-key".to_owned(),
                self.generate_api_bearer_key(api_key)?,
            )
            .header("anthropic-version".to_owned(), "2023-06-01".to_owned())
            .header("content-type".to_owned(), "application/json".to_owned())
            .header(
                "anthropic-beta".to_owned(),
                "prompt-caching-2024-07-31,max-tokens-3-5-sonnet-2024-07-15,computer-use-2024-10-22".to_owned(),
            )
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| {
                error!("sidecar.anthropic.error: {:?}", &e);
                e
            })?;

        // Check for 401 Unauthorized status
        if response_stream.status() == reqwest::StatusCode::UNAUTHORIZED {
            error!("Unauthorized access to Anthropic API");
            return Err(LLMClientError::UnauthorizedAccess);
        }

        let mut event_source = response_stream.bytes_stream().eventsource();

        let mut buffered_string = "".to_owned();
        let mut tool_use_indication: Vec<(String, (String, String))> = vec![];
        let mut current_tool_use = None;
        let current_tool_use_ref = &mut current_tool_use;
        let mut current_tool_use_id = None;
        let current_tool_use_id_ref = &mut current_tool_use_id;
        let mut running_tool_input = "".to_owned();
        let running_tool_input_ref = &mut running_tool_input;

        while let Some(Ok(event)) = event_source.next().await {
            let event = serde_json::from_str::<AnthropicEvent>(&event.data);
            match event {
                Ok(AnthropicEvent::ContentBlockStart { content_block, .. }) => {
                    match content_block {
                        ContentBlockStart::InputToolUse { name, id } => {
                            *current_tool_use_ref = Some(name.to_owned());
                            *current_tool_use_id_ref = Some(id.to_owned());
                            info!("anthropic::tool_use::{}", &name);
                        }
                        ContentBlockStart::TextDelta { text } => {
                            buffered_string = buffered_string + &text;
                            if let Err(e) = sender.send(LLMClientCompletionResponse::new(
                                buffered_string.to_owned(),
                                Some(text),
                                model_str.to_owned(),
                            )) {
                                error!("Failed to send completion response: {}", e);
                                return Err(LLMClientError::SendError(e));
                            }
                        }
                    }
                }
                Ok(AnthropicEvent::ContentBlockDelta { delta, .. }) => match delta {
                    ContentBlockDeltaType::TextDelta { text } => {
                        buffered_string = buffered_string + &text;
                        let time_now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis();
                        let time_diff = time_now - current_time;
                        debug!(
                            event_name = "anthropic.buffered_string",
                            message_tokens_count = message_tokens_count,
                            generated_tokens_count = &buffered_string.len(),
                            time_taken = time_diff,
                        );
                        if let Err(e) = sender.send(LLMClientCompletionResponse::new(
                            buffered_string.to_owned(),
                            Some(text),
                            model_str.to_owned(),
                        )) {
                            error!("Failed to send completion response: {}", e);
                            return Err(LLMClientError::SendError(e));
                        }
                    }
                    ContentBlockDeltaType::InputJsonDelta { partial_json } => {
                        *running_tool_input_ref = running_tool_input_ref.to_owned() + &partial_json;
                    }
                },
                Ok(AnthropicEvent::ContentBlockStop { _index }) => {
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
                    *current_tool_use_ref = None;
                    *running_tool_input_ref = "".to_owned();
                    *current_tool_use_id_ref = None;
                }
                Ok(AnthropicEvent::MessageStart { message }) => {
                    println!(
                        "anthropic::cache_hit::{:?}",
                        message.usage.cache_read_input_tokens
                    );
                }
                Err(e) => {
                    error!("Error parsing event: {:?}", e);
                }
                _ => {}
            }
        }

        if tool_use_indication.is_empty() {
            info!("anthropic::tool_not_found");
        }

        let request_id = uuid::Uuid::new_v4();
        let parea_log_completion = PareaLogCompletion::new(
            messages
                .into_iter()
                .map(|message| {
                    PareaLogMessage::new(message.role().to_string(), {
                        let content = message.content();
                        let tool_use_value = message
                            .tool_use_value()
                            .into_iter()
                            .map(|tool_use_value| {
                                serde_json::to_string(&tool_use_value).expect("to work")
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let tool_return_value = message
                            .tool_return_value()
                            .into_iter()
                            .map(|llm_return_value| {
                                serde_json::to_string(&llm_return_value).expect("to work")
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!(
                            r#"<content>
{content}
</content>
<tool_use_value>
{tool_use_value}
</tool_use_value>
<tool_return_value>
{tool_return_value}
</tool_return_value>"#
                        )
                    })
                })
                .collect::<Vec<_>>(),
            metadata.clone(),
            {
                format!(
                    "<content>
{}
</content>
<tool_use_indication>
{}
</tool_use_indication>",
                    &buffered_string,
                    tool_use_indication
                        .to_vec()
                        .into_iter()
                        .map(|(_, (tool_use_type, tool_use_value))| {
                            format!(
                                "<tool_use_value>
<tool_type>
{}
</tool_type>
<tool_content>
{}
</tool_content>
</tool_use_value>",
                                tool_use_type, tool_use_value
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            },
            0.2,
            request_id.to_string(),
            request_id.to_string(),
            metadata
                .get("root_trace_id")
                .map(|s| s.to_owned())
                .unwrap_or(request_id.to_string()),
            "ClaudeSonnet".to_owned(),
            "Anthropic".to_owned(),
            metadata
                .get("event_type")
                .map(|s| s.to_owned())
                .unwrap_or("no_event_type".to_owned()),
        );
        let _ = PareaClient::new()
            .log_completion(parea_log_completion)
            .await;

        Ok((buffered_string, tool_use_indication))
    }
}

#[async_trait]
impl LLMClient for AnthropicClient {
    fn client(&self) -> &LLMProvider {
        &LLMProvider::Anthropic
    }

    async fn completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError> {
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        self.stream_completion(api_key, request, sender)
            .await
            .map(|answer| answer.answer_up_until_now().to_owned())
    }

    async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<LLMClientCompletionResponse, LLMClientError> {
        info!("anthropic::stream_completion");
        let endpoint = self.chat_endpoint();
        let model_str = self.get_model_string(request.model())?;
        let message_tokens = request
            .messages()
            .iter()
            .map(|message| message.content().len())
            .collect::<Vec<_>>();
        let mut message_tokens_count = 0;
        message_tokens.into_iter().for_each(|tokens| {
            message_tokens_count += tokens;
        });
        let anthropic_request =
            AnthropicRequest::from_client_completion_request(request, model_str.to_owned());

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let response_stream = self
            .client
            .post(endpoint)
            .header(
                "x-api-key".to_owned(),
                self.generate_api_bearer_key(api_key)?,
            )
            .header("anthropic-version".to_owned(), "2023-06-01".to_owned())
            .header("content-type".to_owned(), "application/json".to_owned())
            // anthropic-beta: prompt-caching-2024-07-31
            // enables prompt caching: https://arc.net/l/quote/qtlllqgf
            .header(
                "anthropic-beta".to_owned(),
                "prompt-caching-2024-07-31,max-tokens-3-5-sonnet-2024-07-15,computer-use-2024-10-22".to_owned(),
            )
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| {
                error!("sidecar.anthropic.error: {:?}", &e);
                e
            })?;

        // Check for 401 Unauthorized status
        if response_stream.status() == reqwest::StatusCode::UNAUTHORIZED {
            error!("Unauthorized access to Anthropic API");
            return Err(LLMClientError::UnauthorizedAccess);
        }

        let mut event_source = response_stream.bytes_stream().eventsource();

        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut input_cached_tokens = 0;

        let mut buffered_string = "".to_owned();
        while let Some(Ok(event)) = event_source.next().await {
            // TODO: debugging this
            let event = serde_json::from_str::<AnthropicEvent>(&event.data);
            match event {
                Ok(AnthropicEvent::ContentBlockStart { content_block, .. }) => {
                    match content_block {
                        ContentBlockStart::InputToolUse { name, id: _id } => {
                            println!("anthropic::tool_use::{}", &name);
                        }
                        ContentBlockStart::TextDelta { text } => {
                            buffered_string = buffered_string + &text;
                            if let Err(e) = sender.send(
                                LLMClientCompletionResponse::new(
                                    buffered_string.to_owned(),
                                    Some(text),
                                    model_str.to_owned(),
                                )
                                .set_usage_statistics(
                                    LLMClientUsageStatistics::new()
                                        .set_input_tokens(input_tokens)
                                        .set_output_tokens(output_tokens),
                                ),
                            ) {
                                error!("Failed to send completion response: {}", e);
                                return Err(LLMClientError::SendError(e));
                            }
                        }
                    }
                }
                Ok(AnthropicEvent::ContentBlockDelta { delta, .. }) => match delta {
                    ContentBlockDeltaType::TextDelta { text } => {
                        buffered_string = buffered_string + &text;
                        let time_now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis();
                        let time_diff = time_now - current_time;
                        debug!(
                            event_name = "anthropic.buffered_string",
                            message_tokens_count = message_tokens_count,
                            generated_tokens_count = &buffered_string.len(),
                            time_taken = time_diff,
                        );
                        if let Err(e) = sender.send(
                            LLMClientCompletionResponse::new(
                                buffered_string.to_owned(),
                                Some(text),
                                model_str.to_owned(),
                            )
                            .set_usage_statistics(
                                LLMClientUsageStatistics::new()
                                    .set_input_tokens(input_tokens)
                                    .set_output_tokens(output_tokens)
                                    .set_cached_input_tokens(input_cached_tokens),
                            ),
                        ) {
                            error!("Failed to send completion response: {}", e);
                            return Err(LLMClientError::SendError(e));
                        }
                    }
                    ContentBlockDeltaType::InputJsonDelta { partial_json } => {
                        debug!("input_json_delta::{}", &partial_json);
                    }
                },
                Ok(AnthropicEvent::MessageStart { message }) => {
                    input_tokens = input_tokens + message.usage.input_tokens.unwrap_or_default();
                    output_tokens = output_tokens + message.usage.output_tokens.unwrap_or_default();
                    input_cached_tokens = input_cached_tokens
                        + message.usage.cache_read_input_tokens.unwrap_or_default();
                }
                Ok(AnthropicEvent::MessageDelta { _delta: _, usage }) => {
                    input_tokens = input_tokens + usage.input_tokens.unwrap_or_default();
                    output_tokens = output_tokens + usage.output_tokens.unwrap_or_default();
                    input_cached_tokens =
                        input_cached_tokens + usage.cache_read_input_tokens.unwrap_or_default();
                }
                Err(e) => {
                    println!("{:?}", e);
                    // break;
                }
                _ => {
                    debug!("Received anthropic event: {:?}", &event);
                }
            }
        }

        Ok(
            LLMClientCompletionResponse::new(buffered_string, None, model_str)
                .set_usage_statistics(
                    LLMClientUsageStatistics::new()
                        .set_input_tokens(input_tokens)
                        .set_output_tokens(output_tokens)
                        .set_cached_input_tokens(input_cached_tokens),
                ),
        )
    }

    async fn stream_prompt_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionStringRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError> {
        let endpoint = self.chat_endpoint();
        let model_str = self.get_model_string(request.model())?;
        let anthropic_request = AnthropicRequest::from_client_string_request(request, model_str.to_owned());

        let response_stream = self
            .client
            .post(endpoint)
            .header(
                "x-api-key".to_owned(),
                self.generate_api_bearer_key(api_key)?,
            )
            .header("anthropic-version".to_owned(), "2023-06-01".to_owned())
            .header("content-type".to_owned(), "application/json".to_owned())
            .header(
                "anthropic-beta".to_owned(),
                "prompt-caching-2024-07-31,max-tokens-3-5-sonnet-2024-07-15,computer-use-2024-10-22".to_owned(),
            )
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| {
                error!("sidecar.anthropic.error: {:?}", &e);
                e
            })?;

        if response_stream.status() == reqwest::StatusCode::UNAUTHORIZED {
            error!("Unauthorized access to Anthropic API");
            return Err(LLMClientError::UnauthorizedAccess);
        }

        let mut event_source = response_stream.bytes_stream().eventsource();
        let mut buffered_string = String::new();
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut input_cached_tokens = 0;

        while let Some(Ok(event)) = event_source.next().await {
            let event = serde_json::from_str::<AnthropicEvent>(&event.data);
            match event {
                Ok(AnthropicEvent::ContentBlockStart { content_block, .. }) => {
                    match content_block {
                        ContentBlockStart::InputToolUse { name, .. } => {
                            info!("anthropic::tool_use::{}", &name);
                        }
                        ContentBlockStart::TextDelta { text } => {
                            buffered_string.push_str(&text);
                            if let Err(e) = sender.send(
                                LLMClientCompletionResponse::new(
                                    buffered_string.clone(),
                                    Some(text),
                                    model_str.to_owned(),
                                )
                                .set_usage_statistics(
                                    LLMClientUsageStatistics::new()
                                        .set_input_tokens(input_tokens)
                                        .set_output_tokens(output_tokens)
                                        .set_cached_input_tokens(input_cached_tokens),
                                ),
                            ) {
                                error!("Failed to send completion response: {}", e);
                                return Err(LLMClientError::SendError(e));
                            }
                        }
                    }
                }
                Ok(AnthropicEvent::ContentBlockDelta { delta, .. }) => match delta {
                    ContentBlockDeltaType::TextDelta { text } => {
                        buffered_string.push_str(&text);
                        if let Err(e) = sender.send(
                            LLMClientCompletionResponse::new(
                                buffered_string.clone(),
                                Some(text),
                                model_str.to_owned(),
                            )
                            .set_usage_statistics(
                                LLMClientUsageStatistics::new()
                                    .set_input_tokens(input_tokens)
                                    .set_output_tokens(output_tokens)
                                    .set_cached_input_tokens(input_cached_tokens),
                            ),
                        ) {
                            error!("Failed to send completion response: {}", e);
                            return Err(LLMClientError::SendError(e));
                        }
                    }
                    ContentBlockDeltaType::InputJsonDelta { partial_json } => {
                        debug!("input_json_delta::{}", &partial_json);
                    }
                },
                Ok(AnthropicEvent::MessageStart { message }) => {
                    input_tokens += message.usage.input_tokens.unwrap_or_default();
                    output_tokens += message.usage.output_tokens.unwrap_or_default();
                    input_cached_tokens += message.usage.cache_read_input_tokens.unwrap_or_default();
                }
                Ok(AnthropicEvent::MessageDelta { usage, .. }) => {
                    input_tokens += usage.input_tokens.unwrap_or_default();
                    output_tokens += usage.output_tokens.unwrap_or_default();
                    input_cached_tokens += usage.cache_read_input_tokens.unwrap_or_default();
                }
                Err(e) => {
                    error!("Error parsing event: {:?}", e);
                }
                _ => {}
            }
        }

        Ok(buffered_string)
    }
}