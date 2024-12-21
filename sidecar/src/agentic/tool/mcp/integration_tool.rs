use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};
use async_trait::async_trait;
use mcp_client_rs::client::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

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
pub struct ToolCallResponse {
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MCPIntegrationToolResponse {
    ToolList(ToolListResponse),
    ToolCall(ToolCallResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MCPIntegrationToolQuery {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub arguments: Value,
}

impl MCPIntegrationToolQuery {
    pub fn to_json() -> Value {
        serde_json::json!({
            "action": "list | call",
            "server_name": "string (required if action=call)",
            "tool_name": "string (required if action=call)",
            "arguments": {}
        })
    }
}

pub struct MCPIntegrationToolBroker {
    servers: HashMap<String, Client>,
}

impl MCPIntegrationToolBroker {
    pub fn new(servers: HashMap<String, Client>) -> Self {
        Self { servers }
    }

    async fn list_all_tools(&self) -> Result<MCPIntegrationToolResponse, ToolError> {
        let mut server_list = Vec::new();
        for (server_name, client) in &self.servers {
            let tools_value = client.list_tools().await.map_err(|e| {
                ToolError::InvocationError(format!(
                    "Failed to list tools from '{}': {}",
                    server_name, e
                ))
            })?;

            let tools_array = tools_value.tools;

            let mut tool_descriptors = Vec::new();
            for tool in tools_array {
                tool_descriptors.push(ToolDescriptor {
                    name: tool.name,
                    description: Some(tool.description),
                    schema: Some(tool.schema),
                });
            }

            server_list.push(ServerTools {
                server_name: server_name.clone(),
                tools: tool_descriptors,
            });
        }
        Ok(MCPIntegrationToolResponse::ToolList(ToolListResponse {
            servers: server_list,
        }))
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

        Ok(MCPIntegrationToolResponse::ToolCall(ToolCallResponse {
            result: serde_json::to_value(res).map_err(|e| {
                ToolError::InvocationError(format!("Failed to serialize tool result: {}", e))
            })?,
        }))
    }

    async fn handle_query(
        &self,
        query: MCPIntegrationToolQuery,
    ) -> Result<MCPIntegrationToolResponse, ToolError> {
        match query.action.as_str() {
            "list" => self.list_all_tools().await,
            "call" => {
                let server_name = query.server_name.as_ref().ok_or_else(|| {
                    ToolError::InvalidInput("Missing 'server_name' for call action".to_string())
                })?;
                let tool_name = query.tool_name.as_ref().ok_or_else(|| {
                    ToolError::InvalidInput("Missing 'tool_name' for call action".to_string())
                })?;
                self.call_tool(server_name, tool_name, query.arguments.clone())
                    .await
            }
            _ => Err(ToolError::InvalidInput("Unknown action".to_string())),
        }
    }
}

#[async_trait]
impl Tool for MCPIntegrationToolBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let query = match input {
            ToolInput::MCPIntegrationTool(q) => q,
            _ => {
                return Err(ToolError::InvalidInput(
                    "Expected MCPIntegrationTool input".to_string(),
                ))
            }
        };

        let response = self.handle_query(query).await?;
        Ok(ToolOutput::MCPIntegration(response))
    }

    fn tool_description(&self) -> String {
        // TODO: change description to aggregate descriptions of all servers (or maybe a simpler option?)
        "The MCP Integration tool: Use 'action':'list' to list all servers & tools, 'action':'call' to invoke a tool.".to_string()
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
