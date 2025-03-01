use git2::{Repository, Signature};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub enum GitOperation {
    Add,
    Commit,
    Status,
}

#[derive(Debug, Deserialize)]
pub struct GitRequest {
    operation: GitOperation,
    paths: Option<Vec<String>>,
    message: Option<String>,
    author_name: Option<String>,
    author_email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GitResponse {
    success: bool,
    message: String,
    error: Option<String>,
}

#[derive(Error, Debug)]
pub enum GitToolError {
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

pub struct GitTool {
    repo: Repository,
}

impl GitTool {
    pub fn new(repo_path: &str) -> Result<Self, GitToolError> {
        let repo = Repository::open(repo_path)?;
        Ok(Self { repo })
    }

    pub fn execute(&self, request: GitRequest) -> Result<GitResponse, GitToolError> {
        match request.operation {
            GitOperation::Add => self.add_files(request.paths),
            GitOperation::Commit => self.create_commit(request),
            GitOperation::Status => self.get_status(),
        }
    }

    fn add_files(&self, paths: Option<Vec<String>>) -> Result<GitResponse, GitToolError> {
        let paths = paths.ok_or_else(|| GitToolError::MissingField("paths".to_string()))?;
        
        let mut index = self.repo.index()?;
        for path in paths {
            index.add_path(std::path::Path::new(&path))?;
        }
        index.write()?;

        Ok(GitResponse {
            success: true,
            message: format!("Added {} files to staging", paths.len()),
            error: None,
        })
    }

    fn create_commit(&self, request: GitRequest) -> Result<GitResponse, GitToolError> {
        let message = request.message
            .ok_or_else(|| GitToolError::MissingField("message".to_string()))?;
        
        let signature = Signature::now(
            &request.author_name.unwrap_or_else(|| "Sidecar Agent".to_string()),
            &request.author_email.unwrap_or_else(|| "agent@sidecar.ai".to_string()),
        )?;

        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;
        
        let parent = if let Ok(head) = self.repo.head() {
            Some(head.peel_to_commit()?)
        } else {
            None
        };
        
        let parents = parent.as_ref().map(|c| vec![c]).unwrap_or_default();
        
        self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &tree,
            parents.as_slice(),
        )?;

        Ok(GitResponse {
            success: true,
            message: "Successfully created commit".to_string(),
            error: None,
        })
    }

    fn get_status(&self) -> Result<GitResponse, GitToolError> {
        let statuses = self.repo.statuses(None)?;
        
        let mut staged = 0;
        let mut modified = 0;
        let mut untracked = 0;

        for entry in statuses.iter() {
            match entry.status() {
                s if s.is_index_new() || s.is_index_modified() => staged += 1,
                s if s.is_wt_modified() => modified += 1,
                s if s.is_wt_new() => untracked += 1,
                _ => (),
            }
        }

        Ok(GitResponse {
            success: true,
            message: format!(
                "Status: {} staged, {} modified, {} untracked",
                staged, modified, untracked
            ),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_git_operations() -> Result<(), GitToolError> {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();
        
        let git_tool = GitTool::new(temp_dir.path().to_str().unwrap())?;
        
        // Create test file
        let test_file = temp_dir.path().join("test.txt");
        let mut file = File::create(&test_file).unwrap();
        writeln!(file, "test content").unwrap();
        
        // Test add
        let add_result = git_tool.execute(GitRequest {
            operation: GitOperation::Add,
            paths: Some(vec!["test.txt".to_string()]),
            message: None,
            author_name: None,
            author_email: None,
        })?;
        assert!(add_result.success);
        
        // Test commit
        let commit_result = git_tool.execute(GitRequest {
            operation: GitOperation::Commit,
            paths: None,
            message: Some("test: add test file".to_string()),
            author_name: Some("Test".to_string()),
            author_email: Some("test@example.com".to_string()),
        })?;
        assert!(commit_result.success);
        
        Ok(())
    }
}