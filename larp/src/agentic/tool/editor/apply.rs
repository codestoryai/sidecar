use async_trait::async_trait;
use logging::new_client;

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::text_document::Range,
};

pub struct EditorApply {
    client: reqwest_middleware::ClientWithMiddleware,
    apply_edits_directly: bool,
}

impl EditorApply {
    pub fn new(apply_edits_directly: bool) -> Self {
        Self {
            client: new_client(),
            apply_edits_directly,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EditorApplyRequest {
    fs_file_path: String,
    edited_content: String,
    selected_range: Range,
    editor_url: String,
    // we want to apply the edits directly to the file and not stream it
    direct_apply: bool,
}

impl EditorApplyRequest {
    pub fn new(
        fs_file_path: String,
        edited_content: String,
        selected_range: Range,
        editor_url: String,
        direct_apply: bool,
    ) -> Self {
        Self {
            fs_file_path,
            edited_content,
            selected_range,
            editor_url,
            direct_apply,
        }
    }

    fn to_editor_request(self, apply_edits: bool) -> EditorApplyRequestDirect {
        EditorApplyRequestDirect {
            fs_file_path: self.fs_file_path,
            edited_content: self.edited_content,
            selected_range: self.selected_range,
            editor_url: self.editor_url,
            apply_directly: apply_edits || self.direct_apply,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EditorApplyRequestDirect {
    fs_file_path: String,
    edited_content: String,
    selected_range: Range,
    editor_url: String,
    apply_directly: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EditorApplyResponse {
    fs_file_path: String,
    success: bool,
}

impl EditorApply {
    async fn apply_edits(&self, request: EditorApplyRequest) -> Result<ToolOutput, ToolError> {
        println!(
            "framework_event::edit_event::direct_apply::range({:?})::({:?})",
            &request.fs_file_path, &request.selected_range,
        );
        let editor_endpoint = request.editor_url.to_owned() + "/apply_edits";
        let response = self
            .client
            .post(editor_endpoint)
            .body(
                serde_json::to_string(&request.to_editor_request(self.apply_edits_directly))
                    .map_err(|_e| ToolError::SerdeConversionFailed)?,
            )
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: EditorApplyResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;
        Ok(ToolOutput::EditorApplyChanges(response))
    }
}

#[async_trait]
impl Tool for EditorApply {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request = input.editor_apply_changes()?;
        let fs_file_path = request.fs_file_path.to_owned();
        if self.apply_edits_directly || request.direct_apply {
            self.apply_edits(request).await
        } else {
            Ok(ToolOutput::EditorApplyChanges(EditorApplyResponse {
                fs_file_path,
                success: true,
            }))
        }
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
