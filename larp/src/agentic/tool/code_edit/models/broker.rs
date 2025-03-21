use std::collections::HashMap;

use llm_client::clients::types::{LLMClientCompletionRequest, LLMType};

use crate::agentic::tool::{code_edit::types::CodeEdit, errors::ToolError};

use super::anthropic::AnthropicCodeEditFromatter;

pub struct CodeSnippet {
    snippet_content: String,
    _start_line: i64,
    _end_line: i64,
}

impl CodeSnippet {
    pub fn new(snippet_content: String, start_line: i64, end_line: i64) -> Self {
        Self {
            snippet_content,
            _start_line: start_line,
            _end_line: end_line,
        }
    }

    pub fn snippet_content(&self) -> &str {
        &self.snippet_content
    }
}

pub struct CodeSnippetForEditing {
    snippets: Vec<CodeSnippet>,
    model: LLMType,
    file_path: String,
    user_query: String,
}

impl CodeSnippetForEditing {
    pub fn snippets(&self) -> &[CodeSnippet] {
        self.snippets.as_slice()
    }

    pub fn model(&self) -> &LLMType {
        &self.model
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn new(
        snippets: Vec<CodeSnippet>,
        model: LLMType,
        file_path: String,
        user_query: String,
    ) -> Self {
        Self {
            snippets,
            model,
            file_path,
            user_query,
        }
    }
}

pub trait CodeEditPromptFormatters {
    fn format_prompt(&self, context: &CodeEdit) -> LLMClientCompletionRequest;

    fn find_code_section(&self, context: &CodeSnippetForEditing) -> LLMClientCompletionRequest;
}

pub struct CodeEditBroker {
    models: HashMap<LLMType, Box<dyn CodeEditPromptFormatters + Send + Sync>>,
}

impl CodeEditBroker {
    pub fn new() -> Self {
        let mut models: HashMap<LLMType, Box<dyn CodeEditPromptFormatters + Send + Sync>> =
            HashMap::new();
        models.insert(
            LLMType::ClaudeHaiku,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(
            LLMType::ClaudeSonnet,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(
            LLMType::ClaudeOpus,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(
            LLMType::GeminiPro,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(
            LLMType::GeminiProFlash,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(LLMType::Gpt4O, Box::new(AnthropicCodeEditFromatter::new()));
        models.insert(LLMType::Gpt4, Box::new(AnthropicCodeEditFromatter::new()));
        models.insert(
            LLMType::Gpt4_32k,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(
            LLMType::Llama3_1_8bInstruct,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        models.insert(
            LLMType::Llama3_1_70bInstruct,
            Box::new(AnthropicCodeEditFromatter::new()),
        );
        Self { models }
    }

    pub fn format_prompt(
        &self,
        context: &CodeEdit,
    ) -> Result<LLMClientCompletionRequest, ToolError> {
        let model = context.model();
        if let Some(formatter) = self.models.get(model) {
            Ok(formatter.format_prompt(context))
        } else {
            Err(ToolError::LLMNotSupported)
        }
    }

    pub fn find_code_section_to_edit(
        &self,
        context: &CodeSnippetForEditing,
    ) -> Result<LLMClientCompletionRequest, ToolError> {
        let model = context.model();
        if let Some(formatter) = self.models.get(model) {
            Ok(formatter.find_code_section(context))
        } else {
            Err(ToolError::LLMNotSupported)
        }
    }
}
