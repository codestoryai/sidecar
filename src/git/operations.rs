use std::path::Path;
use anyhow::{Context, Result};
use gix::{self, Repository};
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct GitCommitOptions {
    pub message: String,
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GitPullRequestOptions {
    pub title: String,
    pub body: String,
    pub base_branch: String,
    pub head_branch: String,
}

pub async fn commit_changes(repo_path: &Path, options: &GitCommitOptions) -> Result<()> {
    let repo = gix::open(repo_path)?;
    let mut index = repo.index()?;
    
    // Stage all changes
    index.add_all()?;
    
    // Create commit
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    
    let head = repo.head()?;
    let parent_commit = head.peel_to_commit()?;
    
    repo.commit(
        Some("HEAD"),
        &options.author_name,
        &options.author_email,
        &options.message,
        &tree,
        &[&parent_commit],
    )?;
    
    Ok(())
}

pub async fn create_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
    let repo = gix::open(repo_path)?;
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;
    
    repo.branch(branch_name, &commit, false)?;
    
    Ok(())
}

pub async fn push_changes(repo_path: &Path, branch_name: &str) -> Result<()> {
    let repo = gix::open(repo_path)?;
    let mut remote = repo.find_remote("origin")?;
    
    remote.push(&[format!("refs/heads/{}", branch_name)])?;
    
    Ok(())
}

pub async fn create_pull_request(
    repo_path: &Path,
    github_token: &str,
    options: &GitPullRequestOptions,
) -> Result<()> {
    let repo = gix::open(repo_path)?;
    let remote_url = repo
        .find_remote("origin")?
        .url()
        .context("Failed to get remote URL")?
        .to_string();
    
    // Extract owner and repo from remote URL
    let (owner, repo_name) = parse_github_url(&remote_url)?;
    
    // Create GitHub client
    let octocrab = Octocrab::builder()
        .personal_token(github_token.to_string())
        .build()?;
    
    // Create pull request
    octocrab
        .pulls(owner, repo_name)
        .create(&options.title, &options.head_branch, &options.base_branch)
        .body(&options.body)
        .send()
        .await?;
    
    Ok(())
}

fn parse_github_url(url: &str) -> Result<(&str, &str)> {
    // Extract owner and repo from GitHub URL (e.g., "https://github.com/owner/repo.git")
    let parts: Vec<&str> = url.trim_end_matches(".git").split('/').collect();
    let repo = parts.last().context("No repository name found")?;
    let owner = parts.get(parts.len() - 2).context("No owner found")?;
    
    Ok((owner, repo))
} 