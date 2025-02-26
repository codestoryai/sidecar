use axum::{
    routing::{get, post},
    Router,
    Json,
    http::StatusCode,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, path::PathBuf};
use tokio::sync::RwLock;
use async_trait::async_trait;
use libloading::{Library, Symbol};

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginInfo {
    name: String,
    version: String,
    status: PluginStatus,
    config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus {
    Active,
    Inactive,
    Error,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    name: String,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigureRequest {
    name: String,
    config: HashMap<String, serde_json::Value>,
}

#[async_trait]
pub trait Plugin: Send + Sync {
    async fn initialize(&self) -> Result<(), String>;
    async fn shutdown(&self) -> Result<(), String>;
    async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, String>;
}

pub struct PluginManager {
    plugins: RwLock<HashMap<String, Box<dyn Plugin>>>,
    libraries: RwLock<HashMap<String, Library>>,
    configs: RwLock<HashMap<String, serde_json::Value>>,
}

impl PluginManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            plugins: RwLock::new(HashMap::new()),
            libraries: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
        })
    }

    async fn load_plugin(&self, name: &str, path: &PathBuf) -> Result<(), String> {
        unsafe {
            let library = Library::new(path)
                .map_err(|e| format!("Failed to load plugin library: {}", e))?;

            let constructor: Symbol<fn() -> Box<dyn Plugin>> = library
                .get(b"_plugin_create")
                .map_err(|e| format!("Plugin entry point not found: {}", e))?;

            let plugin = constructor();
            plugin.initialize().await?;

            let mut plugins = self.plugins.write().await;
            let mut libraries = self.libraries.write().await;
            
            plugins.insert(name.to_string(), plugin);
            libraries.insert(name.to_string(), library);
        }
        Ok(())
    }

    async fn unload_plugin(&self, name: &str) -> Result<(), String> {
        let mut plugins = self.plugins.write().await;
        let mut libraries = self.libraries.write().await;

        if let Some(plugin) = plugins.remove(name) {
            plugin.shutdown().await?;
            libraries.remove(name);
        }
        Ok(())
    }
}

pub fn router() -> Router<Arc<PluginManager>> {
    Router::new()
        .route("/plugins/list", get(list_plugins))
        .route("/plugins/install", post(install_plugin))
        .route("/plugins/configure", post(configure_plugin))
        .route("/plugins/status", get(plugin_status))
}

async fn install_plugin(
    State(manager): State<Arc<PluginManager>>,
    Json(req): Json<InstallRequest>,
) -> Result<StatusCode, StatusCode> {
    let plugin_path = PathBuf::from("plugins").join(&req.name);
    manager.load_plugin(&req.name, &plugin_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn configure_plugin(
    State(manager): State<Arc<PluginManager>>,
    Json(req): Json<ConfigureRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut configs = manager.configs.write().await;
    configs.insert(req.name.clone(), serde_json::to_value(req.config).unwrap());
    Ok(StatusCode::OK)
}

async fn list_plugins(
    State(manager): State<Arc<PluginManager>>,
) -> Json<Vec<String>> {
    let plugins = manager.plugins.read().await;
    Json(plugins.keys().cloned().collect())
}

async fn plugin_status(
    State(manager): State<Arc<PluginManager>>,
) -> Json<HashMap<String, PluginInfo>> {
    let plugins = manager.plugins.read().await;
    let configs = manager.configs.read().await;
    
    let mut status = HashMap::new();
    for (name, _) in plugins.iter() {
        let config = configs.get(name).cloned().unwrap_or_default();
        status.insert(
            name.clone(),
            PluginInfo {
                name: name.clone(),
                version: "1.0.0".to_string(),
                status: PluginStatus::Active,
                config: serde_json::from_value(config).unwrap_or_default(),
            },
        );
    }
    Json(status)
}