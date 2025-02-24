use async_trait::async_trait;
use eventsource_stream::EventStreamError;
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::fmt;
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;

use crate::provider::{LLMProvider, LLMProviderAPIKeys};

/// Represents different types of Language Learning Models (LLMs)
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum LLMType {
    /// Mixtral model
    Mixtral,
    /// Mistral Instruct model
    MistralInstruct,
    /// GPT-4 model
    Gpt4,
    /// GPT-3.5 with 16k context window
    GPT3_5_16k,
    /// GPT-4 with 32k context window
    Gpt4_32k,
    /// GPT-4 Optimized model
    Gpt4O,
    /// GPT-4 Optimized Mini model
    Gpt4OMini,
    /// GPT-4 Turbo model
    Gpt4Turbo,
    /// o1 model
    O1Preview,
    /// o1 mini model
    O1Mini,
    /// o1 model
    O1,
    /// o3 mini model + high reasoning
    O3MiniHigh,
    /// DeepSeek Coder 1.3B Instruct model
    DeepSeekCoder1_3BInstruct,
    /// DeepSeek Coder 33B Instruct model
    DeepSeekCoder33BInstruct,
    /// DeepSeek Coder 6B Instruct model
    DeepSeekCoder6BInstruct,
    /// DeepSeek Coder V2 model
    DeepSeekCoderV2,
    /// DeepSeek Coder V3 model
    DeepSeekCoderV3,
    /// DeepSeek r1
    DeepSeekR1,
    /// CodeLLama 70B Instruct model
    CodeLLama70BInstruct,
    /// CodeLlama 13B Instruct model
    CodeLlama13BInstruct,
    /// CodeLlama 7B Instruct model
    CodeLlama7BInstruct,
    /// Llama 3 8B Instruct model
    Llama3_8bInstruct,
    /// Llama 3.1 8B Instruct model
    Llama3_1_8bInstruct,
    /// Llama 3.1 70B Instruct model
    Llama3_1_70bInstruct,
    /// Claude Opus model
    ClaudeOpus,
    /// Claude Sonnet model
    ClaudeSonnet,
    /// Claude Haiku model
    ClaudeHaiku,
    /// PPLX Sonnet Small model
    PPLXSonnetSmall,
    /// Cohere Rerank V3 model
    CohereRerankV3,
    /// Gemini Pro model
    GeminiPro,
    /// Gemini Pro Flash model
    GeminiProFlash,
    /// Gemini 2.0 Flash model
    Gemini2_0Flash,
    /// Gemini 2.0 Flash experimental
    Gemini2_0FlashExperimental,
    /// Gemini 2.0 Flash thinking experimental
    Gemini2_0FlashThinkingExperimental,
    /// Gemini 2.0 Pro experimental model
    Gemini2_0Pro,
    /// Custom model type with a specified name
    Custom(String),
}

impl Serialize for LLMType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            LLMType::Custom(s) => serializer.serialize_str(s),
            _ => serializer.serialize_str(&format!("{:?}", self)),
        }
    }
}

impl<'de> Deserialize<'de> for LLMType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LLMTypeVisitor;

        impl<'de> Visitor<'de> for LLMTypeVisitor {
            type Value = LLMType;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string representing an LLMType")
            }

            fn visit_str<E>(self, value: &str) -> Result<LLMType, E>
            where
                E: de::Error,
            {
                match value {
                    "Mixtral" => Ok(LLMType::Mixtral),
                    "MistralInstruct" => Ok(LLMType::MistralInstruct),
                    "Gpt4" => Ok(LLMType::Gpt4),
                    "Gpt4OMini" => Ok(LLMType::Gpt4OMini),
                    "GPT3_5_16k" => Ok(LLMType::GPT3_5_16k),
                    "Gpt4_32k" => Ok(LLMType::Gpt4_32k),
                    "Gpt4Turbo" => Ok(LLMType::Gpt4Turbo),
                    "DeepSeekCoder1.3BInstruct" => Ok(LLMType::DeepSeekCoder1_3BInstruct),
                    "DeepSeekCoder6BInstruct" => Ok(LLMType::DeepSeekCoder6BInstruct),
                    "deepseek/deepseek-chat" => Ok(LLMType::DeepSeekCoderV3),
                    "CodeLLama70BInstruct" => Ok(LLMType::CodeLLama70BInstruct),
                    "CodeLlama13BInstruct" => Ok(LLMType::CodeLlama13BInstruct),
                    "CodeLlama7BInstruct" => Ok(LLMType::CodeLlama7BInstruct),
                    "DeepSeekCoder33BInstruct" => Ok(LLMType::DeepSeekCoder33BInstruct),
                    "ClaudeOpus" => Ok(LLMType::ClaudeOpus),
                    "ClaudeSonnet" => Ok(LLMType::ClaudeSonnet),
                    "ClaudeSonnet37" => Ok(LLMType::ClaudeSonnet37),
                    "ClaudeHaiku" => Ok(LLMType::ClaudeHaiku),
                    "PPLXSonnetSmall" => Ok(LLMType::PPLXSonnetSmall),
                    "CohereRerankV3" => Ok(LLMType::CohereRerankV3),
                    "GeminiPro1.5" => Ok(LLMType::GeminiPro),
                    "gemini-1.5-pro" => Ok(LLMType::GeminiPro),
                    "gemini-2.0-flash-exp" => Ok(LLMType::Gemini2_0FlashExperimental),
                    "gemini-2.0-flash-thinking-exp-1219" => {
                        Ok(LLMType::Gemini2_0FlashThinkingExperimental)
                    }
                    "gemini-2.0-pro-exp-02-05" => Ok(LLMType::Gemini2_0Pro),
                    "deepseek/deepseek-r1" => Ok(LLMType::DeepSeekR1),
                    "Llama3_8bInstruct" => Ok(LLMType::Llama3_8bInstruct),
                    "Llama3_1_8bInstruct" => Ok(LLMType::Llama3_1_8bInstruct),
                    "Llama3_1_70bInstruct" => Ok(LLMType::Llama3_1_70bInstruct),
                    "Gpt4O" => Ok(LLMType::Gpt4O),
                    "gpt-4o" => Ok(LLMType::Gpt4O),
                    "GeminiProFlash" => Ok(LLMType::GeminiProFlash),
                    "DeepSeekCoderV2" => Ok(LLMType::DeepSeekCoderV2),
                    "o1-preview" => Ok(LLMType::O1Preview),
                    "o1-mini" => Ok(LLMType::O1Mini),
                    "o1" => Ok(LLMType::O1),
                    "o3-mini" => Ok(LLMType::O3MiniHigh),
                    _ => Ok(LLMType::Custom(value.to_string())),
                }
            }
        }

        deserializer.deserialize_string(LLMTypeVisitor)
    }
}

impl LLMType {
    pub fn is_cache_control_explicit(&self) -> bool {
        matches!(
            self,
            LLMType::ClaudeSonnet | LLMType::ClaudeHaiku | LLMType::ClaudeOpus
        )
    }

    pub fn is_openai(&self) -> bool {
        matches!(
            self,
            LLMType::Gpt4
                | LLMType::GPT3_5_16k
                | LLMType::Gpt4_32k
                | LLMType::Gpt4Turbo
                | LLMType::Gpt4O
                | LLMType::Gpt4OMini
        )
    }

    pub fn is_o1_preview(&self) -> bool {
        matches!(self, LLMType::O1Preview | LLMType::O1Mini)
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, LLMType::Custom(_))
    }

    pub fn is_anthropic(&self) -> bool {
        matches!(
            self,
            LLMType::ClaudeOpus | LLMType::ClaudeSonnet | LLMType::ClaudeHaiku | LLMType::ClaudeSonnet37
        )
    }

    pub fn is_openai_gpt4o(&self) -> bool {
        matches!(self, LLMType::Gpt4O)
    }

    pub fn is_gemini_model(&self) -> bool {
        matches!(
            self,
            LLMType::GeminiPro
                | LLMType::GeminiProFlash
                | LLMType::Gemini2_0Flash
                | LLMType::Gemini2_0FlashExperimental
                | LLMType::Gemini2_0FlashThinkingExperimental
                | LLMType::Gemini2_0Pro
        )
    }

    pub fn is_gemini_pro(&self) -> bool {
        self == &LLMType::GeminiPro
    }

    pub fn is_togetherai_model(&self) -> bool {
        matches!(
            self,
            LLMType::CodeLlama13BInstruct
                | LLMType::CodeLlama7BInstruct
                | LLMType::DeepSeekCoder33BInstruct
        )
    }
}

impl fmt::Display for LLMType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LLMType::Mixtral => write!(f, "Mixtral"),
            LLMType::MistralInstruct => write!(f, "MistralInstruct"),
            LLMType::Gpt4 => write!(f, "Gpt4"),
            LLMType::GPT3_5_16k => write!(f, "GPT3_5_16k"),
            LLMType::Gpt4_32k => write!(f, "Gpt4_32k"),
            LLMType::Gpt4Turbo => write!(f, "Gpt4Turbo"),
            LLMType::DeepSeekCoder1_3BInstruct => write!(f, "DeepSeekCoder1.3BInstruct"),
            LLMType::DeepSeekCoder6BInstruct => write!(f, "DeepSeekCoder6BInstruct"),
            LLMType::CodeLLama70BInstruct => write!(f, "CodeLLama70BInstruct"),
            LLMType::CodeLlama13BInstruct => write!(f, "CodeLlama13BInstruct"),
            LLMType::CodeLlama7BInstruct => write!(f, "CodeLlama7BInstruct"),
            LLMType::DeepSeekCoder33BInstruct => write!(f, "DeepSeekCoder33BInstruct"),
            LLMType::DeepSeekCoderV3 => write!(f, "deepseek/deepseek-chat"),
            LLMType::ClaudeOpus => write!(f, "ClaudeOpus"),
            LLMType::ClaudeSonnet => write!(f, "ClaudeSonnet"),
            LLMType::ClaudeSonnet37 => write!(f, "ClaudeSonnet37"),
            LLMType::ClaudeHaiku => write!(f, "ClaudeHaiku"),
            LLMType::PPLXSonnetSmall => write!(f, "PPLXSonnetSmall"),
            LLMType::CohereRerankV3 => write!(f, "CohereRerankV3"),
            LLMType::Llama3_8bInstruct => write!(f, "Llama3_8bInstruct"),
            LLMType::GeminiPro => write!(f, "GeminiPro1.5"),
            LLMType::GeminiProFlash => write!(f, "GeminiProFlash"),
            LLMType::Gemini2_0Flash => write!(f, "gemini-2.0-flash"),
            LLMType::Gemini2_0FlashExperimental => write!(f, "gemini-2.0-flash-exp"),
            LLMType::Gemini2_0FlashThinkingExperimental => {
                write!(f, "gemini-2.0-flash-thinking-exp-1219")
            }
            LLMType::Gemini2_0Pro => write!(f, "gemini-2.0-pro-exp-02-05"),
            LLMType::DeepSeekR1 => write!(f, "deepseek/deepseek-r1"),
            LLMType::Gpt4O => write!(f, "Gpt4O"),
            LLMType::DeepSeekCoderV2 => write!(f, "DeepSeekCoderV2"),
            LLMType::Llama3_1_8bInstruct => write!(f, "Llama3_1_8bInstruct"),
            LLMType::Llama3_1_70bInstruct => write!(f, "Llama3_1_70bInstruct"),
            LLMType::Gpt4OMini => write!(f, "Gpt4OMini"),
            LLMType::O1Preview => write!(f, "o1-preview"),
            LLMType::O1Mini => write!(f, "o1-mini"),
            LLMType::O1 => write!(f, "o1"),
            LLMType::O3MiniHigh => write!(f, "o3-mini"),
            LLMType::Custom(s) => write!(f, "Custom({})", s),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub enum LLMClientRole {
    System,
    User,
    Assistant,
    // function calling is weird, its only supported by openai right now
    // and not other LLMs, so we are going to make this work with the formatters
    // and still keep it as it is
    Function,
}

impl LLMClientRole {
    pub fn is_system(&self) -> bool {
        matches!(self, LLMClientRole::System)
    }

    pub fn is_user(&self) -> bool {
        matches!(self, LLMClientRole::User)
    }

    pub fn is_assistant(&self) -> bool {
        matches!(self, LLMClientRole::Assistant)
    }

    pub fn is_function(&self) -> bool {
        matches!(self, LLMClientRole::Function)
    }

    pub fn to_string(&self) -> String {
        match self {
            LLMClientRole::System => "system".to_owned(),
            LLMClientRole::User => "user".to_owned(),
            LLMClientRole::Assistant => "assistant".to_owned(),
            LLMClientRole::Function => "function".to_owned(),
        }
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientMessageFunctionCall {
    name: String,
    // arguments are generally given as a JSON string, so we keep it as a string
    // here, validate in the upper handlers for this
    arguments: String,
}

impl LLMClientMessageFunctionCall {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientMessageFunctionReturn {
    name: String,
    content: String,
}

impl LLMClientMessageFunctionReturn {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientMessageTool {
    name: String,
    description: String,
    r#type: Option<String>,
    input_schema: Option<serde_json::Value>,
    required: Vec<String>,
}

impl LLMClientMessageTool {
    pub fn new(
        name: String,
        description: String,
        input_schema: Option<serde_json::Value>,
        required: Vec<String>,
    ) -> Self {
        Self {
            name,
            description,
            input_schema,
            required,
            r#type: None,
        }
    }

    pub fn with_type(name: String, r#type: String) -> Self {
        Self {
            name,
            r#type: Some(r#type),
            description: "".to_owned(),
            input_schema: None,
            required: vec![],
        }
    }

    pub fn has_type(&self) -> bool {
        self.r#type.is_some()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn r#type(&self) -> Option<String> {
        self.r#type.clone()
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientToolReturn {
    tool_use_id: String,
    tool_name: String,
    content: String,
}

impl LLMClientToolReturn {
    pub fn new(tool_use_id: String, tool_name: String, content: String) -> Self {
        Self {
            tool_use_id,
            tool_name,
            content,
        }
    }

    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub fn tool_use_id(&self) -> &str {
        &self.tool_use_id
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientToolUse {
    name: String,
    input: serde_json::Value,
    id: String,
}

impl LLMClientToolUse {
    pub fn new(name: String, id: String, input: serde_json::Value) -> Self {
        Self { name, id, input }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn input(&self) -> &serde_json::Value {
        &self.input
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientMessageImage {
    r#type: String,
    media: String,
    data: String,
}

impl LLMClientMessageImage {
    pub fn new(r#type: String, media: String, data: String) -> Self {
        Self {
            r#type,
            media,
            data,
        }
    }

    pub fn r#type(&self) -> &str {
        &self.r#type
    }

    pub fn media(&self) -> &str {
        &self.media
    }

    pub fn data(&self) -> &str {
        &self.data
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct LLMClientMessage {
    role: LLMClientRole,
    message: String,
    images: Vec<LLMClientMessageImage>,
    tools: Vec<serde_json::Value>,
    function_call: Option<LLMClientMessageFunctionCall>,
    function_return: Option<LLMClientMessageFunctionReturn>,
    /// this is going to bite us later on, but until we format the
    /// tool use properly we can figure this out later on
    tool_use: Vec<LLMClientToolUse>,
    tool_return: Vec<LLMClientToolReturn>,
    // if this message marks a caching point in the overall message
    cache_point: bool,
}

impl LLMClientMessage {
    pub fn new(role: LLMClientRole, message: String, images: Vec<LLMClientMessageImage>) -> Self {
        Self {
            role,
            message,
            images,
            tools: vec![],
            tool_use: vec![],
            tool_return: vec![],
            function_call: None,
            function_return: None,
            cache_point: false,
        }
    }

    pub fn concat_message(&mut self, message: &str) {
        self.message = self.message.to_owned() + "\n" + message;
    }

    pub fn concat(self, other: Self) -> Self {
        // We are going to concatenate the 2 llm client messages togehter, at this moment
        // we are just gonig to join the message with a \n
        let mut final_images = self.images.to_vec();
        final_images.extend(other.images);
        let mut final_tools = self.tools.to_vec();
        final_tools.extend(other.tools);
        Self {
            role: self.role,
            message: self.message + "\n" + &other.message,
            images: final_images,
            tools: final_tools,
            function_call: match self.function_call {
                Some(function_call) => Some(function_call),
                None => other.function_call,
            },
            tool_use: vec![],
            tool_return: vec![],
            function_return: match other.function_return {
                Some(function_return) => Some(function_return),
                None => self.function_return,
            },
            cache_point: self.cache_point | other.cache_point,
        }
    }

    pub fn function_call(name: String, arguments: String) -> Self {
        Self {
            role: LLMClientRole::Assistant,
            message: "".to_owned(),
            images: vec![],
            tools: vec![],
            tool_return: vec![],
            tool_use: vec![],
            function_call: Some(LLMClientMessageFunctionCall { name, arguments }),
            function_return: None,
            cache_point: false,
        }
    }

    pub fn function_return(name: String, content: String) -> Self {
        Self {
            role: LLMClientRole::Function,
            message: "".to_owned(),
            images: vec![],
            tools: vec![],
            tool_return: vec![],
            tool_use: vec![],
            function_call: None,
            function_return: Some(LLMClientMessageFunctionReturn { name, content }),
            cache_point: false,
        }
    }

    pub fn user(message: String) -> Self {
        Self::new(LLMClientRole::User, message, vec![])
    }

    pub fn with_images(mut self, images: Vec<LLMClientMessageImage>) -> Self {
        self.images = images;
        self
    }

    pub fn assistant(message: String) -> Self {
        Self::new(LLMClientRole::Assistant, message, vec![])
    }

    pub fn system(message: String) -> Self {
        Self::new(LLMClientRole::System, message, vec![])
    }

    pub fn content(&self) -> &str {
        &self.message
    }

    pub fn set_empty_content(&mut self) {
        self.message =
            "empty message found here, possibly an error but keep following the conversation"
                .to_owned();
    }

    pub fn function(message: String) -> Self {
        Self::new(LLMClientRole::Function, message, vec![])
    }

    pub fn role(&self) -> &LLMClientRole {
        &self.role
    }

    pub fn get_function_call(&self) -> Option<&LLMClientMessageFunctionCall> {
        self.function_call.as_ref()
    }

    pub fn get_function_return(&self) -> Option<&LLMClientMessageFunctionReturn> {
        self.function_return.as_ref()
    }

    pub fn set_cache_point(&mut self) {
        self.cache_point = true;
    }

    pub fn cache_point(mut self) -> Self {
        self.cache_point = true;
        self
    }

    pub fn is_cache_point(&self) -> bool {
        self.cache_point
    }

    pub fn is_human_message(&self) -> bool {
        matches!(self.role(), &LLMClientRole::User)
    }

    pub fn is_system_message(&self) -> bool {
        matches!(self.role(), &LLMClientRole::System)
    }

    pub fn set_role(mut self, role: LLMClientRole) -> Self {
        self.role = role;
        self
    }

    pub fn images(&self) -> &[LLMClientMessageImage] {
        self.images.as_slice()
    }

    pub fn tools(&self) -> &[serde_json::Value] {
        &self.tools.as_slice()
    }

    pub fn insert_tools(mut self, tools: Vec<serde_json::Value>) -> Self {
        self.tools.extend(tools);
        self
    }

    pub fn insert_tool(mut self, tool: serde_json::Value) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn insert_tool_use(mut self, tool_use: LLMClientToolUse) -> Self {
        self.tool_use.push(tool_use);
        self
    }

    pub fn tool_use_value(&self) -> &[LLMClientToolUse] {
        self.tool_use.as_slice()
    }

    pub fn insert_tool_use_values(mut self, tool_use_vec: Vec<LLMClientToolUse>) -> Self {
        self.tool_use.extend(tool_use_vec);
        self
    }

    pub fn tool_return_value(&self) -> &[LLMClientToolReturn] {
        self.tool_return.as_slice()
    }

    pub fn insert_tool_return_values(mut self, tool_return_vec: Vec<LLMClientToolReturn>) -> Self {
        self.tool_return.extend(tool_return_vec);
        self
    }
}

#[derive(Clone, Debug)]
pub struct LLMClientCompletionRequest {
    model: LLMType,
    messages: Vec<LLMClientMessage>,
    temperature: f32,
    frequency_penalty: Option<f32>,
    stop_words: Option<Vec<String>>,
    max_tokens: Option<usize>,
}

#[derive(Clone)]
pub struct LLMClientCompletionStringRequest {
    model: LLMType,
    prompt: String,
    temperature: f32,
    frequency_penalty: Option<f32>,
    stop_words: Option<Vec<String>>,
    max_tokens: Option<usize>,
}

impl LLMClientCompletionStringRequest {
    pub fn new(
        model: LLMType,
        prompt: String,
        temperature: f32,
        frequency_penalty: Option<f32>,
    ) -> Self {
        Self {
            model,
            prompt,
            temperature,
            frequency_penalty,
            stop_words: None,
            max_tokens: None,
        }
    }

    pub fn set_stop_words(mut self, stop_words: Vec<String>) -> Self {
        self.stop_words = Some(stop_words);
        self
    }

    pub fn model(&self) -> &LLMType {
        &self.model
    }

    pub fn temperature(&self) -> f32 {
        self.temperature
    }

    pub fn frequency_penalty(&self) -> Option<f32> {
        self.frequency_penalty
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn stop_words(&self) -> Option<&[String]> {
        self.stop_words.as_deref()
    }

    pub fn set_max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn get_max_tokens(&self) -> Option<usize> {
        self.max_tokens
    }
}

impl LLMClientCompletionRequest {
    pub fn new(
        model: LLMType,
        messages: Vec<LLMClientMessage>,
        temperature: f32,
        frequency_penalty: Option<f32>,
    ) -> Self {
        Self {
            model,
            messages,
            temperature,
            frequency_penalty,
            stop_words: None,
            max_tokens: None,
        }
    }

    pub fn set_llm(mut self, llm: LLMType) -> Self {
        self.model = llm;
        self
    }

    pub fn fix_message_structure(mut self: Self) -> Self {
        // fix here can mean many things, but here we are going to focus on
        // anthropic since there we need alternating human and assistant message
        // if we have repeating human or assistant messages, then we can just compress
        // them to a single message
        if self.model().is_anthropic() {
            let messages = self.messages;
            let mut final_messages = vec![];
            let mut running_message: Option<LLMClientMessage> = None;
            let mut index = 0;
            dbg!("sidecar.fixies_roles");
            while index < messages.len() {
                let current_message = &messages[index];
                index = index + 1;

                if let Some(ref mut running_message_ref) = running_message {
                    if running_message_ref.role() == current_message.role() {
                        running_message_ref.concat_message(current_message.content());
                    } else {
                        if running_message_ref.content().is_empty() {
                            // figure out how to get rid of empty content messages here
                            running_message_ref.set_empty_content();
                        }
                        final_messages.push(running_message_ref.clone());
                        // and also set the running message as the current
                        // message
                        running_message = Some(current_message.clone());
                    }
                } else {
                    running_message = Some(current_message.clone());
                }
            }
            if let Some(mut message) = running_message {
                if message.message.is_empty() {
                    message.set_empty_content();
                }
                final_messages.push(message);
            }
            self.messages = final_messages;
        }
        self
    }

    pub fn from_messages(messages: Vec<LLMClientMessage>, model: LLMType) -> Self {
        Self::new(model, messages, 0.0, None)
    }

    pub fn set_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    pub fn messages(&self) -> &[LLMClientMessage] {
        self.messages.as_slice()
    }

    pub fn temperature(&self) -> f32 {
        self.temperature
    }

    pub fn frequency_penalty(&self) -> Option<f32> {
        self.frequency_penalty
    }

    pub fn model(&self) -> &LLMType {
        &self.model
    }

    pub fn stop_words(&self) -> Option<&[String]> {
        self.stop_words.as_deref()
    }

    pub fn set_max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn get_max_tokens(&self) -> Option<usize> {
        self.max_tokens
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LLMClientUsageStatistics {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    cached_input_tokens: Option<u32>,
}

impl LLMClientUsageStatistics {
    pub fn new() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
        }
    }

    pub fn add(mut self, other: LLMClientUsageStatistics) -> Self {
        self.input_tokens =
            Some(self.input_tokens.unwrap_or_default() + other.input_tokens.unwrap_or_default());
        self.output_tokens =
            Some(self.output_tokens.unwrap_or_default() + other.output_tokens.unwrap_or_default());
        self.cached_input_tokens = Some(
            self.cached_input_tokens.unwrap_or_default()
                + other.cached_input_tokens.unwrap_or_default(),
        );
        self
    }

    pub fn set_usage_statistics(&mut self, other: LLMClientUsageStatistics) {
        self.input_tokens = other.input_tokens;
        self.output_tokens = other.output_tokens;
        self.cached_input_tokens = other.cached_input_tokens;
    }

    pub fn set_input_tokens(mut self, input_tokens: u32) -> Self {
        self.input_tokens = Some(input_tokens);
        self
    }

    pub fn set_output_tokens(mut self, output_tokens: u32) -> Self {
        self.output_tokens = Some(output_tokens);
        self
    }

    pub fn set_cached_input_tokens(mut self, cached_input_tokens: u32) -> Self {
        self.cached_input_tokens = Some(cached_input_tokens);
        self
    }

    pub fn input_tokens(&self) -> Option<u32> {
        self.input_tokens
    }

    pub fn output_tokens(&self) -> Option<u32> {
        self.output_tokens
    }

    pub fn cached_input_tokens(&self) -> Option<u32> {
        self.cached_input_tokens
    }
}

#[derive(Debug)]
pub struct LLMClientCompletionResponse {
    answer_up_until_now: String,
    delta: Option<String>,
    model: String,
    usage_statistics: LLMClientUsageStatistics,
}

impl LLMClientCompletionResponse {
    pub fn new(answer_up_until_now: String, delta: Option<String>, model: String) -> Self {
        Self {
            answer_up_until_now,
            delta,
            model,
            usage_statistics: LLMClientUsageStatistics::new(),
        }
    }

    pub fn set_usage_statistics(mut self, usage_statistics: LLMClientUsageStatistics) -> Self {
        self.usage_statistics = usage_statistics;
        self
    }

    pub fn usage_statistics(&self) -> LLMClientUsageStatistics {
        self.usage_statistics.clone()
    }

    pub fn answer_up_until_now(&self) -> &str {
        &self.answer_up_until_now
    }

    pub fn delta(&self) -> Option<&str> {
        self.delta.as_deref()
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[derive(Error, Debug)]
pub enum LLMClientError {
    #[error("Failed to get response from LLM")]
    FailedToGetResponse,

    #[error("Event stream error: {0}")]
    EventStreamError(#[from] EventStreamError<reqwest::Error>),

    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),

    #[error("ReqwestMiddlewareError error: {0}")]
    ReqwestMiddlewareError(#[from] reqwest_middleware::Error),

    #[error("serde failed: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("send error over channel: {0}")]
    SendError(#[from] tokio::sync::mpsc::error::SendError<LLMClientCompletionResponse>),

    #[error("unsupported model")]
    UnSupportedModel,

    #[error("OpenAI api error: {0}")]
    OpenAPIError(#[from] async_openai::error::OpenAIError),

    #[error("Wrong api key type")]
    WrongAPIKeyType,

    #[error("OpenAI does not support completion")]
    OpenAIDoesNotSupportCompletion,

    #[error("Sqlite setup error")]
    SqliteSetupError,

    #[error("tokio mspc error")]
    TokioMpscSendError,

    #[error("Failed to store in sqlite DB")]
    FailedToStoreInDB,

    #[error("Sqlx erorr: {0}")]
    SqlxError(#[from] sqlx::Error),

    #[error("Function calling role but not function call present")]
    FunctionCallNotPresent,

    #[error("Gemini pro does not support prompt completion")]
    GeminiProDoesNotSupportPromptCompletion,

    #[error("Unauthorized access to API")]
    UnauthorizedAccess,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,
}

#[async_trait]
pub trait LLMClient {
    fn client(&self) -> &LLMProvider;

    async fn stream_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<LLMClientCompletionResponse, LLMClientError>;

    async fn completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionRequest,
    ) -> Result<String, LLMClientError>;

    async fn stream_prompt_completion(
        &self,
        api_key: LLMProviderAPIKeys,
        request: LLMClientCompletionStringRequest,
        sender: UnboundedSender<LLMClientCompletionResponse>,
    ) -> Result<String, LLMClientError>;
}

#[cfg(test)]
mod tests {
    use super::LLMType;

    #[test]
    fn test_llm_type_from_string() {
        let llm_type = LLMType::Custom("skcd_testing".to_owned());
        let str_llm_type = serde_json::to_string(&llm_type).expect("to work");
        assert_eq!(str_llm_type, "");
    }
}