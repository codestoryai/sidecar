use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::{stream, StreamExt};
use thiserror::Error;
use tokio::{io::AsyncWriteExt, sync::mpsc::UnboundedSender};

use crate::{
    agentic::{
        symbol::{
            errors::SymbolError,
            events::{
                edit::SymbolToEdit,
                lsp::LSPDiagnosticError,
                message_event::{SymbolEventMessage, SymbolEventMessageProperties},
            },
            identifier::SymbolIdentifier,
            manager::SymbolManager,
            tool_box::ToolBox,
            tool_properties::ToolProperties,
            types::SymbolEventRequest,
        },
        tool::{
            errors::ToolError, lsp::file_diagnostics::DiagnosticMap,
            session::chat::SessionChatMessage,
        },
    },
    chunking::text_document::Range,
    user_context::types::UserContext,
};

use super::{
    generator::StepSenderEvent,
    plan::Plan,
    plan_step::{PlanStep, StepExecutionContext},
};

/// Operates on Plan
#[derive(Clone)]
pub struct PlanService {
    tool_box: Arc<ToolBox>,
    symbol_manager: Arc<SymbolManager>,
    plan_storage_directory: PathBuf,
}

impl PlanService {
    pub fn new(
        tool_box: Arc<ToolBox>,
        symbol_manager: Arc<SymbolManager>,
        plan_storage_directory: PathBuf,
    ) -> Self {
        Self {
            tool_box,
            symbol_manager,
            plan_storage_directory,
        }
    }

    pub fn tool_box(&self) -> Arc<ToolBox> {
        self.tool_box.clone()
    }

    pub async fn save_plan(&self, plan: &Plan, path: &str) -> std::io::Result<()> {
        let serialized = serde_json::to_string(plan).unwrap();
        let mut file = tokio::fs::File::create(path).await?;
        file.write_all(serialized.as_bytes()).await?;
        Ok(())
    }

    pub async fn load_plan(&self, path: &str) -> std::io::Result<Plan> {
        let content = tokio::fs::read_to_string(path).await?;
        let plan: Plan = serde_json::from_str(&content).unwrap();
        Ok(plan)
    }

    /// also brings in associated files (requires go to reference)
    async fn process_diagnostics(
        &self,
        files_until_checkpoint: Vec<String>,
        message_properties: SymbolEventMessageProperties,
        with_enrichment: bool,
    ) -> Vec<LSPDiagnosticError> {
        let file_lsp_diagnostics = self
            .tool_box()
            .get_lsp_diagnostics_for_files(
                files_until_checkpoint,
                message_properties.clone(),
                with_enrichment,
            )
            .await
            .unwrap_or_default();

        file_lsp_diagnostics
    }

    /// Appends the step to the point after the checkpoint
    /// - diagnostics are included by default
    /// this does not depend on checkpoint anymore, as it is a pure append
    pub async fn generate_plan_steps_and_human_help(
        &self,
        mut plan: Plan,
        query: String,
        // we have to update the plan update context appropriately to the plan
        mut user_context: UserContext,
        message_properties: SymbolEventMessageProperties,
        is_deep_reasoning: bool,
        with_lsp_enrichment: bool,
    ) -> Result<(Plan, Option<String>), PlanServiceError> {
        // append to post checkpoint
        // - gather the plan until the checkpoint
        // - gather the git-diff we have until now
        // - the files which we are present we keep that in the context
        // - figure out the new steps which we want and insert them
        let formatted_plan = plan.format_to_string();

        let mut files_in_plan = plan.files_in_plan();
        // inclued files which are in the variables but not in the user context
        let file_path_in_variables = user_context
            .file_paths_from_variables()
            .into_iter()
            .filter(|file_path| {
                // filter out any file which we already have until the checkpoint
                !files_in_plan
                    .iter()
                    .any(|file_in_plan| file_in_plan == file_path)
            })
            .collect::<Vec<_>>();
        files_in_plan.extend(file_path_in_variables);
        let recent_edits = self
            .tool_box
            .recently_edited_files(
                files_in_plan.clone().into_iter().collect(),
                message_properties.clone(),
            )
            .await?;

        // get all diagnostics present on these files
        let file_lsp_diagnostics = self
            .process_diagnostics(
                files_in_plan,
                message_properties.clone(),
                with_lsp_enrichment,
            ) // this is the main diagnostics caller.
            .await;

        let diagnostics_grouped_by_file: DiagnosticMap =
            file_lsp_diagnostics
                .into_iter()
                .fold(HashMap::new(), |mut acc, error| {
                    acc.entry(error.fs_file_path().to_owned())
                        .or_insert_with(Vec::new)
                        .push(error);
                    acc
                });

        // now we try to enrich the context even more if we can by expanding our search space
        // and grabbing some more context
        if with_lsp_enrichment {
            println!("plan_service::lsp_dignostics::enriching_context_using_tree_sitter");
            for (fs_file_path, lsp_diagnostics) in diagnostics_grouped_by_file.iter() {
                let extra_variables = self
                    .tool_box
                    .grab_type_definition_worthy_positions_using_diagnostics(
                        fs_file_path,
                        lsp_diagnostics.to_vec(),
                        message_properties.clone(),
                    )
                    .await;
                if let Ok(extra_variables) = extra_variables {
                    user_context = user_context.add_variables(extra_variables);
                }
            }
            println!(
                "plan_service::lsp_diagnostics::enriching_context_using_tree_sitter::finished"
            );
        }

        // update the user context with the one from current run
        plan = plan.combine_user_context(user_context);

        let _formatted_diagnostics = Self::format_diagnostics(&diagnostics_grouped_by_file);

        let (mut new_steps, human_help) = self
            .tool_box
            .generate_new_steps_and_user_help_for_plan(
                formatted_plan,
                plan.initial_user_query().to_owned(),
                query,
                plan.user_context().clone(),
                recent_edits,
                message_properties,
                is_deep_reasoning,
            )
            .await?;

        // After generating new_steps
        for step in &mut new_steps {
            step.set_user_context(plan.user_context().clone());
        }

        plan.add_steps_vec(new_steps);
        // let _ = self.save_plan(&plan, plan.storage_path()).await;
        // we want to get the new plan over here and insert it properly
        Ok((plan, human_help))
    }

    /// Appends the step to the point after the checkpoint
    /// - diagnostics are included by default
    /// this does not depend on checkpoint anymore, as it is a pure append
    pub async fn append_steps(
        &self,
        mut plan: Plan,
        query: String,
        // we have to update the plan update context appropriately to the plan
        mut user_context: UserContext,
        message_properties: SymbolEventMessageProperties,
        is_deep_reasoning: bool,
        with_lsp_enrichment: bool,
    ) -> Result<Plan, PlanServiceError> {
        // append to post checkpoint
        // - gather the plan until the checkpoint
        // - gather the git-diff we have until now
        // - the files which we are present we keep that in the context
        // - figure out the new steps which we want and insert them
        let formatted_plan = plan.format_to_string();

        let mut files_in_plan = plan.files_in_plan();
        // inclued files which are in the variables but not in the user context
        let file_path_in_variables = user_context
            .file_paths_from_variables()
            .into_iter()
            .filter(|file_path| {
                // filter out any file which we already have until the checkpoint
                !files_in_plan
                    .iter()
                    .any(|file_in_plan| file_in_plan == file_path)
            })
            .collect::<Vec<_>>();
        files_in_plan.extend(file_path_in_variables);
        let recent_edits = self
            .tool_box
            .recently_edited_files(
                files_in_plan.clone().into_iter().collect(),
                message_properties.clone(),
            )
            .await?;

        // get all diagnostics present on these files
        let file_lsp_diagnostics = self
            .process_diagnostics(
                files_in_plan,
                message_properties.clone(),
                with_lsp_enrichment,
            ) // this is the main diagnostics caller.
            .await;

        let diagnostics_grouped_by_file: DiagnosticMap =
            file_lsp_diagnostics
                .into_iter()
                .fold(HashMap::new(), |mut acc, error| {
                    acc.entry(error.fs_file_path().to_owned())
                        .or_insert_with(Vec::new)
                        .push(error);
                    acc
                });

        // now we try to enrich the context even more if we can by expanding our search space
        // and grabbing some more context
        if with_lsp_enrichment {
            println!("plan_service::lsp_dignostics::enriching_context_using_tree_sitter");
            for (fs_file_path, lsp_diagnostics) in diagnostics_grouped_by_file.iter() {
                let extra_variables = self
                    .tool_box
                    .grab_type_definition_worthy_positions_using_diagnostics(
                        fs_file_path,
                        lsp_diagnostics.to_vec(),
                        message_properties.clone(),
                    )
                    .await;
                if let Ok(extra_variables) = extra_variables {
                    user_context = user_context.add_variables(extra_variables);
                }
            }
            println!(
                "plan_service::lsp_diagnostics::enriching_context_using_tree_sitter::finished"
            );
        }

        // update the user context with the one from current run
        plan = plan.combine_user_context(user_context);

        let formatted_diagnostics = Self::format_diagnostics(&diagnostics_grouped_by_file);

        let mut new_steps = self
            .tool_box
            .generate_new_steps_for_plan(
                formatted_plan,
                plan.initial_user_query().to_owned(),
                query,
                plan.user_context().clone(),
                recent_edits,
                message_properties,
                is_deep_reasoning,
                formatted_diagnostics,
            )
            .await?;

        // After generating new_steps
        for step in &mut new_steps {
            step.set_user_context(plan.user_context().clone());
        }

        plan.add_steps_vec(new_steps);
        let _ = self.save_plan(&plan, plan.storage_path()).await;
        // we want to get the new plan over here and insert it properly
        Ok(plan)
    }

    pub fn format_diagnostics(diagnostics: &DiagnosticMap) -> String {
        if diagnostics.is_empty() {
            return r#"I did not see errors, it could be that the Language Server is not configured or I found no mistakes, you can ask me to look elsewhere if you think I can grab the diagnostics from somewhere else"#.to_owned();
        }
        diagnostics
            .iter()
            .map(|(file, errors)| {
                let formatted_errors = errors
                    .iter()
                    .enumerate()
                    .map(|(index, error)| {
                        format!(
                            r#"#{}:
---
### Snippet:
{}
### Diagnostic:
{}
### Files Affected:
{}
### Quick fixes:
{}
### Parameter hints:
{}
### Additional symbol outlines:
{}
---
"#,
                            index + 1,
                            error.snippet(),
                            error.diagnostic_message(),
                            error.associated_files().map_or_else(
                                || String::from("Only this file."),
                                |files| files.join(", ")
                            ),
                            error
                                .quick_fix_labels()
                                .as_ref()
                                .map_or_else(|| String::from("None"), |labels| labels.join("\n")),
                            error
                                .parameter_hints()
                                .as_ref()
                                .map_or_else(|| String::from("None"), |labels| labels.join("\n")),
                            error.user_variables().map_or_else(
                                || String::from("None"),
                                |user_variables| { user_variables.join("\n") },
                            )
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                format!("File: {}\n{}", file, formatted_errors)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub async fn create_plan(
        &self,
        plan_id: String,
        query: String,
        previous_queries: Vec<String>,
        mut user_context: UserContext,
        aide_rules: Option<String>,
        previous_messages: Vec<SessionChatMessage>,
        is_deep_reasoning: bool,
        plan_storage_path: String,
        step_sender: Option<UnboundedSender<StepSenderEvent>>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Plan, PlanServiceError> {
        println!("plan::service::deep_reasoning::({})", is_deep_reasoning);
        if true {
            println!("gathering::deep_context");
            user_context = self
                .tool_box
                .generate_deep_user_context(user_context.clone(), message_properties.clone())
                .await
                .unwrap_or(user_context);
        }
        let mut plan_steps = self
            .tool_box
            .generate_plan(
                &query,
                previous_queries,
                &user_context,
                aide_rules,
                previous_messages,
                is_deep_reasoning,
                step_sender,
                message_properties,
            )
            .await?;

        // After generating plan_steps
        for step in &mut plan_steps {
            step.set_user_context(user_context.clone()); // every step gets given a clone of user_context
        }

        let plan = Plan::new(
            plan_id.to_owned(),
            "Placeholder Title (to be computed)".to_owned(),
            user_context,
            query,
            plan_steps,
            plan_storage_path.to_owned(),
        );
        self.save_plan(&plan, &plan_storage_path).await?;
        Ok(plan)
    }

    /// gets all files_to_edit from PlanSteps up to index
    pub fn get_edited_files(&self, plan: &Plan, index: usize) -> Vec<String> {
        plan.steps()[..index]
            .iter()
            .filter_map(|step| step.file_to_edit())
            .collect::<Vec<_>>()
    }

    pub fn step_execution_context(
        &self,
        steps: &[PlanStep],
        index: usize,
    ) -> Vec<StepExecutionContext> {
        let steps_to_now = &steps[..index];

        let context_to_now = steps_to_now
            .iter()
            .map(|step| StepExecutionContext::from(step))
            .collect::<Vec<_>>();

        context_to_now
    }

    pub async fn prepare_context(&self, steps: &[PlanStep], checkpoint: usize) -> String {
        let contexts = self.step_execution_context(steps, checkpoint);
        // todo(zi) consider accumulating this in a context manager vs recomputing for each step (long)
        let full_context_as_string = stream::iter(contexts.to_vec().into_iter().enumerate().map(
            |(index, context)| async move {
                let context_string = context.to_string().await;
                format!("Step {}:\n{}", index + 1, context_string)
            },
        ))
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await
        .join("\n");

        full_context_as_string
    }

    pub async fn execute_step(
        &self,
        step: &PlanStep,
        checkpoint: usize,
        context: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), PlanServiceError> {
        let instruction = step.description();
        let fs_file_path = match step.file_to_edit() {
            Some(path) => path,
            None => {
                return Err(PlanServiceError::AbsentFilePath(
                    "No file path provided for editing".to_string(),
                ))
            }
        };

        let hub_sender = self.symbol_manager.hub_sender();
        let (ui_sender, _ui_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (edit_done_sender, edit_done_receiver) = tokio::sync::oneshot::channel();
        let _ = hub_sender.send(SymbolEventMessage::new(
            SymbolEventRequest::simple_edit_request(
                SymbolIdentifier::with_file_path(&fs_file_path, &fs_file_path),
                SymbolToEdit::new(
                    fs_file_path.to_owned(),
                    Range::default(),
                    fs_file_path.to_owned(),
                    vec![instruction.to_owned()],
                    false,
                    false,
                    true,
                    instruction.to_owned(),
                    None,
                    false,
                    Some(context),
                    true,
                    None,
                    vec![],
                    Some(checkpoint.to_string()),
                ),
                ToolProperties::new(),
            ),
            message_properties.request_id().clone(),
            ui_sender,
            edit_done_sender,
            tokio_util::sync::CancellationToken::new(),
            message_properties.editor_url(),
            message_properties.llm_properties().clone(),
        ));

        // await on the edit to finish happening
        let _ = edit_done_receiver.await;

        Ok(())
    }

    /// Marks the plan as complete over here
    pub async fn mark_plan_completed(&self, mut plan: Plan) {
        let step_count = plan.step_count();
        if step_count == 0 {
            plan.set_checkpoint(0);
        } else {
            plan.set_checkpoint(step_count - 1);
        }
        let _ = self.save_plan(&plan, plan.storage_path()).await;
    }

    pub fn generate_unique_plan_id(&self, session_id: &str, exchange_id: &str) -> String {
        format!("{}-{}", session_id, exchange_id)
    }

    pub async fn load_plan_from_id(&self, plan_id: &str) -> std::io::Result<Plan> {
        let mut plan_storage_path = self.plan_storage_directory.clone();
        plan_storage_path = plan_storage_path.join(plan_id);
        let content = tokio::fs::read_to_string(plan_storage_path).await?;
        let plan: Plan = serde_json::from_str(&content).unwrap();
        Ok(plan)
    }
}

#[derive(Debug, Error)]
pub enum PlanServiceError {
    #[error("Tool Error: {0}")]
    ToolError(#[from] ToolError),

    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Tool Error: {0}")]
    SymbolError(#[from] SymbolError),

    #[error("Wrong tool output")]
    WrongToolOutput(),

    #[error("Step not found: {0}")]
    StepNotFound(usize),

    #[error("Absent file path: {0}")]
    AbsentFilePath(String),

    #[error("Invalid step execution request: {0}")]
    InvalidStepExecution(usize),
}
