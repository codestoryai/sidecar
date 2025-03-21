//! Utility functions for extracting operation IDs from PR content

/// Extracts operation ID from PR description or title
/// Returns None if no operation ID is found
pub fn extract_operation_id(content: &str) -> Option<String> {
    // Common patterns for operation IDs in PR titles or descriptions
    let patterns = [
        // Look for explicit operation ID format: "operation_id: XXX" or "operationId: XXX"
        r"operation[_\s]?[iI]d:\s*([a-zA-Z0-9_-]+)",
        // Look for operation ID in brackets: [XXX]
        r"\[([a-zA-Z0-9_-]+)\]",
        // Look for operation ID in parentheses: (XXX)
        r"\(([a-zA-Z0-9_-]+)\)",
        // Look for operation ID with prefix: op-XXX
        r"op-([a-zA-Z0-9_-]+)",
    ];

    for pattern in patterns {
        if let Some(captures) = regex::Regex::new(pattern).ok()?.captures(content) {
            if let Some(id) = captures.get(1) {
                return Some(id.as_str().to_string());
            }
        }
    }

    None
}