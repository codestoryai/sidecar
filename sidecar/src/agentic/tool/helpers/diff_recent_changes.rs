//! The diff recent changes which we have made and which are
//! more static than others, we can maintain a l1 and l2 cache
//! style changes, l1 is ONLY including the files which is being
//! edited and the l2 is the part which is long term and more static

use llm_client::clients::types::LLMClientMessage;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangeWeight {
    frequency: u32,
    importance: u8,
    semantic_relation: f32,
    user_interaction: u8,
}

impl ChangeWeight {
    pub fn new(
        frequency: u32,
        importance: u8,
        semantic_relation: f32,
        user_interaction: u8,
    ) -> Self {
        Self {
            frequency,
            importance,
            semantic_relation,
            user_interaction,
        }
    }

    pub fn calculate_score(&self) -> f32 {
        (self.frequency as f32 * 0.3)
            + (self.importance as f32 * 0.3)
            + (self.semantic_relation * 0.2)
            + (self.user_interaction as f32 * 0.2)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffFileContent {
    fs_file_path: String,
    file_content_latest: String,
    file_content_updated: Option<String>,
    change_weight: Option<ChangeWeight>,
    last_modified: i64,
    is_invalidated: bool,
}

impl DiffFileContent {
    pub fn new(
        fs_file_path: String,
        file_content_latest: String,
        file_content_updated: Option<String>,
        change_weight: Option<ChangeWeight>,
        last_modified: i64,
    ) -> Self {
        Self {
            fs_file_path,
            file_content_latest,
            file_content_updated,
            change_weight,
            last_modified,
            is_invalidated: false,
        }
    }

    pub fn invalidate(&mut self) {
        self.is_invalidated = true;
    }

    pub fn is_invalidated(&self) -> bool {
        self.is_invalidated
    }

    pub fn update_weight(&mut self, weight: ChangeWeight) {
        self.change_weight = Some(weight);
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn file_content_latest(&self) -> &str {
        &self.file_content_latest
    }
}

/// Contains the diff recent changes, with the caveat that the l1_changes are
/// the variable one and the l2_changes are the static one
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffRecentChanges {
    l1_changes: String,
    l2_changes: String,
    file_contents: Vec<DiffFileContent>,
    cache_threshold: f32,
}

impl DiffRecentChanges {
    pub fn new(
        l1_changes: String,
        l2_changes: String,
        file_contents: Vec<DiffFileContent>,
        cache_threshold: f32,
    ) -> Self {
        Self {
            l1_changes,
            l2_changes,
            file_contents,
            cache_threshold,
        }
    }

    pub fn file_contents(&self) -> &[DiffFileContent] {
        self.file_contents.as_slice()
    }

    pub fn l1_changes(&self) -> &str {
        &self.l1_changes
    }

    pub fn l2_changes(&self) -> &str {
        &self.l2_changes
    }

    pub fn should_promote_to_l1(&self, file_content: &DiffFileContent) -> bool {
        if let Some(weight) = &file_content.change_weight {
            weight.calculate_score() >= self.cache_threshold
        } else {
            false
        }
    }

    pub fn invalidate_cache_for_file(&mut self, file_path: &str) {
        if let Some(file_content) = self
            .file_contents
            .iter_mut()
            .find(|f| f.fs_file_path() == file_path)
        {
            file_content.invalidate();
        }
    }

    pub fn to_llm_client_message(&self) -> Vec<LLMClientMessage> {
        let l1_changes = self.l1_changes();
        let l2_changes = self.l2_changes();
        let first_part_message = format!(
            r#"
These are the git diff from the files which were recently edited sorted by the least recent to the most recent:
<diff_recent_changes>
{l2_changes}
"#
        );
        let second_part_message = format!(
            r#"{l1_changes}
</diff_recent_changes>
"#
        );
        vec![
            LLMClientMessage::user(first_part_message).cache_point(),
            LLMClientMessage::user(second_part_message),
        ]
    }
}
