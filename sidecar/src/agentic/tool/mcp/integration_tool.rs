use async_trait::async_trait;
use mcp_client_rs::client::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale, ToolType},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTools {
    pub server_name: String,
    pub tools: Vec<ToolDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolListResponse {
    pub servers: Vec<ServerTools>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericToolCallResponse {
    pub result: Value,
    // value is string or JSON ?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MCPIntegrationToolResponse {
    ToolList(ToolListResponse),
    ToolCall(GenericToolCallResponse),
}

/// example, if the server "notes_server" has a tool
/// "add_note", the broker will store
///    ToolType::DynamicMCPTool("add_note")
/// -> DynamicMCPTool { server_name: "notes_server", tool_name: "add_note", ... }
pub struct DynamicMCPTool {
    server_name: String,
    tool_name: String,
    description: String,
    schema: Value,
    client: Arc<Client>,
    // client is Arc because we want to share it across multiple tools for the same server
}

impl DynamicMCPTool {
    pub fn new(
        server_name: String,
        tool_name: String,
        description: String,
        schema: Value,
        client: Arc<Client>,
    ) -> Self {
        Self {
            server_name,
            tool_name,
            description,
            schema,
            client,
        }
    }
}

/// Generate usage from the server’s JSON schema
fn generate_schema_usage(tool_name: &str, schema: &Value) -> String {
    let mut usage = String::new();
    usage.push_str("Parameters:\n");

    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .unwrap();
    let required_fields = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_owned())
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();

    for (field_name, data) in props {
        let desc = data
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let tpe = data
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("string");
        let is_required = required_fields.contains(field_name);
        usage.push_str(&format!(
            "- {field_name}: ({}) {desc}, type={tpe}\n",
            if is_required { "required" } else { "optional" }
        ));
    }

    usage.push_str("\nUsage:\n");
    usage.push_str(&format!("<{tool_name}>\n"));
    for field in props.keys() {
        usage.push_str(&format!("<{field}>\nvalue\n</{field}>\n"));
    }
    usage.push_str(&format!("</{tool_name}>\n"));

    usage
}

#[async_trait]
impl Tool for DynamicMCPTool {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        // We rely on the new variant
        //   ToolInput::DynamicMCPTool(DynamicMCPToolPartial { tool_name, fields })
        // so let's parse that:

        let partial = match input {
            ToolInput::DynamicMCPTool(p) => p,
            _ => {
                return Err(ToolError::WrongToolInput(ToolType::DynamicMCPTool(
                    self.tool_name.clone(),
                )))
            }
        };

        // Check for mismatch:
        if partial.tool_name != self.tool_name {
            return Err(ToolError::InvalidInput(format!(
                "DynamicMCPTool mismatch: local tool='{}' but user partial='{}'",
                self.tool_name, partial.tool_name
            )));
        }

        // Convert partial.fields -> a JSON object to pass to call_tool
        let mut json_map = serde_json::Map::new();
        for (k, v) in partial.fields.iter() {
            json_map.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        let arguments = serde_json::Value::Object(json_map);

        // Perform the call
        let result = self
            .client
            .call_tool(&self.tool_name, arguments)
            .await
            .map_err(|e| {
                ToolError::InvocationError(format!(
                    "Failed calling dynamic tool '{}' on server '{}': {}",
                    self.tool_name, self.server_name, e
                ))
            })?;

        let value = serde_json::to_value(result).map_err(|e| {
            ToolError::InvocationError(format!("Serialize dynamic tool result failed: {}", e))
        })?;

        // Return as typical
        Ok(ToolOutput::MCPIntegration(
            MCPIntegrationToolResponse::ToolCall(GenericToolCallResponse { result: value }),
        ))
    }

    fn tool_description(&self) -> String {
        // Appear just like a normal built-in, but behind the scenes it's from an MCP server
        format!(
            "### {}\n(mcp server={})\n{}",
            self.tool_name, self.server_name, self.description
        )
    }

    fn tool_input_format(&self) -> String {
        generate_schema_usage(&self.tool_name, &self.schema)
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::tool::{
        input::{DynamicMCPToolPartial, ToolInput},
        lsp::open_file::OpenFileRequest,
    };
    use mcp_client_rs::client::ClientBuilder;
    use tokio;

    async fn setup_test_client() -> anyhow::Result<Arc<Client>> {
        let builder = ClientBuilder::new("uvx")
            .arg("notes-simple");

        let client = builder.spawn_and_initialize().await?;
        Ok(Arc::new(client))
    }

    #[tokio::test]
    async fn test_dynamic_mcp_tool_creation() -> anyhow::Result<()> {
        let client = setup_test_client().await?;

        // List available tools
        let list_res = client.list_tools().await?;
        assert!(
            !list_res.tools.is_empty(),
            "Server should have at least one tool"
        );

        // Create a DynamicMCPTool for each tool
        for tool_info in list_res.tools {
            let dyn_tool = DynamicMCPTool::new(
                "notes_simple".to_string(),
                tool_info.name.clone(),
                tool_info.description.clone(),
                tool_info.input_schema.clone(),
                Arc::clone(&client),
            );

            // Test tool description and input format
            let desc = dyn_tool.tool_description();
            assert!(!desc.is_empty(), "Tool description should not be empty");

            let input_format = dyn_tool.tool_input_format();
            assert!(
                !input_format.is_empty(),
                "Tool input format should not be empty"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_dynamic_mcp_tool_invocation() -> anyhow::Result<()> {
        let client = setup_test_client().await?;

        // Create a test note using the add_note tool
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "Test Note".to_string());
        fields.insert("content".to_string(), "This is a test note".to_string());

        let dyn_tool = DynamicMCPTool::new(
            "notes_simple".to_string(),
            "add-note".to_string(),
            "Add a new note".to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["name", "content"]
            }),
            Arc::clone(&client),
        );

        let input = ToolInput::DynamicMCPTool(DynamicMCPToolPartial {
            tool_name: "add-note".to_string(),
            fields,
        });

        let result = dyn_tool.invoke(input).await?;

        // Verify the response
        match result {
            ToolOutput::MCPIntegration(MCPIntegrationToolResponse::ToolCall(response)) => {
                assert!(
                    response.result.is_object(),
                    "Response should be a JSON object"
                );
            }
            _ => panic!("Unexpected response type"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_dynamic_mcp_tool_errors() -> anyhow::Result<()> {
        let client = setup_test_client().await?;

        let dyn_tool = DynamicMCPTool::new(
            "notes_simple".to_string(),
            "add-note".to_string(),
            "Add a new note".to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["name", "content"]
            }),
            Arc::clone(&client),
        );

        // Test wrong tool input type
        let wrong_input = ToolInput::OpenFile(OpenFileRequest::new(
            "test.txt".to_string(),
            "http://localhost".to_string(),
        ));
        let result = dyn_tool.invoke(wrong_input).await;
        assert!(matches!(result, Err(ToolError::WrongToolInput(_))));

        // Test missing required field
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "Test Note".to_string());
        // Missing content field

        let input = ToolInput::DynamicMCPTool(DynamicMCPToolPartial {
            tool_name: "add_note".to_string(),
            fields,
        });

        let result = dyn_tool.invoke(input).await;
        assert!(result.is_err(), "Should fail with missing required field");

        Ok(())
    }
}
