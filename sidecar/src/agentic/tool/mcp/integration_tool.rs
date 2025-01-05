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

// TODO: remove before merge
// old single-broker approach

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
pub enum MCPIntegrationToolAction {
    List,
    Call,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MCPIntegrationToolQuery {
    pub action: MCPIntegrationToolAction,
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

/// The old broker that can do list/call across multiple servers
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

            let mut descriptors = Vec::new();
            for tool in tools_value.tools {
                descriptors.push(ToolDescriptor {
                    name: tool.name,
                    description: Some(tool.description),
                    schema: Some(tool.schema),
                });
            }
            server_list.push(ServerTools {
                server_name: server_name.clone(),
                tools: descriptors,
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

        let res = client.call_tool(tool_name, arguments).await.map_err(|e| {
            ToolError::InvocationError(format!("call_tool failed for '{}': {}", tool_name, e))
        })?;

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
        match query.action {
            MCPIntegrationToolAction::List => self.list_all_tools().await,
            MCPIntegrationToolAction::Call => {
                let server_name = query.server_name.as_ref().ok_or_else(|| {
                    ToolError::InvalidInput("Missing 'server_name' for call".to_string())
                })?;
                let tool_name = query.tool_name.as_ref().ok_or_else(|| {
                    ToolError::InvalidInput("Missing 'tool_name' for call".to_string())
                })?;

                self.call_tool(server_name, tool_name, query.arguments.clone())
                    .await
            }
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
            MCPIntegrationToolResponse::ToolCall(ToolCallResponse { result: value }),
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
