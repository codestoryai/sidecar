use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        code_symbol::models::anthropic::AnthropicCodeSymbolImportant,
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

use super::types::CodeSymbolError;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClassSymbolFollowupRequest {
    fs_file_path: String,
    original_code: String,
    language: String,
    edited_code: String,
    instructions: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    root_request_id: String,
}

impl ClassSymbolFollowupRequest {
    pub fn new(
        fs_file_path: String,
        original_code: String,
        language: String,
        edited_code: String,
        instructions: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        root_request_id: String,
    ) -> Self {
        Self {
            fs_file_path,
            original_code,
            language,
            edited_code,
            instructions,
            llm,
            provider,
            api_keys,
            root_request_id,
        }
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClassSymbolMember {
    line: String,
    name: String,
    thinking: String,
}

impl ClassSymbolMember {
    pub fn new(line: String, name: String, thinking: String) -> Self {
        Self {
            line,
            name,
            thinking,
        }
    }

    pub fn line(&self) -> &str {
        &self.line
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "members_to_follow")]
pub struct ClassSymbolFollowupResponse {
    #[serde(rename = "$value")]
    members: Vec<ClassSymbolMember>,
}

impl ClassSymbolFollowupResponse {
    pub fn new(members: Vec<ClassSymbolMember>) -> Self {
        Self { members }
    }
    pub fn members(self) -> Vec<ClassSymbolMember> {
        self.members
    }
}

impl ClassSymbolFollowupRequest {
    pub fn llm(&self) -> &LLMType {
        &self.llm
    }

    pub fn provider(&self) -> &LLMProvider {
        &self.provider
    }

    pub fn api_keys(&self) -> &LLMProviderAPIKeys {
        &self.api_keys
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn original_code(&self) -> &str {
        &self.original_code
    }

    pub fn edited_code(&self) -> &str {
        &self.edited_code
    }

    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn language(&self) -> &str {
        &self.language
    }
}

pub struct ClassSymbolFollowupBroker {
    llms: HashMap<LLMType, Box<dyn ClassSymbolFollowup + Send + Sync>>,
}

impl ClassSymbolFollowupBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        let mut llms: HashMap<LLMType, Box<dyn ClassSymbolFollowup + Send + Sync>> =
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
impl Tool for ClassSymbolFollowupBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.class_symbol_followup()?;
        if let Some(implementation) = self.llms.get(context.llm()) {
            let output = implementation
                .get_class_symbol(context)
                .await
                .map_err(|e| ToolError::CodeSymbolError(e))?;
            Ok(ToolOutput::ClassSymbolFollowupResponse(output))
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

#[async_trait]
pub trait ClassSymbolFollowup {
    async fn get_class_symbol(
        &self,
        request: ClassSymbolFollowupRequest,
    ) -> Result<ClassSymbolFollowupResponse, CodeSymbolError>;
}
