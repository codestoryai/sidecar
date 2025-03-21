#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentSettings {
    /// If this is a midwit agent, midwit ==> 50% intelligence
    is_midwit: bool,
    /// Is it a json agent
    is_json: bool,
}

impl AgentSettings {
    pub fn new(is_midwit: bool, is_json: bool) -> Self {
        Self { is_midwit, is_json }
    }
    pub fn is_midwit(&self) -> bool {
        self.is_midwit
    }

    pub fn is_json(&self) -> bool {
        self.is_json
    }
}
