//! Exposes a client to start a new exchange when we want it, can be used by the
//! agent to send replies, followings etc

use async_trait::async_trait;
use logging::new_client;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionExchangeNewRequest {
    session_id: String,
    editor_url: String,
}

impl SessionExchangeNewRequest {
    pub fn new(session_id: String, editor_url: String) -> Self {
        Self {
            session_id,
            editor_url,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SessionExchangeNewResponse {
    exchange_id: Option<String>,
}

impl SessionExchangeNewResponse {
    pub fn exchange_id(self) -> Option<String> {
        self.exchange_id
    }
}

pub struct SessionExchangeClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl SessionExchangeClient {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for SessionExchangeClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_new_exchange_during_session()?;
        let endpoint = context.editor_url.to_owned() + "/new_exchange";
        let response = self
            .client
            .post(endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let new_exchange: SessionExchangeNewResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;
        println!(
            "tool_box::session_exchange_client::new_response::({:?})",
            &new_exchange
        );
        Ok(ToolOutput::new_exchange_during_session(new_exchange))
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
