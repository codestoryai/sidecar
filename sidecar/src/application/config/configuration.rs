use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::repo::state::StateSource;

#[derive(Serialize, Deserialize, Parser, Debug, Clone, Default)]
#[clap(author, version, about, long_about = None)]
pub struct Configuration {
    #[clap(short, long, default_value_os_t = default_index_dir())]
    #[serde(default = "default_index_dir")]
    /// Directory to store all persistent state
    pub index_dir: PathBuf,

    #[clap(long, default_value_t = default_port())]
    #[serde(default = "default_port")]
    /// Bind the webserver to `<host>`
    pub port: u16,

    #[clap(long, default_value_t = default_host())]
    #[serde(default = "default_host")]
    /// Bind the webserver to `<port>`
    pub host: String,

    #[clap(flatten)]
    #[serde(default)]
    pub state_source: StateSource,

    #[clap(short, long, default_value_t = default_parallelism())]
    #[serde(default = "default_parallelism")]
    /// Maximum number of parallel background threads
    pub max_threads: usize,

    #[clap(short, long, default_value_t = default_buffer_size())]
    #[serde(default = "default_buffer_size")]
    /// Size of memory to use for file indexes
    pub buffer_size: usize,

    /// Qdrant allows us to create collections and we need to provide it a default
    /// value to start with
    #[clap(short, long, default_value_t = default_collection_name())]
    #[serde(default = "default_collection_name")]
    pub collection_name: String,

    #[clap(long, default_value_t = default_user_id())]
    #[serde(default = "default_user_id")]
    pub user_id: String,

    /// If we should poll the local repo for updates auto-magically. Disabled
    /// by default, until we figure out the delta sync method where we only
    /// reindex the files which have changed
    #[clap(long)]
    #[serde(default)]
    pub enable_background_polling: bool,

    #[clap(long)]
    pub llm_endpoint: Option<String>,

    #[clap(long)]
    #[serde(default)]
    pub apply_directly: bool,
}

impl Configuration {
    /// Directory where logs are written to
    pub fn log_dir(&self) -> PathBuf {
        self.index_dir.join("logs")
    }

    pub fn index_path(&self, name: impl AsRef<Path>) -> impl AsRef<Path> {
        self.index_dir.join(name)
    }

    pub fn qdrant_storage(&self) -> PathBuf {
        self.index_dir.join("qdrant_storage")
    }

    pub fn scratch_pad(&self) -> PathBuf {
        self.index_dir.join("scratch_pad")
    }

    pub fn models_config_path(&self) -> Option<PathBuf> {
        self.model_config_path.clone().or_else(|| {
            self.index_dir.join("models_config.json").exists().then(|| {
                self.index_dir.join("models_config.json")
            })
        })
    }

    pub async fn load_model_config(&mut self) -> anyhow::Result<()> {
        if let Some(path) = self.models_config_path() {
            if path.exists() {
                let content = tokio::fs::read_to_string(path).await?;
                self.model_configuration = Some(serde_json::from_str(&content)?);
            }
        }
        Ok(())
    }

    pub fn lsp_config_path(&self) -> Option<PathBuf> {
        self.lsp_config_path.clone().or_else(|| {
            self.index_dir.join("lsp_config.json").exists().then(|| {
                self.index_dir.join("lsp_config.json")
            })
        })
    }

    pub async fn load_lsp_config(&mut self) -> anyhow::Result<()> {
        if let Some(path) = self.lsp_config_path() {
            if path.exists() {
                let content = tokio::fs::read_to_string(path).await?;
                self.lsp_configuration = Some(serde_json::from_str(&content)?);
            }
        }
        Ok(())
    }
}

fn default_index_dir() -> PathBuf {
    match directories::ProjectDirs::from("ai", "codestory", "sidecar") {
        Some(dirs) => dirs.data_dir().to_owned(),
        None => "codestory_sidecar".into(),
    }
}

fn default_port() -> u16 {
    42424
}

fn default_host() -> String {
    "127.0.0.1".to_owned()
}

pub fn default_parallelism() -> usize {
    std::thread::available_parallelism().unwrap().get()
}

fn default_buffer_size() -> usize {
    100_000_000 * default_parallelism()
}

fn default_collection_name() -> String {
    "codestory".to_owned()
}

fn default_user_id() -> String {
    let username = whoami::username();
    username
}