//! The edited files and the git-diff which is ordered by timestamp
//! The idea is that the file which we are editing can go last

use crate::agentic::tool::{
    errors::ToolError,
    helpers::diff_recent_changes::DiffFileContent,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};
use async_trait::async_trait;
use logging::new_client;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EditedFilesRequest {
    editor_url: String,
    diff_file_content: Vec<DiffFileContent>,
}

impl EditedFilesRequest {
    pub fn new(editor_url: String, diff_file_content: Vec<DiffFileContent>) -> Self {
        Self {
            editor_url,
            diff_file_content,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EditedGitDiffFile {
    fs_file_path: String,
    diff: String,
    current_content: String,
    updated_timestamp_ms: i64,
    change_frequency: u32,
    importance_score: u8,
    semantic_relation_score: f32,
    user_interaction_score: u8,
}

impl EditedGitDiffFile {
    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn diff(&self) -> &str {
        &self.diff
    }

    pub fn updated_timestamp_ms(&self) -> i64 {
        self.updated_timestamp_ms
    }

    pub fn current_content(&self) -> &str {
        &self.current_content
    }

    pub fn change_frequency(&self) -> u32 {
        self.change_frequency
    }

    pub fn importance_score(&self) -> u8 {
        self.importance_score
    }

    pub fn semantic_relation_score(&self) -> f32 {
        self.semantic_relation_score
    }

    pub fn user_interaction_score(&self) -> u8 {
        self.user_interaction_score
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EditedFilesResponse {
    changed_files: Vec<EditedGitDiffFile>,
}

impl EditedFilesResponse {
    pub fn changed_files(self) -> Vec<EditedGitDiffFile> {
        self.changed_files
    }
}

pub struct EditedFiles {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl EditedFiles {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for EditedFiles {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        match input {
            ToolInput::EditedFiles(request) => {
                let response = self
                    .client
                    .get(&format!("{}/api/v1/edited-files", request.editor_url))
                    .send()
                    .await
                    .map_err(|e| ToolError::NetworkError(e.to_string()))?
                    .json::<EditedFilesResponse>()
                    .await
                    .map_err(|e| ToolError::DeserializationError(e.to_string()))?;

                // Calculate weights for each file
                let weighted_files = response.changed_files().into_iter().map(|file| {
                    let weight = ChangeWeight::new(
                        file.change_frequency(),
                        file.importance_score(),
                        file.semantic_relation_score(),
                        file.user_interaction_score(),
                    );
                    (file, weight)
                });

                // Create DiffFileContent with weights
                let file_contents = weighted_files
                    .map(|(file, weight)| {
                        DiffFileContent::new(
                            file.fs_file_path().to_owned(),
                            file.current_content().to_owned(),
                            None,
                            Some(weight),
                            file.updated_timestamp_ms(),
                        )
                    })
                    .collect();

                Ok(ToolOutput::EditedFiles(EditedFilesResponse {
                    changed_files: file_contents,
                }))
            }
            _ => Err(ToolError::WrongToolInput),
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

    fn reward_scale(&self) -> ToolRewardScale {
        ToolRewardScale::new(1.0, 0.0)
    }
}
