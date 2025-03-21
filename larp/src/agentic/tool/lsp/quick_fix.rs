use async_trait::async_trait;

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::text_document::Range,
};
use logging::new_client;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GetQuickFixRequest {
    fs_file_path: String,
    editor_url: String,
    range: Range,
    request_id: String,
}

impl GetQuickFixRequest {
    pub fn new(fs_file_path: String, editor_url: String, range: Range, request_id: String) -> Self {
        Self {
            fs_file_path,
            editor_url,
            range,
            request_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuickFixOption {
    label: String,
    index: i64,
}

impl QuickFixOption {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn index(&self) -> i64 {
        self.index
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GetQuickFixResponse {
    options: Vec<QuickFixOption>,
}

impl GetQuickFixResponse {
    pub fn remove_options(self) -> Vec<QuickFixOption> {
        self.options
    }
}

pub struct LSPQuickFixClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl LSPQuickFixClient {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for LSPQuickFixClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        // we want to make sure that the input over here will have the request id
        // setup properly and things are working
        let context = input.quick_fix_request()?;
        let editor_endpoint = context.editor_url.to_owned() + "/select_quick_fix";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;

        let quick_fix_list: GetQuickFixResponse = response.json().await.map_err(|e| {
            eprintln!("Error response.json(): {:?}", e);
            ToolError::SerdeConversionFailed
        })?;

        Ok(ToolOutput::quick_fix_list(quick_fix_list))
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LSPQuickFixInvocationRequest {
    request_id: String,
    index: i64,
    editor_url: String,
    fs_file_path: String,
}

impl LSPQuickFixInvocationRequest {
    pub fn new(request_id: String, index: i64, editor_url: String, fs_file_path: String) -> Self {
        Self {
            request_id,
            index,
            editor_url,
            fs_file_path,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LSPQuickFixInvocationResponse {
    request_id: String,
    invocation_success: bool,
}

impl LSPQuickFixInvocationResponse {
    pub fn is_success(&self) -> bool {
        self.invocation_success
    }
}

pub struct LSPQuickFixInvocationClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl LSPQuickFixInvocationClient {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for LSPQuickFixInvocationClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.quick_fix_invocation_request()?;
        let editor_endpoint = context.editor_url.to_owned() + "/invoke_quick_fix";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let quick_fix_list: LSPQuickFixInvocationResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;
        Ok(ToolOutput::quick_fix_invocation_result(quick_fix_list))
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
