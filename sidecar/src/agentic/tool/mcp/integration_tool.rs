use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};
use mcp_client_rs::client::Client;
use crate::agentic::tool::code_edit::code_editor::EditorCommand;


/// A request structure for MCPIntegrationTool, analogous to ImportantFilesFinderQuery.
/// This holds the action ("list" or "call"), and optional fields for server and tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MCPIntegrationToolQuery {
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Value,
}

impl MCPIntegrationToolQuery {
    pub fn new(
        action: String,
        server_name: Option<String>,
        tool_name: Option<String>,
        arguments: Value,
    ) -> Self {
        Self {
            action,
            server_name,
            tool_name,
            arguments,
        }
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "action": "list | call",
            "server_name": "string",
            "tool_name": "string",
            "arguments": {}
        })
    }

    pub fn action(&self) -> &str {
        &self.action
    }

    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }

    pub fn tool_name(&self) -> Option<&str> {
        self.tool_name.as_deref()
    }

    pub fn arguments(&self) -> &Value {
        &self.arguments
    }
}

/// The response from the MCPIntegrationTool, always a JSON value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPIntegrationToolResponse {
    data: Value,
}

impl MCPIntegrationToolResponse {
    pub fn new(data: Value) -> Self {
        Self { data }
    }

    pub fn data(&self) -> &Value {
        &self.data
    }
}

/// The MCPIntegrationToolBroker is analogous to ImportantFilesFinderBroker but for MCP aggregator.
/// It manages a set of MCP clients and performs "list" and "call" actions.
pub struct MCPIntegrationToolBroker {
    servers: HashMap<String, Arc<Client>>,
}

impl MCPIntegrationToolBroker {
    pub fn new(servers: HashMap<String, Arc<Client>>) -> Self {
        Self { servers }
    }

    async fn list_all_tools(&self) -> Result<MCPIntegrationToolResponse, ToolError> {
        let mut result = Vec::new();
        for (server_name, client) in &self.servers {
            let tools = client
                .list_tools()
                .await
                .map_err(|e| ToolError::InvocationError(format!("Failed to list tools: {}", e)))?;
            result.push(serde_json::json!({
                "server_name": server_name,
                "tools": tools
            }));
        }
        Ok(MCPIntegrationToolResponse::new(Value::Array(result)))
    }

    async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<MCPIntegrationToolResponse, ToolError> {
        let client = self.servers.get(server_name).ok_or_else(|| {
            ToolError::InvalidInput(format!("Server '{}' not found", server_name))
        })?;

        let res = client
            .call_tool(tool_name, arguments)
            .await
            .map_err(|e| ToolError::InvocationError(format!("call_tool failed: {}", e)))?;
        Ok(MCPIntegrationToolResponse::new(res))
    }

    async fn handle_query(
        &self,
        query: MCPIntegrationToolQuery,
    ) -> Result<MCPIntegrationToolResponse, ToolError> {
        match query.action() {
            "list" => self.list_all_tools().await,
            "call" => {
                let server_name = query.server_name().ok_or_else(|| {
                    ToolError::InvalidInput("Missing 'server_name' for call action".to_string())
                })?;
                let tool_name = query.tool_name().ok_or_else(|| {
                    ToolError::InvalidInput("Missing 'tool_name' for call action".to_string())
                })?;
                self.call_tool(server_name, tool_name, query.arguments().clone())
                    .await
            }
            _ => Err(ToolError::InvalidInput("Unknown action".to_string())),
        }
    }
}

#[async_trait]
impl Tool for MCPIntegrationToolBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        // Extract query from the input based on its variant
        let query = match input {
            ToolInput::MCPIntegrationTool(query) => query,
            _ => {
                return Err(ToolError::InvalidInput(
                    "Expected IntegrationTool input".to_string(),
                ))
            }
        };

        let response = self.handle_query(query).await?;

        // Return the response as a ToolOutput::CodeEditTool variant
        Ok(ToolOutput::CodeEditTool(
            serde_json::to_string(response.data()).map_err(|e| {
                ToolError::InvocationError(format!("Failed to serialize response: {}", e))
            })?,
        ))
    }

    fn tool_description(&self) -> String {
        "The Integration tool for MCP servers: Use action='list' to see all servers & tools, action='call' to invoke a specific tool.".to_string()
    }

    fn tool_input_format(&self) -> String {
        r#"{"action":"list"} or {"action":"call","server_name":"string","tool_name":"string","arguments":{}}"#.to_string()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
