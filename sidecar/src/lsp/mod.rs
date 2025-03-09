use axum::{
    routing::{get, post},
    Router,
    Json,
    http::StatusCode,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, path::{Path, PathBuf}, process::{Child, Command}};
use tokio::sync::RwLock;
use tower_lsp::lsp_types::*;
use anyhow::Result;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LspServerConfig {
    command: String,
    args: Vec<String>,
    initialization_options: HashMap<String, serde_json::Value>,
    root_markers: Vec<String>,
    capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlobalSettings {
    workspace_folders: Vec<String>,
    sync_kind: String,
    completion_trigger_characters: Vec<String>,
    signature_trigger_characters: Vec<String>,
    hover_trigger_characters: Vec<String>,
    code_action_trigger_characters: Vec<String>,
    format_on_save: bool,
    max_completion_items: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LspConfiguration {
    language_servers: HashMap<String, LspServerConfig>,
    global_settings: GlobalSettings,
}

#[derive(Debug)]
pub struct LspServerInstance {
    config: LspServerConfig,
    process: Child,
    status: LspStatus,
    workspace_folders: Vec<PathBuf>,
}

pub struct LspState {
    servers: RwLock<HashMap<String, LspServerInstance>>,
    config: RwLock<LspConfiguration>,
    capabilities: RwLock<HashMap<String, Vec<String>>>,
}

impl LspState {
    pub async fn new() -> Result<Arc<Self>> {
        let default_config = LspConfiguration {
            language_servers: HashMap::new(),
            global_settings: GlobalSettings {
                workspace_folders: vec!["src".to_string(), "tests".to_string()],
                sync_kind: "full".to_string(),
                completion_trigger_characters: vec![".".to_string(), ":".to_string()],
                signature_trigger_characters: vec!["(".to_string(), ",".to_string()],
                hover_trigger_characters: vec![".".to_string()],
                code_action_trigger_characters: vec![".".to_string()],
                format_on_save: true,
                max_completion_items: 100,
            },
        };

        Ok(Arc::new(Self {
            servers: RwLock::new(HashMap::new()),
            config: RwLock::new(default_config),
            capabilities: RwLock::new(HashMap::new()),
        }))
    }

    pub async fn load_configuration(&self, path: &Path) -> Result<()> {
        let content = tokio::fs::read_to_string(path).await?;
        let config: LspConfiguration = serde_json::from_str(&content)?;
        
        let mut current_config = self.config.write().await;
        *current_config = config;
        
        // Update capabilities
        let mut caps = self.capabilities.write().await;
        for (lang, server_config) in current_config.language_servers.iter() {
            caps.insert(lang.clone(), server_config.capabilities.clone());
        }
        
        Ok(())
    }

    pub async fn start_server(&self, language: &str, workspace_root: PathBuf) -> Result<()> {
        let config = self.config.read().await;
        let server_config = config.language_servers.get(language)
            .ok_or_else(|| anyhow::anyhow!("Language server not configured: {}", language))?;

        // Check if root markers exist
        let has_root_marker = server_config.root_markers.iter().any(|marker| {
            workspace_root.join(marker).exists()
        });
        
        if !has_root_marker {
            return Err(anyhow::anyhow!("No root markers found for {}", language));
        }

        // Start the language server process
        let mut process = Command::new(&server_config.command)
            .args(&server_config.args)
            .current_dir(&workspace_root)
            .spawn()?;

        let instance = LspServerInstance {
            config: server_config.clone(),
            process,
            status: LspStatus::Running,
            workspace_folders: vec![workspace_root],
        };

        let mut servers = self.servers.write().await;
        servers.insert(language.to_string(), instance);

        Ok(())
    }

    pub async fn stop_server(&self, language: &str) -> Result<()> {
        let mut servers = self.servers.write().await;
        if let Some(mut instance) = servers.remove(language) {
            instance.process.kill()?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LspServerInfo {
    language: String,
    status: LspStatus,
    pid: Option<u32>,
    capabilities: Vec<String>,
    workspace_folders: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum LspStatus {
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    language: String,
    workspace_root: PathBuf,
}

pub fn router() -> Router<Arc<LspState>> {
    Router::new()
        .route("/lsp/start", post(start_server))
        .route("/lsp/stop", post(stop_server))
        .route("/lsp/status", get(server_status))
        .route("/lsp/capabilities", get(get_capabilities))
}

async fn start_server(
    State(state): State<Arc<LspState>>,
    Json(req): Json<StartRequest>
) -> Result<StatusCode, StatusCode> {
    state.start_server(&req.language, req.workspace_root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn stop_server(
    State(state): State<Arc<LspState>>,
    Json(req): Json<StartRequest>
) -> Result<StatusCode, StatusCode> {
    state.stop_server(&req.language)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn server_status(
    State(state): State<Arc<LspState>>
) -> Json<HashMap<String, LspServerInfo>> {
    let servers = state.servers.read().await;
    let mut status = HashMap::new();
    
    for (language, instance) in servers.iter() {
        status.insert(
            language.clone(),
            LspServerInfo {
                language: language.clone(),
                status: instance.status.clone(),
                pid: instance.process.id(),
                capabilities: instance.config.capabilities.clone(),
                workspace_folders: instance.workspace_folders.clone(),
            },
        );
    }
    
    Json(status)
}

async fn get_capabilities(
    State(state): State<Arc<LspState>>
) -> Json<HashMap<String, Vec<String>>> {
    let capabilities = state.capabilities.read().await;
    Json(capabilities.clone())
}