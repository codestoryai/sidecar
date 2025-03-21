use async_trait::async_trait;
use mcp_client_rs::{client::Client, MessageContent, ResourceContents};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

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
pub struct McpToolResponse {
    pub data: String,
}

/// example, if the server "notes_server" has a tool
/// "add-note", the broker will store
///    ToolType::McpTool("mcp::notes_server::add_note")
/// -> McpTool { full_name: "mcp::notes_server::add_note", server_name: "notes_server", original_name: "add-note", ... }
pub struct McpTool {
    pub full_name: String,
    server_name: String,
    original_tool_name: String,
    description: String,
    schema: Value,
    client: Arc<Client>,
    // client is Arc because we want to share it across multiple tools for the same server
}

impl McpTool {
    pub fn new(
        server_name: String,
        tool_name: String,
        description: String,
        schema: Value,
        client: Arc<Client>,
    ) -> Self {
        // full name creation:
        // * mcp:: prefix
        // * lower case names, and replace hyphens with underscore
        // * join server name and tool name with ::

        let server_name_normalized = server_name.to_lowercase().replace('-', "_");
        let tool_name_normalized = tool_name.to_lowercase().replace('-', "_");
        let full_name = format!("mcp::{}::{}", server_name_normalized, tool_name_normalized);

        Self {
            full_name,
            server_name,
            original_tool_name: tool_name,
            description,
            schema,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let input = match input {
            ToolInput::McpTool(p) => p,
            _ => {
                return Err(ToolError::WrongToolInput(ToolType::McpTool(
                    self.full_name.clone(),
                )))
            }
        };

        // Check for mismatch:
        if input.partial.full_name != self.full_name {
            return Err(ToolError::InvalidInput(format!(
                "McpTool mismatch: local tool='{}' but input='{}'",
                self.full_name, input.partial.full_name
            )));
        }

        let arguments = Value::Object(input.partial.json);

        // Perform the call
        let result = self
            .client
            .call_tool(&self.original_tool_name, arguments)
            .await
            .map_err(|e| {
                ToolError::InvocationError(format!(
                    "Failed calling dynamic tool '{}' on server '{}': {}",
                    self.full_name, self.server_name, e
                ))
            })?;

        // for now we only handle responses of type "text"
        let content = result
            .content
            .into_iter()
            .map(|content| match content {
                MessageContent::Text { text } => text,
                MessageContent::Image { uri, alt_text: _ } => format!("image at {}", uri),
                MessageContent::Resource { resource } => match resource.contents {
                    ResourceContents::Text { text, .. } => text,
                    _ => "unsupported MCP blob embedded resource".to_string(),
                },
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Return as typical
        Ok(ToolOutput::McpTool(McpToolResponse { data: content }))
    }

    fn tool_description(&self) -> String {
        format!("### {}\n{}", self.full_name, self.description)
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
Provide all parameters as a JSON object. This object will be passed on to an MCP server, so it must be valid JSON, and it must match the schema below. This is the schema for the tool:

{}

Usage:

For example, if the schema above included a "city" parameter, the tool might be called like this:

<{}>
{{
  "city": "San Francisco"
}}
</{}>"#,
            serde_json::to_string_pretty(&self.schema).unwrap_or_else(|_| self.schema.to_string()),
            self.full_name,
            self.full_name
        )
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
        input::ToolInput, lsp::open_file::OpenFileRequest, mcp::input::McpToolInput,
        mcp::input::McpToolPartial,
    };
    use lazy_static::lazy_static;
    use mcp_client_rs::client::ClientBuilder;
    use tokio;

    lazy_static! {
        static ref CONVERT_TIME_SCHEMA: serde_json::Value = serde_json::json!({
            "type": "object",
            "properties": {
                "source_timezone": {
                    "type": "string",
                    "description": "Source IANA timezone name (e.g., 'America/New_York', 'Europe/London'). Use '{local_tz}' as local timezone if no source timezone provided by the user.",
                },
                "time": {
                    "type": "string",
                    "description": "Time to convert in 24-hour format (HH:MM)",
                },
                "target_timezone": {
                    "type": "string",
                    "description": "Target IANA timezone name (e.g., 'Asia/Tokyo', 'America/San_Francisco'). Use '{local_tz}' as local timezone if no target timezone provided by the user.",
                },
            },
            "required": ["source_timezone", "time", "target_timezone"],
        });
    }

    async fn setup_test_client() -> anyhow::Result<Arc<Client>> {
        let builder = ClientBuilder::new("uvx").arg("mcp-server-time");

        let client = builder.spawn_and_initialize().await?;
        Ok(Arc::new(client))
    }

    #[tokio::test]
    async fn test_mcp_tool_creation() -> anyhow::Result<()> {
        let client = setup_test_client().await?;

        // List available tools
        let list_res = client.list_tools().await?;
        assert!(
            !list_res.tools.is_empty(),
            "Server should have at least one tool"
        );

        // Create McpTool for each tool
        for tool_info in list_res.tools {
            let mcp_tool = McpTool::new(
                "time".to_string(),
                tool_info.name.clone(),
                tool_info.description.clone(),
                tool_info.input_schema.clone(),
                Arc::clone(&client),
            );

            // Test tool description and input format
            let desc = mcp_tool.tool_description();
            assert!(!desc.is_empty(), "Tool description should not be empty");

            let input_format = mcp_tool.tool_input_format();
            assert!(
                !input_format.is_empty(),
                "Tool input format should not be empty"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_mcp_tool_invocation() -> anyhow::Result<()> {
        let client = setup_test_client().await?;

        // Create a test note using the add_note tool
        let json_str = r#"{
            "source_timezone": "America/New_York",
            "time": "16:30",
            "target_timezone": "Asia/Tokyo"
            }"#;
        let partial = McpToolPartial::parse("mcp::time::convert_time", json_str)?;
        let input = ToolInput::McpTool(McpToolInput { partial });

        let tool = McpTool::new(
            "time".to_string(),
            "convert_time".to_string(),
            "Convert time".to_string(),
            CONVERT_TIME_SCHEMA.clone(),
            Arc::clone(&client),
        );

        let result = tool.invoke(input).await?;

        // Verify the response
        match result {
            ToolOutput::McpTool(response) => {
                assert!(
                    response.data.contains("time_difference"),
                    "Response should contain time_difference field"
                );
            }
            _ => panic!("Unexpected response type"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_dynamic_mcp_tool_errors() -> anyhow::Result<()> {
        let client = setup_test_client().await?;

        let dyn_tool = McpTool::new(
            "time".to_string(),
            "convert_time".to_string(),
            "Convert time".to_string(),
            CONVERT_TIME_SCHEMA.clone(),
            Arc::clone(&client),
        );

        // Test wrong tool input type
        let wrong_input = ToolInput::OpenFile(OpenFileRequest::new(
            "test.txt".to_string(),
            "http://localhost".to_string(),
            None,
            None,
        ));
        let result = dyn_tool.invoke(wrong_input).await;
        assert!(matches!(result, Err(ToolError::WrongToolInput(_))));

        // Test missing required field
        let json_str = r#"{"source_timezone": "America/New_York"}"#;
        let partial = McpToolPartial::parse("mcp::time::convert_time", json_str).unwrap();
        let input = McpToolInput { partial };
        let result = dyn_tool.invoke(ToolInput::McpTool(input)).await;
        assert!(result.is_err(), "Should fail with missing required field");

        // Test with invalid JSON
        let invalid_json = r#"{"name": "Test Note", content: }"#; // Invalid JSON syntax
        let partial = McpToolPartial::parse("mcp::notes_server::add_note", invalid_json);
        assert!(partial.is_err(), "Should fail with invalid JSON");

        // Test with all required fields
        let valid_json = r#"{
            "source_timezone": "America/New_York",
            "time": "16:30",
            "target_timezone": "Asia/Tokyo"
        }"#;
        let partial = McpToolPartial::parse("mcp::time::convert_time", valid_json).unwrap();
        let input = McpToolInput { partial };
        let result = dyn_tool.invoke(ToolInput::McpTool(input)).await;
        assert!(result.is_ok(), "Should succeed with all required fields");

        Ok(())
    }
}
