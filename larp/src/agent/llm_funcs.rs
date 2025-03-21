use llm_client::clients::types::LLMClientMessage;
use tiktoken_rs::FunctionCall as tiktoken_rs_FunctionCall;

pub mod llm {
    use std::collections::HashMap;

    #[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct FunctionCall {
        pub name: Option<String>,
        pub arguments: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Function {
        pub name: String,
        pub description: String,
        pub parameters: Parameters,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Parameters {
        #[serde(rename = "type")]
        pub _type: String,
        pub properties: HashMap<String, Parameter>,
        pub required: Vec<String>,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    pub enum Role {
        User,
        System,
        Assistant,
        Function,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Parameter {
        #[serde(rename = "type")]
        pub _type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub items: Option<Box<Parameter>>,
    }
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
    #[serde(untagged)]
    pub enum Message {
        FunctionReturn {
            role: Role,
            name: String,
            content: String,
        },
        FunctionCall {
            role: Role,
            function_call: FunctionCall,
            content: (),
        },
        // NB: This has to be the last variant as this enum is marked `#[serde(untagged)]`, so
        // deserialization will always try this variant last. Otherwise, it is possible to
        // accidentally deserialize a `FunctionReturn` value as `PlainText`.
        PlainText {
            role: Role,
            content: String,
        },
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Messages {
        pub messages: Vec<Message>,
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Functions {
        pub functions: Vec<Function>,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    pub struct Request {
        pub messages: Messages,
        pub functions: Option<Functions>,
        pub provider: Provider,
        pub max_tokens: Option<u32>,
        pub temperature: Option<f32>,
        pub presence_penalty: Option<f32>,
        pub frequency_penalty: Option<f32>,
        pub model: String,
        #[serde(default)]
        pub extra_stop_sequences: Vec<String>,
        pub session_reference_id: Option<String>,
    }

    #[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Provider {
        OpenAi,
    }

    #[derive(thiserror::Error, Debug, serde::Deserialize, serde::Serialize)]
    pub enum Error {
        #[error("bad OpenAI request")]
        BadOpenAiRequest,

        #[error("incorrect configuration")]
        BadConfiguration,
    }

    pub type Result = std::result::Result<String, Error>;
}

impl llm::Message {
    pub fn system(content: &str) -> Self {
        llm::Message::PlainText {
            role: llm::Role::System,
            content: content.to_owned(),
        }
    }

    pub fn user(content: &str) -> Self {
        llm::Message::PlainText {
            role: llm::Role::User,
            content: content.to_owned(),
        }
    }

    pub fn function_call(function_call: llm::FunctionCall) -> Self {
        // This is where the assistant ends up calling the function
        llm::Message::FunctionCall {
            role: llm::Role::Assistant,
            function_call,
            content: (),
        }
    }

    pub fn function_return(name: String, content: String) -> Self {
        // This is where we assume that the function is the one returning
        // the answer to the agent
        llm::Message::FunctionReturn {
            role: llm::Role::Function,
            name,
            content,
        }
    }

    pub fn role(&self) -> llm::Role {
        match self {
            &llm::Message::FunctionCall {
                ref role,
                function_call: _,
                content: _,
            } => role.clone(),
            &llm::Message::FunctionReturn {
                ref role,
                name: _,
                content: _,
            } => role.clone(),
            &llm::Message::PlainText {
                ref role,
                content: _,
            } => role.clone(),
        }
    }
}

impl From<&llm::Role> for async_openai::types::Role {
    fn from(role: &llm::Role) -> Self {
        match role {
            llm::Role::User => async_openai::types::Role::User,
            llm::Role::System => async_openai::types::Role::System,
            llm::Role::Assistant => async_openai::types::Role::Assistant,
            llm::Role::Function => async_openai::types::Role::Function,
        }
    }
}

impl llm::Role {
    pub fn to_string(&self) -> String {
        match self {
            llm::Role::Assistant => "assistant".to_owned(),
            llm::Role::Function => "function".to_owned(),
            llm::Role::System => "system".to_owned(),
            llm::Role::User => "user".to_owned(),
        }
    }
}

impl TryFrom<&llm::Message> for LLMClientMessage {
    type Error = anyhow::Error;
    fn try_from(m: &llm::Message) -> anyhow::Result<LLMClientMessage> {
        match m {
            llm::Message::PlainText { ref role, content } => match role {
                &llm::Role::Assistant => Ok(LLMClientMessage::assistant(content.to_owned())),
                &llm::Role::System => Ok(LLMClientMessage::system(content.to_owned())),
                &llm::Role::User => Ok(LLMClientMessage::user(content.to_owned())),
                &llm::Role::Function => Ok(LLMClientMessage::function(content.to_owned())),
            },
            llm::Message::FunctionCall {
                role,
                function_call,
                content: _,
            } => match role {
                &llm::Role::Assistant => Ok(LLMClientMessage::function_call(
                    function_call
                        .name
                        .as_ref()
                        .map(|name| name.to_owned())
                        .unwrap_or_default(),
                    function_call.arguments.to_owned(),
                )),
                _ => Err(anyhow::anyhow!("Invalid role found")),
            },
            llm::Message::FunctionReturn {
                role: _,
                name,
                content,
            } => Ok(LLMClientMessage::function_return(
                name.to_owned(),
                content.to_owned(),
            )),
        }
    }
}

impl From<&llm::Message> for tiktoken_rs::ChatCompletionRequestMessage {
    fn from(m: &llm::Message) -> tiktoken_rs::ChatCompletionRequestMessage {
        match m {
            llm::Message::PlainText { role, content } => {
                tiktoken_rs::ChatCompletionRequestMessage {
                    role: role.to_string(),
                    content: Some(content.to_owned()),
                    name: None,
                    function_call: None,
                }
            }
            llm::Message::FunctionReturn {
                role,
                name,
                content,
            } => tiktoken_rs::ChatCompletionRequestMessage {
                role: role.to_string(),
                content: Some(content.to_owned()),
                name: Some(name.clone()),
                function_call: None,
            },
            llm::Message::FunctionCall {
                role,
                function_call,
                content: _,
            } => tiktoken_rs::ChatCompletionRequestMessage {
                role: role.to_string(),
                content: None,
                name: None,
                function_call: Some(tiktoken_rs_FunctionCall {
                    name: function_call
                        .name
                        .as_ref()
                        .expect("function_name to exist for function_call")
                        .to_owned(),
                    arguments: function_call.arguments.to_owned(),
                }),
            },
        }
    }
}
