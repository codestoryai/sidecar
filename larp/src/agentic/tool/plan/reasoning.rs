//! Reasoning tool, we just show it all the information we can and ask it for a query
//! to come up with a plan and thats it

use async_trait::async_trait;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
    provider::{LLMProvider, LLMProviderAPIKeys, OpenAIProvider},
};

use crate::{
    agentic::{
        symbol::{identifier::LLMProperties, ui_event::EditedCodeStreamingRequest},
        tool::{
            code_edit::search_and_replace::StreamedEditingForEditor,
            errors::ToolError,
            input::ToolInput,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
        },
    },
    chunking::text_document::{Position, Range},
};

#[derive(Debug, Clone)]
pub struct ReasoningResponse {
    response: String,
}

impl ReasoningResponse {
    pub fn response(self) -> String {
        self.response
    }
}

#[derive(Debug, Clone)]
pub struct ReasoningRequest {
    user_query: String,
    files_in_selection: String,
    code_in_selection: String,
    lsp_diagnostics: String,
    diff_recent_edits: String,
    root_request_id: String,
    // These 2 are weird and not really required over here, we are using this
    // as a proxy to output the plan to a file path
    plan_output_path: String,
    plan_output_content: String,
    aide_rules: Option<String>,
    editor_url: String,
    session_id: String,
    exchange_id: String,
}

impl ReasoningRequest {
    pub fn new(
        user_query: String,
        files_in_selection: String,
        code_in_selection: String,
        lsp_diagnostics: String,
        diff_recent_edits: String,
        root_request_id: String,
        plan_output_path: String,
        plan_output_content: String,
        aide_rules: Option<String>,
        editor_url: String,
        session_id: String,
        exchange_id: String,
    ) -> Self {
        Self {
            user_query,
            files_in_selection,
            code_in_selection,
            lsp_diagnostics,
            diff_recent_edits,
            root_request_id,
            plan_output_path,
            plan_output_content,
            aide_rules,
            editor_url,
            session_id,
            exchange_id,
        }
    }
}

pub struct ReasoningClient {
    llm_client: Arc<LLMBroker>,
}

impl ReasoningClient {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn user_message(&self, context: ReasoningRequest) -> String {
        let user_query = context.user_query;
        let files_in_selection = context.files_in_selection;
        let code_in_selection = context.code_in_selection;
        let lsp_diagnostics = context.lsp_diagnostics;
        let diff_recent_edits = context.diff_recent_edits;
        let aide_rules = context.aide_rules.clone().unwrap_or_default();
        format!(
            r#"<files_in_selection>
{files_in_selection}
</files_in_selection>
<recent_diff_edits>
{diff_recent_edits}
</recent_diff_edits>
<lsp_diagnostics>
{lsp_diagnostics}
</lsp_diagnostics>
<code_in_selection>
{code_in_selection}
</code_in_selection>

I have provided you with the following context:
- <files_in_selection>
These are the files which are present in context that is useful
- <recent_diff_edits>
The recent edits which have been made to the files
- <lsp_diagnostics>
The diagnostic errors which are generated from the Language Server running inside the editor
- <code_in_selection>
These are the code sections which are in our selection

Additional rules and guidelines which the user has provided to you:
{aide_rules}

The query I want help with:
{user_query}"#
        )
    }
}

#[async_trait]
impl Tool for ReasoningClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.should_reasoning()?;
        let editor_url = context.editor_url.to_owned();
        let scratch_pad_path = context.plan_output_path.to_owned();
        let scratch_pad_content = context.plan_output_content.to_owned();
        let root_id = context.root_request_id.to_owned();
        let session_id = context.session_id.to_owned();
        let exchange_id = context.exchange_id.to_owned();
        let request = LLMClientCompletionRequest::new(
            LLMType::O1Preview,
            vec![LLMClientMessage::user(self.user_message(context))],
            1.0,
            None,
        );
        let llm_properties = LLMProperties::new(
            LLMType::O1Preview,
            LLMProvider::OpenAI,
            LLMProviderAPIKeys::OpenAI(OpenAIProvider::new("".to_owned())),
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let model_str = llm_properties.llm().to_string();
        let response = self
            .llm_client
            .stream_completion(
                llm_properties.api_key().clone(),
                request,
                llm_properties.provider().clone(),
                vec![
                    ("root_id".to_owned(), root_id),
                    (
                        "event_type".to_owned(),
                        format!("reasoning_{}", model_str).to_owned(),
                    ),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await;
        let output = response
            .map(|response| response)
            .map_err(|e| ToolError::LLMClientError(e))?;

        let scratch_pad_range = Range::new(
            Position::new(0, 0, 0),
            Position::new(
                {
                    let lines = scratch_pad_content
                        .lines()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .len();
                    if lines == 0 {
                        0
                    } else {
                        lines - 1
                    }
                },
                1000,
                0,
            ),
        );

        // Now send this over for writing to the LLM
        let edit_request_id = uuid::Uuid::new_v4().to_string();
        let fs_file_path = scratch_pad_path.to_owned();
        let streamed_edit_client = StreamedEditingForEditor::new();
        streamed_edit_client
            .send_edit_event(
                editor_url.to_owned(),
                EditedCodeStreamingRequest::start_edit(
                    edit_request_id.to_owned(),
                    session_id.to_owned(),
                    scratch_pad_range.clone(),
                    fs_file_path.to_owned(),
                    exchange_id.to_owned(),
                    None,
                )
                .set_apply_directly(),
            )
            .await;
        streamed_edit_client
            .send_edit_event(
                editor_url.to_owned(),
                EditedCodeStreamingRequest::delta(
                    edit_request_id.to_owned(),
                    session_id.to_owned(),
                    scratch_pad_range.clone(),
                    fs_file_path.to_owned(),
                    "```\n".to_owned(),
                    exchange_id.to_owned(),
                    None,
                )
                .set_apply_directly(),
            )
            .await;
        let _ = streamed_edit_client
            .send_edit_event(
                editor_url.to_owned(),
                EditedCodeStreamingRequest::delta(
                    edit_request_id.to_owned(),
                    session_id.to_owned(),
                    scratch_pad_range.clone(),
                    fs_file_path.to_owned(),
                    output.answer_up_until_now().to_owned(),
                    exchange_id.to_owned(),
                    None,
                )
                .set_apply_directly(),
            )
            .await;
        let _ = streamed_edit_client
            .send_edit_event(
                editor_url.to_owned(),
                EditedCodeStreamingRequest::delta(
                    edit_request_id.to_owned(),
                    session_id.to_owned(),
                    scratch_pad_range.clone(),
                    fs_file_path.to_owned(),
                    "\n```".to_owned(),
                    exchange_id.to_owned(),
                    None,
                )
                .set_apply_directly(),
            )
            .await;
        let _ = streamed_edit_client
            .send_edit_event(
                editor_url.to_owned(),
                EditedCodeStreamingRequest::end(
                    edit_request_id.to_owned(),
                    session_id.to_owned(),
                    scratch_pad_range.clone(),
                    fs_file_path.to_owned(),
                    exchange_id.to_owned(),
                    None,
                )
                .set_apply_directly(),
            )
            .await;
        Ok(ToolOutput::reasoning(ReasoningResponse {
            response: output.answer_up_until_now().to_owned(),
        }))
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
