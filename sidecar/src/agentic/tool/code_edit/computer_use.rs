use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerUseRequest {
    pub operation: String,
    pub content: Option<String>,
    pub target_path: Option<String>,
}

impl ComputerUseRequest {
    pub fn to_string(&self) -> String {
        let mut parts = vec![format!("Operation: {}", self.operation)];

        if let Some(content) = &self.content {
            parts.push(format!("Content: {}", content));
        }

        if let Some(path) = &self.target_path {
            parts.push(format!("Target Path: {}", path));
        }

        parts.join("\n")
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "computer_use",
            "type": "computer_use_20240116",
        })
    }
}
