//! We can grab the inlay hints from the LSP using this

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
pub struct InlayHintsRequest {
    fs_file_path: String,
    range: Range,
    editor_url: String,
}

impl InlayHintsRequest {
    pub fn new(fs_file_path: String, range: Range, editor_url: String) -> Self {
        Self {
            fs_file_path,
            range,
            editor_url,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlayHintsResponseParts {
    position: Position,
    padding_left: bool,
    padding_right: bool,
    values: Vec<String>,
}

impl InlayHintsResponseParts {
    pub fn position(&self) -> &Position {
        &self.position
    }

    pub fn padding_left(&self) -> bool {
        self.padding_left
    }

    pub fn padding_right(&self) -> bool {
        self.padding_right
    }

    pub fn values(&self) -> &[String] {
        self.values.as_slice()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlayHintsResponse {
    parts: Vec<InlayHintsResponseParts>,
}

impl InlayHintsResponse {
    pub fn parts(self) -> Vec<InlayHintsResponseParts> {
        self.parts
    }

    pub fn new() -> Self {
        Self { parts: vec![] }
    }
}

pub struct InlayHints {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl InlayHints {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for InlayHints {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.inlay_hints_request()?;
        let editor_endpoint = context.editor_url.to_owned() + "/inlay_hints";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: InlayHintsResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;
        Ok(ToolOutput::inlay_hints(response))
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
