//! We want to get the diagnostics which might be present on a file after making
//! the edit, this is extremely useful to verify if the code written has produced
//! any errors. We have to time when the LSP is ready for providing the diagnostics
//! cause there is no clear way to do that in VScode, as its all async right now
//!
//! Note: we do not store the editor url here since we could have reloaded the editor
//! and the url changes because of that
use async_trait::async_trait;
use thiserror::Error;

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::text_document::Range,
};

pub struct LSPDiagnostics {
    client: reqwest::Client,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct LSPDiagnosticsInput {
    fs_file_path: String,
    range: Range,
    editor_url: String,
}

impl LSPDiagnosticsInput {
    pub fn new(fs_file_path: String, range: Range, editor_url: String) -> Self {
        Self {
            fs_file_path,
            range,
            editor_url,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Diagnostic {
    message: String,
    range: Range,
    quick_fix_labels: Option<Vec<String>>,
    parameter_hints: Option<Vec<String>>,
    fs_file_path: String,
}

impl Diagnostic {
    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn with_snippet_from_contents(
        self,
        file_contents: &str,
        fs_file_path: &str,
    ) -> Result<DiagnosticWithSnippet, DiagnosticSnippetError> {
        DiagnosticWithSnippet::from_diagnostic_and_contents(
            self,
            file_contents,
            fs_file_path.to_owned(),
        )
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DiagnosticWithSnippet {
    message: String,
    range: Range,
    snippet: String,
    fs_file_path: String,
    quick_fix_labels: Option<Vec<String>>,
    parameter_hints: Option<Vec<String>>,
}

impl DiagnosticWithSnippet {
    pub fn new(message: String, range: Range, snippet: String, fs_file_path: String) -> Self {
        Self {
            message,
            range,
            snippet,
            fs_file_path,
            quick_fix_labels: None,
            parameter_hints: None,
        }
    }

    pub fn from_diagnostic_and_contents(
        diagnostic: Diagnostic,
        file_contents: &str,
        _fs_file_path: String,
    ) -> Result<Self, DiagnosticSnippetError> {
        let Diagnostic {
            range,
            message,
            quick_fix_labels,
            parameter_hints,
            fs_file_path,
        } = diagnostic;

        let start_line = range.start_line();
        let end_line = range.end_line();

        let lines: Vec<&str> = file_contents.lines().collect();

        // Safely slice the vector
        let relevant_lines = lines
            .get(start_line..=end_line)
            .ok_or(DiagnosticSnippetError::InvalidRange(range))?;

        let snippet = relevant_lines.join("\n").to_owned();

        Ok(Self {
            message,
            range,
            snippet,
            fs_file_path,
            quick_fix_labels,
            parameter_hints,
        })
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn snippet(&self) -> &str {
        &self.snippet
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn quick_fix_labels(&self) -> &Option<Vec<String>> {
        &self.quick_fix_labels
    }

    pub fn parameter_hints(&self) -> &Option<Vec<String>> {
        &self.parameter_hints
    }
}

#[derive(Debug, Error)]
pub enum DiagnosticSnippetError {
    #[error("Invalid range: {0:?}")]
    InvalidRange(Range),
    #[error("File content error: {0}")]
    FileContentError(String),
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LSPDiagnosticsOutput {
    diagnostics: Vec<Diagnostic>,
}

impl LSPDiagnosticsOutput {
    pub fn get_diagnostics(&self) -> &[Diagnostic] {
        self.diagnostics.as_slice()
    }

    pub fn remove_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl LSPDiagnostics {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for LSPDiagnostics {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_lsp_diagnostics()?;
        let editor_endpoint = context.editor_url.to_owned() + "/diagnostics";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let diagnostics_response: LSPDiagnosticsOutput = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;

        Ok(ToolOutput::lsp_diagnostics(diagnostics_response))
    }

    // identical to sidecar/src/agentic/tool/lsp/file_diagnostics.rs
    fn tool_description(&self) -> String {
        "Get LSP diagnostics for a file".to_owned()
    }

    // identical to sidecar/src/agentic/tool/lsp/file_diagnostics.rs
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
