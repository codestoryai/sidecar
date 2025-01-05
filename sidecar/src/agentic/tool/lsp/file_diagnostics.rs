use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::diagnostics::Diagnostic;
use crate::{
    agentic::{
        symbol::events::lsp::LSPDiagnosticError,
        tool::{
            errors::ToolError,
            input::ToolInput,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
        },
    },
    chunking::text_document::Position,
};

pub struct FileDiagnostics {
    client: Client,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkspaceDiagnosticsPartial {}

impl WorkspaceDiagnosticsPartial {
    pub fn new() -> Self {
        Self {}
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<thinking>
...
</thinking>
<get_diagnostics>
<fs_file_path>
{{full workspace}}
</fs_file_path>
</get_diagnostics>"#
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileDiagnosticsInput {
    fs_file_path: String,
    editor_url: String,
    with_enrichment: bool,
    with_hover_check: Option<Position>,
    full_workspace: bool,
}

impl FileDiagnosticsInput {
    pub fn new(
        fs_file_path: String,
        editor_url: String,
        with_enrichment: bool,
        with_hover_check: Option<Position>,
        full_workspace: bool,
    ) -> Self {
        Self {
            fs_file_path,
            editor_url,
            with_enrichment,
            with_hover_check,
            full_workspace,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileDiagnosticsOutput {
    diagnostics: Vec<Diagnostic>,
}

/// Diagnostics grouped by fs_file_path
pub type DiagnosticMap = HashMap<String, Vec<LSPDiagnosticError>>;

impl FileDiagnosticsOutput {
    pub fn get_diagnostics(&self) -> &[Diagnostic] {
        self.diagnostics.as_slice()
    }

    pub fn remove_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl FileDiagnostics {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Tool for FileDiagnostics {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_file_diagnostics()?;
        let editor_endpoint = context.editor_url.to_owned() + "/file_diagnostics";
        let response = self
            .client
            .post(editor_endpoint)
            .json(&context)
            .send()
            .await
            .map_err(|e| {
                eprintln!("{:?}", e);
                ToolError::ErrorCommunicatingWithEditor
            })?;

        let diagnostics_response: FileDiagnosticsOutput = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;

        Ok(ToolOutput::file_diagnostics(diagnostics_response))
    }

    // identical to sidecar/src/agentic/tool/lsp/diagnostics.rs
    fn tool_description(&self) -> String {
        "### get_diagnostics
Gets the linter and language server diagnostics wihch have happend after making edits.
Make sure to use this tool if you believe that the code which you just edited could have cascading effects elsewhere or could lead to potential bugs. We want to make sure that the codebase remains intact and no linter or Language Server Problems exist after we are done with the changes. You can act on top of these changes to move towards correctness.".to_owned()
    }

    // identical to sidecar/src/agentic/tool/lsp/diagnostics.rs
    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters: 
- fs_file_path: (required) The ABSOLUTE path of the file to get diagnostics for.

Usage:
<get_diagnostics>
<fs_file_path>
File path here
</fs_file_path>
</get_diagnostics>
"#
        )
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
