use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename = "response", default)]
pub struct DecideResponse {
    #[serde(default)]
    suggestions: String,
    complete: bool,
}

impl DecideResponse {
    pub fn suggestions(&self) -> &str {
        &self.suggestions
    }

    pub fn complete(&self) -> bool {
        self.complete
    }
}
