#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ToolType {
    // ... existing tool types ...
    GitCommit,
    GitCreateBranch,
    GitPush,
    GitCreatePullRequest,
    // ... existing code ...
} 