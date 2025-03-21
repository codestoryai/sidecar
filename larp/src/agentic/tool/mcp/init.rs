use super::integration_tool::McpTool;
use mcp_client_rs::client::Client;
use mcp_client_rs::client::ClientBuilder;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

// Minimal code for MCP client spawner
#[derive(Deserialize)]
struct ServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct RootConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, ServerConfig>,
}

#[derive(Error, Debug)]
pub enum McpError {
    #[error("Could not determine home directory")]
    NoHomeDir,

    #[error("Failed to read config file: {0}")]
    ConfigReadError(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    ConfigParseError(#[from] serde_json::Error),

    #[error("Failed listing tools from server '{server}': {source}")]
    ToolListError {
        server: String,
        source: mcp_client_rs::Error,
    },
}

/// Set up MCP clients by reading ~/.aide/config.json, spawning each server,
/// and returning a HashMap<server_name -> Arc<Client>>.
/// spawn a single MCP process per server, share references.
async fn setup_mcp_clients() -> Result<HashMap<String, Arc<Client>>, McpError> {
    let config_path = dirs::home_dir()
        .ok_or(McpError::NoHomeDir)?
        .join(".aide/config.json");

    if !config_path.exists() {
        return Ok(HashMap::new());
    }

    let config_str = tokio::fs::read_to_string(&config_path).await?;
    let root_config: RootConfig = serde_json::from_str(&config_str)?;

    let mut mcp_clients_map = HashMap::new();

    // For each server in the config, spawn an MCP client
    for (server_name, server_conf) in &root_config.mcp_servers {
        let mut builder = ClientBuilder::new(&server_conf.command);
        for arg in &server_conf.args {
            builder = builder.arg(arg);
        }
        for (k, v) in &server_conf.env {
            builder = builder.env(k, v);
        }

        match builder.spawn_and_initialize().await {
            Ok(client) => {
                let client_arc = Arc::new(client);
                mcp_clients_map.insert(server_name.clone(), client_arc);
                eprintln!("Initialized MCP client for '{}'", server_name);
            }
            Err(e) => {
                eprintln!(
                    "Failed to initialize MCP client for '{}': {}",
                    server_name, e
                );
                // keep trying other clients
            }
        }
    }

    Ok(mcp_clients_map)
}

/// discover each MCP server in ~/.aide/config.json
/// create dynamic tools from each server
/// used to augument broker initialization w/MCP tools
pub async fn discover_mcp_tools() -> Result<Vec<McpTool>, McpError> {
    let clients = setup_mcp_clients().await?;
    if clients.is_empty() {
        return Ok(Vec::new());
    }

    let mut tools: Vec<McpTool> = Vec::new();

    for (server_name, client) in clients {
        let list_res = client
            .list_tools()
            .await
            .map_err(|e| McpError::ToolListError {
                server: server_name.clone(),
                source: e,
            })?;

        for tool_info in list_res.tools {
            let name = tool_info.name;
            let tool: McpTool = McpTool::new(
                server_name.clone(),
                name.clone(),
                tool_info.description,
                tool_info.input_schema,
                Arc::clone(&client),
            );

            tools.push(tool);
        }
    }

    Ok(tools)
}
