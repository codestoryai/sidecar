use std::path::PathBuf;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale, ToolType},
    },
    git::operations::{
        commit_changes, create_branch, create_pull_request, push_changes, GitCommitOptions,
        GitPullRequestOptions,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitRequest {
    pub repo_path: PathBuf,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCreateBranchRequest {
    pub repo_path: PathBuf,
    pub branch_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitPushRequest {
    pub repo_path: PathBuf,
    pub branch_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCreatePullRequestRequest {
    pub repo_path: PathBuf,
    pub title: String,
    pub body: String,
    pub base_branch: String,
    pub head_branch: String,
    pub github_token: String,
}

pub struct GitCommitTool;
pub struct GitCreateBranchTool;
pub struct GitPushTool;
pub struct GitCreatePullRequestTool;

#[async_trait]
impl Tool for GitCommitTool {
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request: GitCommitRequest = serde_json::from_value(input.input)?;
        
        let options = GitCommitOptions {
            message: request.message,
            author_name: request.author_name,
            author_email: request.author_email,
        };
        
        commit_changes(&request.repo_path, &options)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        
        Ok(ToolOutput::Success("Changes committed successfully".to_string()))
    }

    fn tool_type(&self) -> ToolType {
        ToolType::GitCommit
    }

    fn reward_scale(&self) -> ToolRewardScale {
        ToolRewardScale::High
    }
}

#[async_trait]
impl Tool for GitCreateBranchTool {
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request: GitCreateBranchRequest = serde_json::from_value(input.input)?;
        
        create_branch(&request.repo_path, &request.branch_name)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        
        Ok(ToolOutput::Success(format!("Branch '{}' created successfully", request.branch_name)))
    }

    fn tool_type(&self) -> ToolType {
        ToolType::GitCreateBranch
    }

    fn reward_scale(&self) -> ToolRewardScale {
        ToolRewardScale::High
    }
}

#[async_trait]
impl Tool for GitPushTool {
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request: GitPushRequest = serde_json::from_value(input.input)?;
        
        push_changes(&request.repo_path, &request.branch_name)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        
        Ok(ToolOutput::Success(format!("Changes pushed to branch '{}' successfully", request.branch_name)))
    }

    fn tool_type(&self) -> ToolType {
        ToolType::GitPush
    }

    fn reward_scale(&self) -> ToolRewardScale {
        ToolRewardScale::High
    }
}

#[async_trait]
impl Tool for GitCreatePullRequestTool {
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request: GitCreatePullRequestRequest = serde_json::from_value(input.input)?;
        
        let options = GitPullRequestOptions {
            title: request.title,
            body: request.body,
            base_branch: request.base_branch,
            head_branch: request.head_branch,
        };
        
        create_pull_request(&request.repo_path, &request.github_token, &options)
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        
        Ok(ToolOutput::Success("Pull request created successfully".to_string()))
    }

    fn tool_type(&self) -> ToolType {
        ToolType::GitCreatePullRequest
    }

    fn reward_scale(&self) -> ToolRewardScale {
        ToolRewardScale::High
    }
} 