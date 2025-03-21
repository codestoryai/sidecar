use reqwest::header::AUTHORIZATION;
use reqwest::header::CONTENT_TYPE;
use reqwest::Client as HttpClient;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::time::Duration;
use uuid::Uuid;

use crate::agent::llm_funcs;

extern crate serde_json;

const API_ENDPOINT: &str = "https://studio.axflow.dev/api/ingest";
const API_KEY: &str = "ax_VPLFeMNVQ9Er3kpcEJkY4e";
const TIMEOUT: &Duration = &Duration::from_millis(1000); // This should be specified by the user

pub struct AxflowClient {
    client: HttpClient,
}

pub fn client() -> AxflowClient {
    let client = HttpClient::builder()
        .timeout(TIMEOUT.clone())
        .build()
        .unwrap(); // Unwrap here is as safe as `HttpClient::new`
    AxflowClient { client }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connection(msg) => write!(f, "Connection Error: {}", msg),
            Error::Serialization(msg) => write!(f, "Serialization Error: {}", msg),
        }
    }
}

// We define the types for the axflow client
#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    User,
    Assistant,
    System,
    Function,
}

impl From<llm_funcs::llm::Role> for Role {
    fn from(role: llm_funcs::llm::Role) -> Self {
        match role {
            llm_funcs::llm::Role::User => Role::User,
            llm_funcs::llm::Role::Assistant => Role::Assistant,
            llm_funcs::llm::Role::System => Role::System,
            llm_funcs::llm::Role::Function => Role::Function,
        }
    }
}

#[derive(serde::Serialize)]
struct FunctionType {}

#[derive(serde::Serialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(serde::Serialize)]
struct MessageType {
    id: String,
    role: Role,
    content: String,
    created: u64,
    functions: Option<Vec<FunctionType>>,
    function_call: Option<FunctionCall>,
}

impl From<llm_funcs::llm::Message> for MessageType {
    fn from(message: llm_funcs::llm::Message) -> Self {
        match message {
            llm_funcs::llm::Message::PlainText { role, content } => MessageType {
                id: Uuid::new_v4().to_string(),
                role: role.into(),
                content,
                created: chrono::Utc::now().timestamp_millis() as u64,
                functions: None,
                function_call: None,
            },
            llm_funcs::llm::Message::FunctionCall {
                role,
                function_call,
                ..
            } => MessageType {
                id: Uuid::new_v4().to_string(),
                role: role.into(),
                content: "".to_owned(),
                created: chrono::Utc::now().timestamp_millis() as u64,
                functions: None,
                function_call: match function_call.name {
                    Some(name) => Some(FunctionCall {
                        name,
                        arguments: function_call.arguments,
                    }),
                    None => None,
                },
            },
            llm_funcs::llm::Message::FunctionReturn {
                role,
                name,
                content,
            } => MessageType {
                id: Uuid::new_v4().to_string(),
                role: role.into(),
                content: content.to_owned(),
                created: chrono::Utc::now().timestamp_millis() as u64,
                functions: None,
                function_call: Some(FunctionCall {
                    name,
                    arguments: content,
                }),
            },
        }
    }
}

#[derive(serde::Serialize)]
struct ConversationType {
    model: String,
    parameters: HashMap<String, String>,
    messages: Vec<MessageType>,
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub enum Error {
    Connection(String),
    Serialization(String),
}

impl AxflowClient {
    pub async fn capture(&self, request: llm_funcs::llm::Request) -> Result<(), Error> {
        let conversation_type = ConversationType {
            model: request.model,
            parameters: vec![{
                if request.temperature.is_some() {
                    Some((
                        "temperature".to_owned(),
                        request.temperature.expect("to be present").to_string(),
                    ))
                } else {
                    None
                }
            }]
            .into_iter()
            .filter_map(|val| val)
            .collect(),
            messages: request
                .messages
                .messages
                .into_iter()
                .map(|message| message.into())
                .collect(),
        };

        let _res = self
            .client
            .post(API_ENDPOINT)
            .header(AUTHORIZATION, format!("Bearer {}", API_KEY))
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&conversation_type).expect("unwrap here is safe"))
            .send()
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(())
    }
}
