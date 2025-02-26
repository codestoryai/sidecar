# Git Tool Integration Guide

This guide explains how to integrate the git tool into the Sidecar agent's tool system.

## Integration Steps

1. Add the git tool to the agent's tool registry:

```rust
// src/tools/mod.rs
mod git_tool;
use git_tool::GitTool;

pub struct ToolRegistry {
    git_tool: GitTool,
    // other tools...
}

impl ToolRegistry {
    pub fn new(repo_path: &str) -> Result<Self, Error> {
        Ok(Self {
            git_tool: GitTool::new(repo_path)?,
            // other tools...
        })
    }
}
```

2. Add tool XML schema:

```rust
// src/tools/schema.rs
#[derive(Deserialize)]
pub struct GitToolInput {
    operation: String,
    paths: Option<Vec<String>>,
    message: Option<String>,
    author_name: Option<String>,
    author_email: Option<String>,
}
```

3. Implement tool handler:

```rust
// src/tools/handlers.rs
pub async fn handle_git_operation(input: GitToolInput) -> Result<String> {
    let request = GitRequest {
        operation: match input.operation.as_str() {
            "add" => GitOperation::Add,
            "commit" => GitOperation::Commit,
            "status" => GitOperation::Status,
            _ => return Err(Error::InvalidOperation),
        },
        paths: input.paths,
        message: input.message,
        author_name: input.author_name,
        author_email: input.author_email,
    };

    let response = TOOL_REGISTRY.git_tool.execute(request)?;
    Ok(serde_json::to_string(&response)?)
}
```

## Usage in Agent Prompts

The git tool can be used in agent prompts like this:

1. Adding files:
```xml
<git_operation>
<operation>add</operation>
<paths>
["src/new_feature.rs", "tests/new_feature_test.rs"]
</paths>
</git_operation>
```

2. Creating commits:
```xml
<git_operation>
<operation>commit</operation>
<message>feat: implement new authentication system</message>
</git_operation>
```

## Error Handling

The tool provides detailed error messages that should be handled appropriately:

```rust
match git_tool.execute(request) {
    Ok(response) => {
        // Handle successful response
    },
    Err(GitToolError::MissingField(field)) => {
        // Handle missing required fields
    },
    Err(GitToolError::Git(err)) => {
        // Handle git operation errors
    },
    Err(GitToolError::InvalidOperation(op)) => {
        // Handle invalid operation errors
    }
}
```

## Best Practices

1. Always validate input parameters before executing git operations
2. Use conventional commits format for commit messages
3. Group related file changes into single commits
4. Handle errors gracefully and provide clear error messages
5. Clean up resources after operations complete

## Security Considerations

1. Validate file paths to prevent path traversal
2. Sanitize commit messages and author information
3. Implement proper access controls
4. Log all git operations for audit purposes
5. Handle sensitive data in repositories carefully