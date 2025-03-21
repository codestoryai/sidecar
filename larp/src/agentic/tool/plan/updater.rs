use async_trait::async_trait;
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
    provider::{AnthropicAPIKey, LLMProvider, LLMProviderAPIKeys},
};
use std::sync::Arc;

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

use super::{plan::Plan, plan_step::PlanStep};

#[derive(Debug, Clone)]
pub struct PlanUpdateRequest {
    plan: Plan,
    new_context: String,
    checkpoint_index: usize,
    update_query: String,
    root_request_id: String,
    editor_url: String,
}

impl PlanUpdateRequest {
    pub fn new(
        plan: Plan,
        new_context: String,
        checkpoint_index: usize,
        update_query: String,
        root_request_id: String,
        editor_url: String,
    ) -> Self {
        Self {
            plan,
            new_context,
            checkpoint_index,
            update_query,
            root_request_id,
            editor_url,
        }
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn new_context(&self) -> &str {
        &self.new_context
    }

    pub fn checkpoint_index(&self) -> usize {
        self.checkpoint_index
    }

    pub fn update_query(&self) -> &str {
        &self.update_query
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }

    pub fn editor_url(&self) -> &str {
        &self.editor_url
    }
}

pub struct PlanUpdaterClient {
    llm_client: Arc<LLMBroker>,
}

impl PlanUpdaterClient {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    pub fn system_message(&self) -> String {
        format!(
            r#"You are an assistant that helps update a plan based on new information.

**Initial Context**:

**User Query**:

**Edit History (Steps up to checkpoint)**:

**Current Steps After Checkpoint**:

**New Context**:

Based on the above, please update the steps after the checkpoint to incorporate the new information and address the user's query. Provide the updated steps in a numbered list format.
"#
        )
    }

    fn format_steps(steps: &[PlanStep], start_index: usize) -> String {
        steps
            .iter()
            .enumerate()
            .map(|(i, step)| format!("{}. {}", start_index + i + 1, step.description()))
            .collect::<Vec<String>>()
            .join("\n")
    }

    pub fn user_message(&self, context: PlanUpdateRequest) -> String {
        let plan = context.plan();
        let steps = plan.steps();

        let checkpoint_index = context.checkpoint_index();
        // let initial_context = plan.initial_context(); // original plan context

        let steps_up_to_checkpoint = Self::format_steps(&steps[..checkpoint_index], 0);

        let steps_after_checkpoint =
            Self::format_steps(&steps[checkpoint_index..], checkpoint_index);

        // remove initial context over here for now
        format!(
            r#"You are an assistant that helps update a plan based on new information.

**User Query**:
{}

**Edit History (Steps up to checkpoint)**:
{}

**Current Steps After Checkpoint**:
{}

**New Context**:
{}

Based on the above, please update the steps after the checkpoint to incorporate the new information and address the user's query. Provide the updated steps in a numbered list format."#,
            // initial_context,
            context.update_query(),
            steps_up_to_checkpoint,
            steps_after_checkpoint,
            context.new_context()
        )
    }
}

#[async_trait]
impl Tool for PlanUpdaterClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        // check whether tool_input is for plan updater
        let context = input.plan_updater()?;

        let _editor_url = context.editor_url.to_owned();
        let root_id = context.root_request_id.to_owned();

        let messages = vec![
            LLMClientMessage::system(self.system_message()),
            LLMClientMessage::user(self.user_message(context)),
        ];

        let request = LLMClientCompletionRequest::new(LLMType::ClaudeSonnet, messages, 0.2, None);

        let llm_properties = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::Anthropic,
            LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned())),
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();

        let response = self
            .llm_client
            .stream_completion(
                llm_properties.api_key().clone(),
                request,
                llm_properties.provider().clone(),
                vec![
                    ("root_id".to_owned(), root_id),
                    ("event_type".to_owned(), format!("update_plan").to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await?;

        dbg!(response);

        // parse
        todo!() // todo(zi) holy shit careful here

        // damn, this is actually unfinished
    }

    fn tool_description(&self) -> String {
        "unfinished".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "missing_tool_input".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
