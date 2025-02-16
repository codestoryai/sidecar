//! The llm client broker takes care of getting the right tokenizer formatter etc
//! without us having to worry about the specifics, just pass in the message and the
//! provider we take care of the rest

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use either::Either;
use futures::{stream, FutureExt, StreamExt};
use sqlx::SqlitePool;

use crate::{
    clients::{
        anthropic::AnthropicClient,
        codestory::CodeStoryClient,
        fireworks::FireworksAIClient,
        gemini_pro::GeminiProClient,
        google_ai::GoogleAIStdioClient,
        groq::GroqClient,
        lmstudio::LMStudioClient,
        ollama::OllamaClient,
        open_router::OpenRouterClient,
        openai::OpenAIClient,
        openai_compatible::OpenAICompatibleClient,
        togetherai::TogetherAIClient,
        types::{
            LLMClient, LLMClientCompletionRequest, LLMClientCompletionResponse,
            LLMClientCompletionStringRequest, LLMClientError, LLMType,
        },
    },
    provider::{CodeStoryLLMTypes, LLMProvider, LLMProviderAPIKeys},
    reporting::posthog::{posthog_client, PosthogClient},
};

use logging::parea::{PareaClient, PareaLogCompletion, PareaLogMessage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    pub text: Option<String>,
    pub blob: Option<String>,
}

/// A trait representing a tool that can be called by the client
pub trait MCPTool {
    /// The name of the tool
    fn name(&self) -> &str;
    
    /// A human-readable description of the tool
    fn description(&self) -> Option<&str>;
    
    /// The input schema for the tool parameters
    fn input_schema(&self) -> ToolInputSchema;

    /// Convert the tool's input schema to a JSON schema object
    fn to_json_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": self.input_schema().properties.unwrap_or_default(),
            "required": self.input_schema().required.unwrap_or_default(),
        })
    }

    /// Validate input JSON against the tool's schema
    fn validate_input(&self, input: &serde_json::Map<String, serde_json::Value>) -> bool {
        let schema = self.to_json_schema();
        // Check required fields
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if !input.contains_key(field_name) {
                        return false;
                    }
                }
            }
        }
        // Check property types - could be expanded for more thorough validation
        if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
            for (key, schema_value) in properties {
                if let Some(input_value) = input.get(key) {
                    if let Some(expected_type) = schema_value.get("type").and_then(|t| t.as_str()) {
                        match expected_type {
                            "string" => if !input_value.is_string() { return false; },
                            "number" => if !input_value.is_number() { return false; },
                            "boolean" => if !input_value.is_boolean() { return false; },
                            "object" => if !input_value.is_object() { return false; },
                            "array" => if !input_value.is_array() { return false; },
                            _ => {}
                        }
                    }
                }
            }
        }
        true
    }

    /// Convert XML input to a JSON object that can be validated
    fn parse_xml_input(&self, xml_input: &str) -> Result<serde_json::Map<String, serde_json::Value>, Box<dyn std::error::Error>> {
        let root = xmltree::Element::parse(xml_input.as_bytes())?;
        let mut map = serde_json::Map::new();
        
        for child in root.children {
            if let xmltree::XMLNode::Element(child_elem) = child {
                let name = child_elem.name.clone();
                let text = child_elem.get_text().map(|t| t.to_string());
                if let Some(text) = text {
                    map.insert(name, serde_json::Value::String(text));
                }
            }
        }
        
        Ok(map)
    }

    /// Parse and validate XML input against the tool's schema
    fn validate_xml_input(&self, xml_input: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let json_input = self.parse_xml_input(xml_input)?;
        Ok(self.validate_input(&json_input))
    }

    /// List available resources
    fn list_resources(&self) -> Vec<Resource> {
        Vec::new()
    }

    /// List available resource templates
    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        Vec::new()
    }

    /// Read a resource's contents
    fn read_resource(&self, _uri: &str) -> Result<Vec<ResourceContent>, Box<dyn std::error::Error>> {
        Ok(Vec::new())
    }

    /// Subscribe to resource updates
    fn subscribe_resource(&self, _uri: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Unsubscribe from resource updates
    fn unsubscribe_resource(&self, _uri: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

/// Input schema for tool parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    /// Always "object" for MCP tools
    pub r#type: String,
    /// Properties defining the expected parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, serde_json::Value>>,
    /// List of required parameter names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl Default for ToolInputSchema {
    fn default() -> Self {
        Self {
            r#type: "object".to_string(),
            properties: None,
            required: None,
        }
    }
}

pub type SqlDb = Arc<SqlitePool>;

pub struct LLMBroker {
    pub providers: HashMap<LLMProvider, Box<dyn LLMClient + Send + Sync>>,
    posthog_client: Arc<PosthogClient>,
    parea_client: Arc<PareaClient>,
}

pub type LLMBrokerResponse = Result<LLMClientCompletionResponse, LLMClientError>;

impl LLMBroker {
    pub async fn new() -> Result<Self, LLMClientError> {
        // later we need to configure the user id over here to be the one from
        // the client machine we are running it on
        let posthog_client = Arc::new(posthog_client("agentic"));
        let parea_client = Arc::new(PareaClient::new());
        let broker = Self {
            providers: HashMap::new(),
            posthog_client,
            parea_client,
        };
        Ok(broker
            .add_provider(LLMProvider::OpenAI, Box::new(OpenAIClient::new()))
            .add_provider(LLMProvider::Ollama, Box::new(OllamaClient::new()))
            .add_provider(LLMProvider::TogetherAI, Box::new(TogetherAIClient::new()))
            .add_provider(LLMProvider::LMStudio, Box::new(LMStudioClient::new()))
            .add_provider(
                LLMProvider::OpenAICompatible,
                Box::new(OpenAICompatibleClient::new()),
            )
            .add_provider(
                LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None }),
                Box::new(CodeStoryClient::new(
                    "https://codestory-provider-dot-anton-390822.ue.r.appspot.com",
                )),
            )
            .add_provider(LLMProvider::FireworksAI, Box::new(FireworksAIClient::new()))
            .add_provider(LLMProvider::Anthropic, Box::new(AnthropicClient::new()))
            .add_provider(LLMProvider::GeminiPro, Box::new(GeminiProClient::new()))
            .add_provider(LLMProvider::OpenRouter, Box::new(OpenRouterClient::new()))
            .add_provider(
                LLMProvider::GoogleAIStudio,
                Box::new(GoogleAIStdioClient::new()),
            )
            .add_provider(LLMProvider::Groq, Box::new(GroqClient::new())))
    }

    pub fn add_provider(
        mut self,
        provider: LLMProvider,
        client: Box<dyn LLMClient + Send + Sync>,
    ) -> Self {
        self.providers.insert(provider, client);
        self
    }

    pub async fn stream_answer(
        &self,
        api_key: LLMProviderAPIKeys,
        provider: LLMProvider,
        request: Either<LLMClientCompletionRequest, LLMClientCompletionStringRequest>,
        metadata: HashMap<String, String>,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> LLMBrokerResponse {
        match request {
            Either::Left(request) => {
                self.stream_completion(api_key, request, provider, metadata, sender)
                    .await
            }
            Either::Right(request) => {
                self.stream_string_completion(api_key, request, metadata, sender)
                    .await
            }
        }
    }

    fn get_provider(&self, api_key: &LLMProviderAPIKeys) -> LLMProvider {
        match api_key {
            LLMProviderAPIKeys::Ollama(_) => LLMProvider::Ollama,
            LLMProviderAPIKeys::OpenAI(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::OpenAIAzureConfig(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::TogetherAI(_) => LLMProvider::TogetherAI,
            LLMProviderAPIKeys::LMStudio(_) => LLMProvider::LMStudio,
            LLMProviderAPIKeys::CodeStory(_) => {
                LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None })
            }
            LLMProviderAPIKeys::OpenAICompatible(_) => LLMProvider::OpenAICompatible,
            LLMProviderAPIKeys::Anthropic(_) => LLMProvider::Anthropic,
            LLMProviderAPIKeys::FireworksAI(_) => LLMProvider::FireworksAI,
            LLMProviderAPIKeys::GeminiPro(_) => LLMProvider::GeminiPro,
            LLMProviderAPIKeys::GoogleAIStudio(_) => LLMProvider::GoogleAIStudio,
            LLMProviderAPIKeys::OpenRouter(_) => LLMProvider::OpenRouter,
            LLMProviderAPIKeys::GroqProvider(_) => LLMProvider::Groq,
        }
    }

    pub async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        provider: LLMProvider,
        metadata: HashMap<String, String>,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> LLMBrokerResponse {
        let request_id = uuid::Uuid::new_v4();
        let api_key = api_key
            .key(&provider)
            .ok_or(LLMClientError::UnSupportedModel)?;
        let provider_type = match &api_key {
            LLMProviderAPIKeys::Ollama(_) => LLMProvider::Ollama,
            LLMProviderAPIKeys::OpenAI(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::OpenAIAzureConfig(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::TogetherAI(_) => LLMProvider::TogetherAI,
            LLMProviderAPIKeys::LMStudio(_) => LLMProvider::LMStudio,
            LLMProviderAPIKeys::CodeStory(_) => {
                LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None })
            }
            LLMProviderAPIKeys::OpenAICompatible(_) => LLMProvider::OpenAICompatible,
            LLMProviderAPIKeys::Anthropic(_) => LLMProvider::Anthropic,
            LLMProviderAPIKeys::FireworksAI(_) => LLMProvider::FireworksAI,
            LLMProviderAPIKeys::GeminiPro(_) => LLMProvider::GeminiPro,
            LLMProviderAPIKeys::GoogleAIStudio(_) => LLMProvider::GoogleAIStudio,
            LLMProviderAPIKeys::OpenRouter(_) => LLMProvider::OpenRouter,
            LLMProviderAPIKeys::GroqProvider(_) => LLMProvider::Groq,
        };
        let provider = self.providers.get(&provider_type);
        if let Some(provider) = provider {
            let result = provider
                .stream_completion(api_key, request.clone(), sender)
                .await;
            if let Ok(result) = result.as_ref() {
                let parea_log_completion = PareaLogCompletion::new(
                    request
                        .messages()
                        .into_iter()
                        .map(|message| {
                            PareaLogMessage::new(
                                message.role().to_string(),
                                message.content().to_owned(),
                            )
                        })
                        .collect::<Vec<_>>(),
                    metadata.clone(),
                    result.answer_up_until_now().to_owned(),
                    request.temperature(),
                    request_id.to_string(),
                    request_id.to_string(),
                    metadata
                        .get("root_trace_id")
                        .map(|s| s.to_owned())
                        .unwrap_or(request_id.to_string()),
                    request.model().to_string(),
                    provider_type.to_string(),
                    metadata
                        .get("event_type")
                        .map(|s| s.to_owned())
                        .unwrap_or("no_event_type".to_owned()),
                );
                let _ = self.parea_client.log_completion(parea_log_completion).await;
                // we write the inputs to the DB so we can keep track of the inputs
                // and the result provided by the LLM
                // Log to posthog as well
                let _ = self
                    .posthog_client
                    .capture_reqeust_and_response(&request, result.answer_up_until_now(), metadata)
                    .await;
            }
            result
        } else {
            Err(LLMClientError::UnSupportedModel)
        }
    }

    // TODO(skcd): Debug this part of the code later on, cause we have
    // some bugs around here about the new line we are sending over
    pub async fn stream_string_completion_owned(
        value: Arc<Self>,
        api_key: LLMProviderAPIKeys,
        request: Either<LLMClientCompletionRequest, LLMClientCompletionStringRequest>,
        metadata: HashMap<String, String>,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
        skip_start_line: Option<String>,
        // all of this needs to be a editing option for the stream instead
        is_trigger_line_whitespace: bool,
        trigger_line_indentation: String,
        model: LLMType,
    ) -> LLMBrokerResponse {
        let (sender_channel, receiver) = tokio::sync::mpsc::unbounded_channel();
        let receiver_stream =
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver).map(either::Right);
        let provider = value.get_provider(&api_key);
        let result = value
            .stream_answer(api_key, provider, request, metadata, sender_channel)
            .into_stream()
            .map(either::Left);
        let mut final_result = None;
        struct RunningAnswer {
            answer_up_until_now: String,
            running_line: String,
            first_line_check: bool,
            first_streamable_line_check: bool,
            first_line_indent: Option<String>,
        }
        let running_line = Arc::new(Mutex::new(RunningAnswer {
            answer_up_until_now: "".to_owned(),
            running_line: "".to_owned(),
            first_line_check: false,
            first_streamable_line_check: false,
            first_line_indent: None,
        }));
        // claude is throwing a wrench into this code
        // observations I have seen are:
        // - for output which starts with only whitespace we are going too far
        // and generating code which also has whitespace at the start but our cursor
        //  is at the correct location so we have to trim the whitespace
        // - for output which does not start with whitespace we are fillin in words
        // to the llms mouth so the output we get is mostly correct and we can just insert it
        let should_apply_special_edits = model.is_anthropic();
        stream::select(receiver_stream, result)
            .map(|element| (element, running_line.clone()))
            .for_each(|(element, running_line)| {
                match element {
                    either::Right(item) => {
                        if should_apply_special_edits {
                            // if the answer ends with \n</code_inserted> then its generated
                            // by claude and we should stop streaming back
                            if item.answer_up_until_now().ends_with("\n</code_inserted>") {
                                return futures::future::ready(());
                            }
                        }
                        let delta = item.delta().map(|delta| delta.to_owned());
                        if let Ok(mut current_running_line) = running_line.lock() {
                            if let Some(delta) = delta {
                                current_running_line.running_line.push_str(&delta);
                            }
                            while let Some(new_line_index) =
                                current_running_line.running_line.find('\n')
                            {
                                let mut line =
                                    current_running_line.running_line[..new_line_index].to_owned();

                                // we need to check for the first line here if we are starting with
                                // whitespace and are using anthropic
                                if should_apply_special_edits
                                    && is_trigger_line_whitespace
                                    && !current_running_line.first_line_check
                                    && Some(line.to_owned()) == skip_start_line
                                {
                                    current_running_line.first_line_check = true;
                                } else {
                                    // we need to indent and fix the output here
                                    // vodoo magic here to fix the indent for the lines
                                    // coming from anthropic
                                    if should_apply_special_edits {
                                        // first we check for the first line which we get
                                        if !current_running_line.first_streamable_line_check {
                                            if is_trigger_line_whitespace {
                                                let whitespace_difference = get_indent_diff(
                                                    &line,
                                                    &trigger_line_indentation,
                                                );
                                                dbg!(
                                                    "get_indent_diff",
                                                    &whitespace_difference,
                                                    &line,
                                                    &trigger_line_indentation
                                                );
                                                current_running_line.first_line_indent =
                                                    Some(whitespace_difference.to_owned());
                                                // if we are streaming based on completions we are getting
                                                // from whitespace trigger, we need to see what indent
                                                // the llm generates at and then fix it up manually after that
                                                // it will always be an extra ident if anything
                                                line = line.trim_start().to_owned();
                                            }
                                        } else {
                                            if is_trigger_line_whitespace {
                                                if let Some(whitespace_extra) =
                                                    &current_running_line.first_line_indent
                                                {
                                                    line = whitespace_extra.to_owned() + &line;
                                                }
                                            }
                                        }
                                        // we need to check and fix the line here
                                    }
                                    let current_answer = current_running_line
                                        .answer_up_until_now
                                        .clone()
                                        .lines()
                                        .into_iter()
                                        .map(|line| line.to_owned())
                                        .chain(vec![line.to_owned()])
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    if should_apply_special_edits {
                                        // do not send if the delta is the marker for
                                        // the closing tag
                                        if line.trim() != "</code_inserted>" {
                                            let _ = sender.send(LLMClientCompletionResponse::new(
                                                current_answer + "\n",
                                                Some(line.to_owned() + "\n"),
                                                "parsing_model".to_owned(),
                                            ));
                                        }
                                    } else {
                                        let _ = sender.send(LLMClientCompletionResponse::new(
                                            current_answer + "\n",
                                            Some(line.to_owned() + "\n"),
                                            "parsing_model".to_owned(),
                                        ));
                                    }
                                    // add the new line and the \n
                                    current_running_line.answer_up_until_now.push_str(&line);
                                    current_running_line.answer_up_until_now.push_str("\n");
                                    // we have our first streamable line, so set it to
                                    // true
                                    current_running_line.first_streamable_line_check = true;
                                }

                                // set the first line as done
                                current_running_line.first_line_check = true;

                                // drain the running line
                                current_running_line.running_line.drain(..=new_line_index);
                            }
                            // current_running_line.answer_up_until_now = answer_until_now;
                        }
                    }
                    either::Left(item) => {
                        final_result = Some(item);
                    }
                };
                futures::future::ready(())
            })
            .await;

        if let Ok(current_running_line) = running_line.lock() {
            let _ = sender.send(LLMClientCompletionResponse::new(
                current_running_line.answer_up_until_now.to_owned(),
                Some(current_running_line.running_line.to_owned()),
                "parsing_model".to_owned(),
            ));
        }
        final_result.ok_or(LLMClientError::FailedToGetResponse)?
    }

    pub async fn stream_string_completion<'a>(
        &'a self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionStringRequest,
        _metadata: HashMap<String, String>,
        sender: tokio::sync::mpsc::UnboundedSender<LLMClientCompletionResponse>,
    ) -> LLMBrokerResponse {
        let provider_type = match &api_key {
            LLMProviderAPIKeys::Ollama(_) => LLMProvider::Ollama,
            LLMProviderAPIKeys::OpenAI(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::OpenAIAzureConfig(_) => LLMProvider::OpenAI,
            LLMProviderAPIKeys::TogetherAI(_) => LLMProvider::TogetherAI,
            LLMProviderAPIKeys::LMStudio(_) => LLMProvider::LMStudio,
            LLMProviderAPIKeys::CodeStory(_) => {
                LLMProvider::CodeStory(CodeStoryLLMTypes { llm_type: None })
            }
            LLMProviderAPIKeys::OpenAICompatible(_) => LLMProvider::OpenAICompatible,
            LLMProviderAPIKeys::Anthropic(_) => LLMProvider::Anthropic,
            LLMProviderAPIKeys::FireworksAI(_) => LLMProvider::FireworksAI,
            LLMProviderAPIKeys::GeminiPro(_) => LLMProvider::GeminiPro,
            LLMProviderAPIKeys::GoogleAIStudio(_) => LLMProvider::GoogleAIStudio,
            LLMProviderAPIKeys::OpenRouter(_) => LLMProvider::OpenRouter,
            LLMProviderAPIKeys::GroqProvider(_) => LLMProvider::Groq,
        };
        let provider = self.providers.get(&provider_type);
        if let Some(provider) = provider {
            let result = provider
                .stream_prompt_completion(api_key, request.clone(), sender)
                .await;
            result.map(|result| {
                LLMClientCompletionResponse::new(result, None, "not_present".to_owned())
            })
        } else {
            Err(LLMClientError::UnSupportedModel)
        }
    }
}

fn get_indent_diff(s: &str, whitespace: &str) -> String {
    dbg!("Calculating indent difference");
    let mut indent_count = 0;
    for c in s.chars() {
        if c.is_whitespace() || c == '\t' {
            indent_count += 1;
        } else {
            break;
        }
    }

    let whitespace_count = whitespace
        .chars()
        .filter(|&c| c.is_whitespace() || c == '\t')
        .count();
    let whitepsace_indent_difference = if indent_count >= whitespace_count {
        indent_count - whitespace_count
    } else {
        whitespace_count - indent_count
    };

    if whitespace.chars().next() == Some('\t') {
        let mut whitespace_string = "".to_owned();
        for _ in 0..whitepsace_indent_difference {
            whitespace_string = whitespace_string + "\t";
        }
        whitespace_string
    } else {
        let mut whitespace_string = "".to_owned();
        for _ in 0..whitepsace_indent_difference {
            whitespace_string = whitespace_string + " ";
        }
        whitespace_string
    }
}