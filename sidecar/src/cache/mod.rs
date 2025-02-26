use axum::{
    routing::{get, post},
    Router,
    Json,
    http::StatusCode,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::{SystemTime, Duration}};
use tokio::sync::RwLock;
use tokio::time::interval;

#[derive(Debug, Clone)]
struct CacheEntry {
    value: Vec<u8>,
    expiry: SystemTime,
}

pub struct CacheState {
    entries: RwLock<HashMap<String, CacheEntry>>,
    config: RwLock<CacheConfig>,
    stats: RwLock<CacheStats>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheStats {
    size: u64,
    items: u64,
    hit_rate: f32,
    miss_rate: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheConfig {
    max_size: u64,
    ttl_seconds: u64,
    compression_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct ClearRequest {
    cache_type: String,
}

impl CacheState {
    pub fn new(config: CacheConfig) -> Arc<Self> {
        let state = Arc::new(Self {
            entries: RwLock::new(HashMap::new()),
            config: RwLock::new(config),
            stats: RwLock::new(CacheStats {
                size: 0,
                items: 0,
                hit_rate: 0.0,
                miss_rate: 0.0,
            }),
        });

        // Start background cleanup task
        let state_clone = state.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                state_clone.cleanup_expired().await;
            }
        });

        state
    }

    async fn cleanup_expired(&self) {
        let now = SystemTime::now();
        let mut entries = self.entries.write().await;
        entries.retain(|_, entry| entry.expiry > now);
        
        let mut stats = self.stats.write().await;
        stats.items = entries.len() as u64;
    }

    async fn clear_cache(&self, cache_type: &str) {
        let mut entries = self.entries.write().await;
        if cache_type == "*" {
            entries.clear();
        } else {
            entries.retain(|key, _| !key.starts_with(cache_type));
        }
        
        let mut stats = self.stats.write().await;
        stats.items = entries.len() as u64;
    }
}

pub fn router() -> Router<Arc<CacheState>> {
    Router::new()
        .route("/cache/clear", post(clear_cache))
        .route("/cache/status", get(cache_status))
        .route("/cache/configure", post(configure_cache))
}

async fn clear_cache(
    State(state): State<Arc<CacheState>>,
    Json(req): Json<ClearRequest>
) -> Result<StatusCode, StatusCode> {
    state.clear_cache(&req.cache_type).await;
    Ok(StatusCode::OK)
}

async fn cache_status(
    State(state): State<Arc<CacheState>>
) -> Json<CacheStats> {
    let stats = state.stats.read().await;
    Json(stats.clone())
}

async fn configure_cache(
    State(state): State<Arc<CacheState>>,
    Json(config): Json<CacheConfig>
) -> Result<StatusCode, StatusCode> {
    let mut current_config = state.config.write().await;
    *current_config = config;
    Ok(StatusCode::OK)
}