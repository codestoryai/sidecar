#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpToolPartial {
    /// The normalized name including mcp prefix, e.g. "mcp::notes_server::add_note"
    pub full_name: String,
    /// A JSON object from the LLM
    pub json: serde_json::Map<String, serde_json::Value>,
}

impl McpToolPartial {
    pub fn to_string(&self) -> String {
        format!(
            r#"<{}>
{}
</{}>"#,
            self.full_name,
            serde_json::to_string_pretty(&self.json).unwrap_or_default(),
            self.full_name
        )
    }

    pub fn parse(full_name: &str, json_str: &str) -> Result<Self, serde_json::Error> {
        let json: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json_str)?;
        let full_name = full_name.to_string();
        Ok(Self { full_name, json })
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct McpToolInput {
    pub partial: McpToolPartial,
}
