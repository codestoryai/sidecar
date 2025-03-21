//! We try to fix the errors which might be present in the code symbol which
//! we have edited before. The major difference here is that we need to show
//! the model how the code was before and how it is after the changes have been
//! made along with the reason to make the change again.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

use super::{models::anthropic::AnthropicCodeSymbolImportant, types::CodeSymbolError};

#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeEditingErrorRequest {
    fs_file_path: String,
    code_above: Option<String>,
    code_below: Option<String>,
    code_in_selection: String,
    extra_context: String,
    original_code: String,
    error_instructions: String,
    previous_instructions: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    root_request_id: String,
}

impl CodeEditingErrorRequest {
    pub fn new(
        fs_file_path: String,
        code_above: Option<String>,
        code_below: Option<String>,
        code_in_selection: String,
        extra_context: String,
        original_code: String,
        error_instructions: String,
        previous_instructions: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        root_request_id: String,
    ) -> Self {
        Self {
            fs_file_path,
            code_above,
            code_below,
            code_in_selection,
            extra_context,
            original_code,
            previous_instructions,
            error_instructions,
            llm,
            provider,
            api_keys,
            root_request_id,
        }
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }

    pub fn llm(&self) -> &LLMType {
        &self.llm
    }

    pub fn llm_provider(&self) -> &LLMProvider {
        &self.provider
    }

    pub fn llm_api_keys(&self) -> &LLMProviderAPIKeys {
        &self.api_keys
    }

    pub fn instructions(&self) -> &str {
        &self.previous_instructions
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn code_above(&self) -> Option<String> {
        self.code_above.clone()
    }

    pub fn code_below(&self) -> Option<String> {
        self.code_below.clone()
    }

    pub fn code_in_selection(&self) -> &str {
        &self.code_in_selection
    }

    pub fn original_code(&self) -> &str {
        &self.original_code
    }

    pub fn error_instructions(&self) -> &str {
        &self.error_instructions
    }

    pub fn extra_context(&self) -> &str {
        &self.extra_context
    }
}

#[async_trait]
pub trait CodeSymbolErrorFix {
    async fn fix_code_symbol(
        &self,
        code_fix: CodeEditingErrorRequest,
    ) -> Result<String, CodeSymbolError>;
}

pub struct CodeSymbolErrorFixBroker {
    llms: HashMap<LLMType, Box<dyn CodeSymbolErrorFix + Send + Sync>>,
}

impl CodeSymbolErrorFixBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        let mut llms: HashMap<LLMType, Box<dyn CodeSymbolErrorFix + Send + Sync>> =
            Default::default();
        llms.insert(
            LLMType::ClaudeHaiku,
            Box::new(AnthropicCodeSymbolImportant::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );
        llms.insert(
            LLMType::ClaudeSonnet,
            Box::new(AnthropicCodeSymbolImportant::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );
        llms.insert(
            LLMType::ClaudeOpus,
            Box::new(AnthropicCodeSymbolImportant::new(
                llm_client.clone(),
                fail_over_llm.clone(),
            )),
        );
        llms.insert(
            LLMType::GeminiPro,
            Box::new(AnthropicCodeSymbolImportant::new(
                llm_client.clone(),
                fail_over_llm,
            )),
        );
        Self { llms }
    }
}

#[async_trait]
impl Tool for CodeSymbolErrorFixBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.code_editing_error()?;
        if let Some(implementation) = self.llms.get(context.llm()) {
            implementation
                .fix_code_symbol(context)
                .await
                .map_err(|e| ToolError::CodeSymbolError(e))
                .map(|output| ToolOutput::CodeEditingForError(output))
        } else {
            Err(ToolError::LLMNotSupported)
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
