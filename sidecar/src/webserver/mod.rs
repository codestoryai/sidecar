pub mod agent;
pub mod agent_stream;
pub mod agentic;
pub mod config;
pub mod context_trimming;
pub mod file_edit;
pub mod health;
pub mod in_line_agent;
pub mod in_line_agent_stream;
pub mod inline_completion;
pub mod model_selection;
pub mod plan;
pub mod tree_sitter;
pub mod types;

use axum::Router;
use std::sync::Arc;
use crate::{
    auth, fs, lsp, cache, metrics, plugins, security,
    webserver::{
        health, config, tree_sitter, agent, agentic,
    },
    application::Application,
};

pub async fn create_router(app: Arc<Application>) -> anyhow::Result<Router> {
    // Initialize states for independent services
    let cache_state = cache::CacheState::new(cache::CacheConfig {
        max_size: 1024 * 1024 * 100, // 100MB
        ttl_seconds: 3600,
        compression_enabled: true,
    });
    
    let metrics_state = metrics::MetricsState::new();
    let plugin_manager = plugins::PluginManager::new();
    let security_manager = security::SecurityManager::new();

    Ok(Router::new()
        // Existing routes
        .merge(health::router())
        .merge(config::router())
        .merge(tree_sitter::router())
        .merge(agent::router())
        .merge(agentic::router())
        // Independent service routes with state
        .merge(auth::router())
        .merge(fs::router())
        .merge(lsp::router().with_state(app.lsp_state.clone()))
        .merge(cache::router().with_state(cache_state))
        .merge(metrics::router().with_state(metrics_state))
        .merge(plugins::router().with_state(plugin_manager))
        .merge(security::router().with_state(security_manager)))
}