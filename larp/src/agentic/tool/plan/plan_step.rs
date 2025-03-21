use serde::{Deserialize, Serialize};

use crate::user_context::types::UserContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    id: String,
    title: String,
    files_to_edit: Vec<String>, // paths of files that step may execute against
    description: String,        // we want to keep the step's edit as deterministic as possible
    user_context: UserContext,  // Store the current user context
}

impl PlanStep {
    pub fn new(
        id: String,
        files_to_edit: Vec<String>,
        title: String,
        description: String,
        user_context: UserContext,
    ) -> Self {
        Self {
            id,
            title,
            files_to_edit,
            description,
            user_context,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn edit_title(&mut self, new_title: String) {
        self.title = new_title;
    }

    pub fn id(&self) -> String {
        self.id.to_owned()
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn edit_description(&mut self, new_description: String) {
        self.description = new_description;
    }

    pub fn user_context(&self) -> &UserContext {
        &self.user_context
    }

    pub fn set_user_context(&mut self, user_context: UserContext) {
        self.user_context = user_context;
    }

    pub fn files_to_edit(&self) -> &[String] {
        &self.files_to_edit.as_slice()
    }

    /// Returns first file in Vec. Temporary measure until we decide whether files_to_edit should be an vec.
    pub fn file_to_edit(&self) -> Option<String> {
        self.files_to_edit.first().map(|s| s.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct StepExecutionContext {
    description: String,
    user_context: UserContext,
}

impl StepExecutionContext {
    pub fn new(description: String, user_context: UserContext) -> Self {
        Self {
            description,
            user_context,
        }
    }

    pub fn from_plan_step(plan_step: &PlanStep) -> Self {
        Self {
            description: plan_step.description.clone(),
            user_context: plan_step.user_context.clone(),
        }
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn user_context(&self) -> &UserContext {
        &self.user_context
    }

    pub fn update_description(&mut self, new_description: String) {
        self.description = new_description;
    }

    pub fn update_user_context(&mut self, new_user_context: UserContext) {
        self.user_context = new_user_context;
    }

    pub async fn to_string(&self) -> String {
        let context_string = self
            .user_context
            .to_context_string()
            .await
            .unwrap_or("".to_owned());

        format!(
            r#"Description: {}

User Context: {}"#,
            self.description, context_string
        )
    }
}

impl From<&PlanStep> for StepExecutionContext {
    fn from(plan_step: &PlanStep) -> Self {
        Self::from_plan_step(plan_step)
    }
}
