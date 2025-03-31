use crate::agentic::tool::errors::ToolError;
use llm_client::broker::MCPTool;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
}

pub async fn discover_resources(tools: &HashMap<String, Box<dyn MCPTool>>) -> Result<Vec<ResourceInfo>, ToolError> {
    let mut resources = Vec::new();

    for tool in tools.values() {
        let tool_resources = tool.list_resources();
        for resource in tool_resources {
            resources.push(ResourceInfo {
                uri: resource.uri,
                name: resource.name,
                description: resource.description,
            });
        }
    }

    Ok(resources)
}

pub fn format_resource_info(resources: &[ResourceInfo]) -> String {
    if resources.is_empty() {
        return "No resources available.".to_string();
    }

    let mut output = String::from("Available Resources:\n");
    for resource in resources {
        output.push_str(&format!("- URI: {}\n  Name: {}\n", resource.uri, resource.name));
        if let Some(desc) = &resource.description {
            output.push_str(&format!("  Description: {}\n", desc));
        }
        output.push('\n');
    }

    output
}