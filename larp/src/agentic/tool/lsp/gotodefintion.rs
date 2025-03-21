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
pub struct GoToDefinitionRequest {
    fs_file_path: String,
    editor_url: String,
    position: Position,
}

impl GoToDefinitionRequest {
    pub fn new(fs_file_path: String, editor_url: String, position: Position) -> Self {
        Self {
            fs_file_path,
            editor_url,
            position,
        }
    }

    pub fn editor_url(&self) -> &str {
        &self.editor_url
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoToDefinitionResponse {
    definitions: Vec<DefinitionPathAndRange>,
}

impl GoToDefinitionResponse {
    pub fn definitions(self) -> Vec<DefinitionPathAndRange> {
        self.definitions
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DefinitionPathAndRange {
    fs_file_path: String,
    range: Range,
}

impl DefinitionPathAndRange {
    pub fn file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn range(&self) -> &Range {
        &self.range
    }
}

pub struct LSPGoToDefinition {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl LSPGoToDefinition {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for LSPGoToDefinition {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_go_to_definition()?;
        let editor_endpoint = context.editor_url.to_owned() + "/go_to_definition";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: GoToDefinitionResponse = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;

        Ok(ToolOutput::GoToDefinition(response))
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

#[cfg(test)]
mod tests {
    use crate::{
        agentic::tool::{input::ToolInput, r#type::Tool},
        chunking::text_document::Position,
    };

    use super::LSPGoToDefinition;

    /// This test runs with a live editor, sometime later we can abstract this
    /// part out
    #[tokio::test]
    async fn test_lsp_invocation() {
        let input = ToolInput::GoToDefinition(super::GoToDefinitionRequest {
            fs_file_path: "/Users/skcd/scratch/sidecar/sidecar/src/bin/webserver.rs".to_owned(),
            editor_url: "http://localhost:42423".to_owned(),
            position: Position::new(144, 54, 0),
        });
        let lsp_go_to_definition = LSPGoToDefinition::new();
        let result = lsp_go_to_definition.invoke(input).await;
        println!("{:?}", result);
        assert!(false);
    }
}
