//! The code editor tool over here contains the handler for the code edit tool
//! We will implement this on our own without the tool coming into the picture

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EditorCommand {
    View,
    Create,
    StrReplace,
    Insert,
    UndoEdit,
}

impl std::fmt::Display for EditorCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::View => write!(f, "view"),
            Self::Create => write!(f, "create"),
            Self::StrReplace => write!(f, "str_replace"),
            Self::Insert => write!(f, "insert"),
            Self::UndoEdit => write!(f, "undo_edit"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CodeEditorParameters {
    pub command: EditorCommand,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_str: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_str: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_range: Option<Vec<i32>>,
}

impl CodeEditorParameters {
    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "str_replace_editor",
            "type": "text_editor_20241022",
        })
    }

    pub fn to_string(&self) -> String {
        let command = self.command.clone().to_string();
        let path = self.path.to_owned();
        let mut remaining_parts = vec![];
        if let Some(file_text) = self.file_text.clone() {
            remaining_parts.push(format!(
                r#"<file_text>
{}
</file_text>"#,
                file_text
            ));
        }
        if let Some(insert_line) = self.insert_line.clone() {
            remaining_parts.push(format!(
                r#"<insert_line>
{insert_line}
</insert_line>"#
            ));
        }
        if let Some(old_str) = self.old_str.clone() {
            remaining_parts.push(format!(
                r#"<old_str>
{old_str}
</old_str>"#
            ));
        }
        if let Some(new_str) = self.new_str.clone() {
            remaining_parts.push(format!(
                r#"<new_str>
{new_str}
</new_str>"#
            ));
        }
        if let Some(view_range) = self.view_range.clone() {
            remaining_parts.push(format!(
                r#"<view_range>
{}
</view_range>"#,
                view_range
                    .into_iter()
                    .map(|view_range| view_range.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        }
        let remainig_parts_str = remaining_parts.join("\n");
        format!(
            r#"<thinking>
...
</thinking>
<str_replace_editor>
<command>
{command}
</command>
<path>
{path}
</path>
{remainig_parts_str}
</str_replace_editor>"#
        )
    }
}
