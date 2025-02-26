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
use chrono::{DateTime, Utc};
use walkdir::WalkDir;
use regex::Regex;
use std::fs;

#[derive(Debug, Serialize)]
pub struct SecurityScan {
    id: String,
    timestamp: DateTime<Utc>,
    findings: Vec<SecurityFinding>,
}

#[derive(Debug, Serialize)]
pub struct SecurityFinding {
    severity: Severity,
    description: String,
    location: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuditLog {
    timestamp: DateTime<Utc>,
    action: String,
    user: String,
    resource: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecurityPolicy {
    name: String,
    enabled: bool,
    rules: Vec<SecurityRule>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecurityRule {
    name: String,
    description: String,
    enabled: bool,
    action: String,
}

pub struct SecurityManager {
    policies: RwLock<HashMap<String, SecurityPolicy>>,
    audit_logs: RwLock<Vec<AuditLog>>,
    vulnerability_patterns: Vec<(Severity, Regex)>,
}

impl SecurityManager {
    pub fn new() -> Arc<Self> {
        let vulnerability_patterns = vec![
            (Severity::High, Regex::new(r"(?i)password\s*=\s*['\"]\w+['\"]").unwrap()),
            (Severity::High, Regex::new(r"(?i)api_key\s*=\s*['\"]\w+['\"]").unwrap()),
            (Severity::Medium, Regex::new(r"(?i)TODO:|FIXME:").unwrap()),
            (Severity::Low, Regex::new(r"(?i)console\.log").unwrap()),
        ];

        Arc::new(Self {
            policies: RwLock::new(HashMap::new()),
            audit_logs: RwLock::new(Vec::new()),
            vulnerability_patterns,
        })
    }

    async fn scan_directory(&self, path: &PathBuf) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            if let Ok(content) = fs::read_to_string(entry.path()) {
                for (severity, pattern) in &self.vulnerability_patterns {
                    for mat in pattern.find_iter(&content) {
                        findings.push(SecurityFinding {
                            severity: severity.clone(),
                            description: format!("Found pattern: {}", mat.as_str()),
                            location: entry.path().to_string_lossy().into_owned(),
                        });
                    }
                }
            }
        }

        findings
    }

    async fn log_audit_event(&self, event: AuditLog) {
        let mut logs = self.audit_logs.write().await;
        logs.push(event);
    }

    async fn enforce_policy(&self, policy_name: &str, resource: &str) -> bool {
        let policies = self.policies.read().await;
        if let Some(policy) = policies.get(policy_name) {
            for rule in &policy.rules {
                if !rule.enabled {
                    continue;
                }
                // Add your policy enforcement logic here
                match rule.action.as_str() {
                    "deny" => return false,
                    "allow" => return true,
                    _ => continue,
                }
            }
        }
        true
    }
}

pub fn router() -> Router<Arc<SecurityManager>> {
    Router::new()
        .route("/security/scan", post(security_scan))
        .route("/security/audit", get(audit_logs))
        .route("/security/policy", get(get_policies))
        .route("/security/policy", post(update_policy))
}

async fn security_scan(
    State(manager): State<Arc<SecurityManager>>,
) -> Json<SecurityScan> {
    let path = PathBuf::from(".");
    let findings = manager.scan_directory(&path).await;

    let scan = SecurityScan {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        findings,
    };

    manager.log_audit_event(AuditLog {
        timestamp: Utc::now(),
        action: "security_scan".to_string(),
        user: "system".to_string(),
        resource: "codebase".to_string(),
        status: "completed".to_string(),
    }).await;

    Json(scan)
}

async fn audit_logs(
    State(manager): State<Arc<SecurityManager>>,
) -> Json<Vec<AuditLog>> {
    let logs = manager.audit_logs.read().await;
    Json(logs.clone())
}

async fn get_policies(
    State(manager): State<Arc<SecurityManager>>,
) -> Json<Vec<SecurityPolicy>> {
    let policies = manager.policies.read().await;
    Json(policies.values().cloned().collect())
}

async fn update_policy(
    State(manager): State<Arc<SecurityManager>>,
    Json(policy): Json<SecurityPolicy>,
) -> Result<StatusCode, StatusCode> {
    let mut policies = manager.policies.write().await;
    policies.insert(policy.name.clone(), policy);

    manager.log_audit_event(AuditLog {
        timestamp: Utc::now(),
        action: "update_policy".to_string(),
        user: "system".to_string(),
        resource: "security_policy".to_string(),
        status: "success".to_string(),
    }).await;

    Ok(StatusCode::OK)
}