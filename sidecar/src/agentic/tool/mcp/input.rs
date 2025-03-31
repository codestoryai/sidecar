use xmltree::Element;
use crate::agentic::tool::errors::ToolError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpToolPartial {
    /// The normalized name including mcp prefix, e.g. "mcp::notes_server::add_note"
    pub full_name: String,
    /// A JSON object from the LLM
    pub json: serde_json::Map<String, serde_json::Value>,
    /// Optional XML input string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xml_input: Option<String>,
}

impl McpToolPartial {
    pub fn to_string(&self) -> String {
        if let Some(xml) = &self.xml_input {
            format!(
                r#"<{}>
{}
</{}>"#,
                self.full_name, xml, self.full_name
            )
        } else {
            format!(
                r#"<{}>
{}
</{}>"#,
                self.full_name,
                serde_json::to_string_pretty(&self.json).unwrap_or_default(),
                self.full_name
            )
        }
    }

    pub fn parse(full_name: &str, input_str: &str) -> Result<Self, ToolError> {
        // Try parsing as XML first
        if let Ok(root) = Element::parse(input_str.as_bytes()) {
            return Ok(Self {
                full_name: full_name.to_string(),
                json: serde_json::Map::new(),
                xml_input: Some(input_str.to_string()),
            });
        }

        // If XML parsing fails, try JSON
        match serde_json::from_str(input_str) {
            Ok(json) => Ok(Self {
                full_name: full_name.to_string(),
                json,
                xml_input: None,
            }),
            Err(e) => Err(ToolError::InvalidInput(format!(
                "Failed to parse input as either XML or JSON: {}",
                e
            ))),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct McpToolInput {
    pub partial: McpToolPartial,
}