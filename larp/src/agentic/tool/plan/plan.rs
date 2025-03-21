use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{agentic::tool::lsp::open_file::OpenFileResponse, user_context::types::UserContext};

use super::plan_step::PlanStep;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    id: String,
    name: String, // for UI label
    steps: Vec<PlanStep>,
    user_context: UserContext,
    user_query: String, // this may only be useful for initial plan generation. Steps better represent the overall direction?
    checkpoint: Option<usize>,
    storage_path: String,
    original_file_content: HashMap<String, OpenFileResponse>,
}

impl Plan {
    pub fn new(
        id: String,
        name: String,
        user_context: UserContext,
        user_query: String,
        steps: Vec<PlanStep>,
        storage_path: String,
    ) -> Self {
        Self {
            id,
            name,
            user_context,
            steps,
            user_query,
            checkpoint: None,
            storage_path,
            original_file_content: Default::default(),
        }
    }

    /// Drops the steps which are present in the plan until a point
    pub fn drop_plan_steps(mut self, drop_from: usize) -> Self {
        if drop_from < self.steps.len() {
            self.steps.truncate(drop_from);
            if let Some(checkpoint) = self.checkpoint {
                self.checkpoint = Some(checkpoint.min(drop_from));
            }
        }
        self
    }

    pub fn storage_path(&self) -> &str {
        &self.storage_path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn with_user_context(mut self, user_context: UserContext) -> Self {
        self.user_context = user_context;
        self
    }

    pub fn user_context(&self) -> &UserContext {
        &self.user_context
    }

    pub fn initial_user_query(&self) -> &str {
        &self.user_query
    }

    pub fn add_step(&mut self, mut step: PlanStep) {
        step.set_user_context(self.user_context.clone());
        self.steps.push(step);
    }

    pub fn add_steps(&mut self, steps: &[PlanStep]) {
        self.steps.extend(steps.to_vec())
    }

    pub fn add_steps_vec(&mut self, mut steps: Vec<PlanStep>) {
        for step in &mut steps {
            step.set_user_context(self.user_context.clone());
        }
        self.steps.extend(steps);
    }

    pub fn edit_step(&mut self, step_id: String, new_content: String) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id() == step_id) {
            step.edit_description(new_content);
        }
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps.as_slice()
    }

    pub fn steps_mut(&mut self) -> &mut Vec<PlanStep> {
        &mut self.steps
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn checkpoint(&self) -> Option<usize> {
        self.checkpoint
    }

    pub fn increment_checkpoint(&mut self) {
        match self.checkpoint {
            None => {
                self.checkpoint = Some(0);
            }
            Some(value) => {
                self.checkpoint = Some(value + 1);
            }
        }
    }

    pub fn set_checkpoint(&mut self, index: usize) {
        self.checkpoint = Some(index);
    }

    pub fn final_checkpoint(&self) -> usize {
        &self.steps.len() - 1
    }

    /// Combine current user context with the additional user context
    pub fn combine_user_context(mut self, user_context: UserContext) -> Self {
        self.user_context = self.user_context.add_variables(user_context.variables);
        self
    }

    pub fn format_to_string(&self) -> String {
        let plan_steps = self
            .steps
            .iter()
            .enumerate()
            .map(|(idx, step)| {
                let index = idx + 1;
                let title = step.title();
                let description = step.description();
                format!(
                    r#"Plan step: {index}
### Title
{title}
### Description
{description}"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let user_query = self.user_query();
        format!(
            r#"Initial user query: {user_query}
Plan up until now:
{plan_steps}"#
        )
    }

    pub fn plan_until_point(&self, checkpoint: usize) -> String {
        let plan_steps = self
            .steps
            .iter()
            .enumerate()
            .filter(|(idx, _step)| *idx <= checkpoint)
            .map(|(idx, step)| {
                let index = idx + 1;
                let title = step.title();
                let description = step.description();
                format!(
                    r#"Plan step: {index}
### Title
{title}
### Description
{description}"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let user_query = self.user_query();
        format!(
            r#"Initial user query: {user_query}
Plan up until now:
{plan_steps}"#
        )
    }

    /// The files which we have used to generate the context up until now
    pub fn files_in_plan(&self) -> Vec<String> {
        let mut files_in_context = vec![];
        let mut files_already_seen: HashSet<String> = Default::default();
        self.steps.iter().enumerate().for_each(|(_, step)| {
            step.files_to_edit().into_iter().for_each(|file_path| {
                if files_already_seen.contains(file_path.as_str()) {
                    return;
                }
                files_already_seen.insert(file_path.to_owned());
                files_in_context.push(file_path.to_owned());
            })
        });
        files_in_context
    }

    /// Tracks the original file content so we can know what all has changed
    ///
    /// This allows us to find outline nodes which have changed and then generate
    /// go-to-reference potential options for it
    pub fn track_original_file(
        &mut self,
        fs_file_path: String,
        file_open_response: OpenFileResponse,
    ) {
        if !self.original_file_content.contains_key(&fs_file_path) {
            self.original_file_content
                .insert(fs_file_path, file_open_response);
        }
    }

    pub fn to_debug_message(&self) -> String {
        self.steps
            .iter()
            .enumerate()
            .map(|(idx, step)| {
                let step_title = step.title();
                let step_description = step.description();
                let files_to_edit = step
                    .files_to_edit()
                    .into_iter()
                    .enumerate()
                    .map(|(idx, files_to_edit)| format!("{} - {}", idx + 1, files_to_edit))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "## Plan step {idx}:
### Title
{step_title}
### Description
{step_description}
### Files to edit
{files_to_edit}"
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
