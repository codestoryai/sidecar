//! Goes to the previous word in the text document if possible

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::text_document::{Position, Range},
};
use async_trait::async_trait;
use logging::new_client;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoToPreviousWordRequest {
    fs_file_path: String,
    current_position: Position,
    editor_url: String,
}

impl GoToPreviousWordRequest {
    pub fn new(fs_file_path: String, current_position: Position, editor_url: String) -> Self {
        Self {
            fs_file_path,
            current_position,
            editor_url,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoToPreviousWordResponse {
    fs_file_path: String,
    range: Option<Range>,
}

impl GoToPreviousWordResponse {
    pub fn range(&self) -> Option<Range> {
        self.range
    }
}

pub struct GoToPreviousWordClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl GoToPreviousWordClient {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for GoToPreviousWordClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_go_to_previous_word_request()?;
        let endpoint = context.editor_url.to_owned() + "/previous_word_at_position";
        let response = self
            .client
            .post(endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: GoToPreviousWordResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;
        Ok(ToolOutput::GoToPreviousWord(response))
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
