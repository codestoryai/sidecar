use llm_client::{
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};
use regex::Regex;

use super::model_selection::LLMClientConfig;
use crate::{
    chunking::text_document::{Position, Range},
    repo::types::RepoRef,
    user_context::types::UserContext,
};

/// This module contains all the helper structs which we need to enable in-editor experience

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetInformation {
    pub start_position: Position,
    pub end_position: Position,
    pub should_use_exact_matching: bool,
}

impl SnippetInformation {
    pub fn to_range(&self) -> Range {
        Range::new(self.start_position.clone(), self.end_position.clone())
    }

    pub fn set_start_byte_offset(&mut self, byte_offset: usize) {
        self.start_position.set_byte_offset(byte_offset);
    }

    pub fn set_end_byte_offset(&mut self, byte_offset: usize) {
        self.end_position.set_byte_offset(byte_offset);
    }

    pub fn exact_selection(&self) -> bool {
        self.should_use_exact_matching
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentWeb {
    pub text: String,
    pub utf8_array: Vec<u8>,
    pub language: String,
    pub fs_file_path: String,
    pub relative_path: String,
    pub line_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRelatedInformation {
    pub text: String,
    pub language: String,
    pub range: Range,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticInformation {
    pub prompt_parts: Vec<String>,
    pub related_information: Vec<DiagnosticRelatedInformation>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticInformationFromEditor {
    pub first_message: String,
    pub diagnostic_information: Vec<DiagnosticInformation>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInEditorRequest {
    pub query: String,
    pub language: String,
    pub repo_ref: RepoRef,
    pub user_context: UserContext,
    pub snippet_information: SnippetInformation,
    pub text_document_web: TextDocumentWeb,
    pub thread_id: uuid::Uuid,
    pub diagnostics_information: Option<DiagnosticInformationFromEditor>,
    pub model_config: LLMClientConfig,
}

impl ProcessInEditorRequest {
    pub fn source_code(&self) -> &str {
        &self.text_document_web.text
    }

    pub fn source_code_bytes(&self) -> &[u8] {
        &self.text_document_web.utf8_array
    }

    pub fn language(&self) -> &str {
        &self.text_document_web.language
    }

    pub fn line_count(&self) -> usize {
        self.text_document_web.line_count
    }

    pub fn start_position(&self) -> Position {
        self.snippet_information.start_position.clone()
    }

    pub fn end_position(&self) -> Position {
        self.snippet_information.end_position.clone()
    }

    pub fn fs_file_path(&self) -> &str {
        &self.text_document_web.fs_file_path
    }

    /// Grabs the fast model from the model configuration
    pub fn fast_model(&self) -> LLMType {
        self.model_config.fast_model.clone()
    }

    pub fn slow_model(&self) -> LLMType {
        self.model_config.slow_model.clone()
    }

    pub fn exact_selection(&self) -> bool {
        self.snippet_information.exact_selection()
    }

    /// Grabs the provider required for the fast model
    pub fn provider_for_slow_model(&self) -> Option<&LLMProviderAPIKeys> {
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        let model = self.model_config.models.get(&self.model_config.slow_model);
        if let None = model {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        // get the related provider if its present
        self.model_config
            .providers
            .iter()
            .find(|p| p.key(provider).is_some())
    }

    /// Grabs the provider required for the fast model
    pub fn provider_for_fast_model(&self) -> Option<&LLMProviderAPIKeys> {
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        let model = self.model_config.models.get(&self.model_config.fast_model);
        if let None = model {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        // get the related provider if its present
        self.model_config
            .providers
            .iter()
            .find(|p| p.key(provider).is_some())
    }

    pub fn provider_config_for_fast_model(&self) -> Option<&LLMProvider> {
        self.model_config
            .models
            .get(&self.model_config.fast_model)
            .map(|model_config| &model_config.provider)
    }

    pub fn provider_config_for_slow_model(&self) -> Option<&LLMProvider> {
        self.model_config
            .models
            .get(&self.model_config.slow_model)
            .map(|model_config| &model_config.provider)
    }

    /// Are we using OpenAI models
    pub fn using_openai_models(&self) -> bool {
        self.model_config.fast_model.is_openai()
    }
}

fn line_column_to_byte_offset(
    lines: Vec<&str>,
    target_line: usize,
    target_column: usize,
) -> Option<usize> {
    // Keep track of the current line and column in the input text
    let mut current_byte_offset = 0;

    for (index, line) in lines.iter().enumerate() {
        if index == target_line {
            let mut current_col = 0;

            // If the target column is at the beginning of the line
            if target_column == 0 {
                return Some(current_byte_offset);
            }

            for char in line.chars() {
                if current_col == target_column {
                    return Some(current_byte_offset);
                }
                current_byte_offset += char.len_utf8();
                current_col += 1;
            }

            // If the target column is exactly at the end of this line
            if current_col == target_column {
                return Some(current_byte_offset); // target_column is at the line break
            }

            // Column requested is beyond the current line length
            return None;
        }

        // Increment the byte offset by the length of the current line and its newline
        current_byte_offset += line.len() + "\n".len(); // add 1 for the newline character
    }

    // Line requested is beyond the input text line count
    None
}

pub fn fix_snippet_information(
    mut snippet_information: SnippetInformation,
    text_bytes: &[u8],
) -> SnippetInformation {
    // First we convert from the bytes to the string
    let text_str = String::from_utf8(text_bytes.to_vec()).unwrap_or_default();
    // Now we have to split the text on the new lines
    let re = Regex::new(r"\r\n|\r|\n").unwrap();

    // Split the string using the regex pattern
    let lines: Vec<&str> = re.split(&text_str).collect();

    let start_position_byte_offset = line_column_to_byte_offset(
        lines.to_vec(),
        snippet_information.start_position.line(),
        snippet_information.start_position.column(),
    );

    let end_position_byte_offset = line_column_to_byte_offset(
        lines.to_vec(),
        snippet_information.end_position.line(),
        snippet_information.end_position.column(),
    );

    if let Some(start_position_byte) = start_position_byte_offset {
        snippet_information.set_start_byte_offset(start_position_byte);
    }

    if let Some(end_position_byte) = end_position_byte_offset {
        snippet_information.set_end_byte_offset(end_position_byte);
    }

    snippet_information
}
