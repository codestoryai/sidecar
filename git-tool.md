# Git Tool for Sidecar Agent

This document describes the git tool that enables the agent to perform git operations like adding files and making commits.

## Tool: git_operation

The git_operation tool allows the agent to perform git operations on the repository.

### Parameters

- operation: (required) The git operation to perform. Valid values:
  - "add": Stage files for commit
  - "commit": Create a commit with staged changes
  - "status": Get repository status
  
- paths: (required for "add") Array of file paths to stage
- message: (required for "commit") Commit message that follows conventional commits format
- author_name: (optional) Author name for commit
- author_email: (optional) Author email for commit

### Usage Examples

1. Adding files:
```xml
<git_operation>
<operation>add</operation>
<paths>
["src/main.rs", "docs/README.md"]
</paths>
</git_operation>
```

2. Creating a commit:
```xml
<git_operation>
<operation>commit</operation>
<message>feat: add new authentication system</message>
<author_name>Sidecar Agent</author_name>
<author_email>agent@sidecar.ai</author_email>
</git_operation>
```

3. Checking status:
```xml
<git_operation>
<operation>status</operation>
</git_operation>
```

### Response Format

The tool returns a JSON response with:
- success: Boolean indicating operation success
- message: Description of what was done
- error: Error message if operation failed

Example response:
```json
{
  "success": true,
  "message": "Successfully committed 2 files",
  "error": null
}
```

### Implementation Notes

1. The tool uses libgit2 for git operations
2. Handles relative and absolute paths
3. Validates commit message format
4. Provides detailed error messages
5. Supports atomic operations

### Best Practices

1. Always check repository status before operations
2. Use conventional commits format for messages
3. Group related files in single commits
4. Provide meaningful commit messages
5. Handle errors gracefully