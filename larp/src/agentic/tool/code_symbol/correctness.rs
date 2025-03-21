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
        lsp::{diagnostics::DiagnosticWithSnippet, quick_fix::QuickFixOption},
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale, ToolType},
    },
};

use super::{models::anthropic::AnthropicCodeSymbolImportant, types::CodeSymbolError};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct CodeCorrectnessAction {
    thinking: String,
    index: i64,
}

impl CodeCorrectnessAction {
    pub fn thinking(&self) -> &str {
        &self.thinking
    }

    pub fn index(&self) -> i64 {
        self.index
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeCorrectnessRequest {
    code_in_selection: String,
    symbol_name: String,
    instruction: String,
    diagnostic_with_snippet: DiagnosticWithSnippet,
    quick_fix_actions: Vec<QuickFixOption>,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    // the extra symbols which we will be creating and are part of the plan
    // helps keep the edits in a course correct way
    extra_symbol_plan: Option<String>,
    root_request_id: String,
}

impl CodeCorrectnessRequest {
    pub fn new(
        code_in_selection: String,
        symbol_name: String,
        instruction: String,
        diagnostic_with_snippet: DiagnosticWithSnippet,
        quick_fix_actions: Vec<QuickFixOption>,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        extra_symbol_plan: Option<String>,
        root_request_id: String,
    ) -> Self {
        Self {
            code_in_selection,
            quick_fix_actions,
            instruction,
            symbol_name,
            diagnostic_with_snippet,
            llm,
            provider,
            api_keys,
            extra_symbol_plan,
            root_request_id,
        }
    }

    pub fn extra_symbol_plan(&self) -> Option<String> {
        self.extra_symbol_plan.clone()
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }

    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    pub fn diagnostic_with_snippet(&self) -> &DiagnosticWithSnippet {
        &self.diagnostic_with_snippet
    }

    pub fn quick_fix_actions(&self) -> &[QuickFixOption] {
        self.quick_fix_actions.as_slice()
    }

    pub fn code_in_selection(&self) -> &str {
        &self.code_in_selection
    }

    pub fn instruction(&self) -> &str {
        &self.instruction
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
}

#[async_trait]
pub trait CodeCorrectness {
    async fn decide_tool_use(
        &self,
        code_correctness_request: CodeCorrectnessRequest,
    ) -> Result<CodeCorrectnessAction, CodeSymbolError>;
}

pub struct CodeCorrectnessBroker {
    llms: HashMap<LLMType, Box<dyn CodeCorrectness + Send + Sync>>,
}

impl CodeCorrectnessBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        let mut llms: HashMap<LLMType, Box<dyn CodeCorrectness + Send + Sync>> = Default::default();
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
            LLMType::Gpt4O,
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
impl Tool for CodeCorrectnessBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.code_correctness_action()?;
        if let Some(implementation) = self.llms.get(context.llm()) {
            implementation
                .decide_tool_use(context)
                .await
                .map(|response| ToolOutput::code_correctness_action(response))
                .map_err(|e| ToolError::CodeSymbolError(e))
        } else {
            Err(ToolError::WrongToolInput(
                ToolType::CodeCorrectnessActionSelection,
            ))
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
