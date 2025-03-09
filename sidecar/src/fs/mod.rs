use axum::{
    routing::{get, post},
    Router,
    Json,
    http::StatusCode,
    extract::Query,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, collections::HashMap};
use tokio::fs;
use ignore::WalkBuilder;

#[derive(Debug, Serialize)]
pub struct FileInfo {
    path: String,
    is_dir: bool,
    size: u64,
    modified: u64,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pattern: String,
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Deserialize)]
pub struct WatchRequest {
    path: String,
    recursive: bool,
}

pub fn router() -> Router {
    Router::new()
        .route("/fs/watch", post(watch_directory))
        .route("/fs/search", get(search_files))
        .route("/fs/workspace", get(get_workspace_info))
}

async fn watch_directory(
    State(state): State<Arc<FsState>>,
    Json(req): Json<WatchRequest>
) -> Result<StatusCode, StatusCode> {
    let path = PathBuf::from(&req.path);
    if !path.exists() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let tx = state.event_tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    }).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mode = if req.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    watcher.watch(&path, mode)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut watchers = state.watchers.write().await;
    watchers.insert(req.path, watcher);

    Ok(StatusCode::OK)
}

async fn search_files(Query(query): Query<SearchQuery>) -> Json<Vec<FileInfo>> {
    let mut files = Vec::new();
    
    if let Ok(walker) = WalkBuilder::new(".")
        .hidden(false)
        .build() {
        for entry in walker.filter_map(Result::ok) {
            if let Some(path) = entry.path().to_str() {
                if path.contains(&query.pattern) {
                    if let Ok(metadata) = entry.metadata() {
                        files.push(FileInfo {
                            path: path.to_string(),
                            is_dir: metadata.is_dir(),
                            size: metadata.len(),
                            modified: metadata.modified()
                                .map(|time| time.duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default().as_secs())
                                .unwrap_or(0),
                        });
                    }
                }
            }
        }
    }
    
    Json(files)
}

async fn get_workspace_info() -> Json<HashMap<String, String>> {
    let mut info = HashMap::new();
    info.insert("root".to_string(), ".".to_string());
    info.insert("git_enabled".to_string(), "true".to_string());
    Json(info)
}