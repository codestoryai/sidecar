//! The thinking tool allows the LLM to log a thought for itself
//! This can be extremely useful when forcing the agent to think explicitly

/// Helps with logging the thought from the LLM and nothing more than that
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThinkingPartialInput {
    thought: String,
}

impl ThinkingPartialInput {
    pub fn thought(&self) -> &str {
        &self.thought
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<thought>
{}
</thought>"#,
            self.thought
        )
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "Think",
            "description": r#"Use the tool to think about something. It will not obtain new information or make any changes to the repository, but just log the thought. Use it when complex reasoning or brainstorming is needed.

Common use cases:
1. When exploring a repository and discovering the source of a bug, call this tool to brainstorm several unique ways of fixing the bug, and assess which change(s) are likely to be simplest and most effective
2. After receiving test results, use this tool to brainstorm ways to fix failing tests
3. When planning a complex refactoring, use this tool to outline different approaches and their tradeoffs
4. When designing a new feature, use this tool to think through architecture decisions and implementation details
5. When debugging a complex issue, use this tool to organize your thoughts and hypotheses

The tool simply logs your thought process for better transparency and does not execute any code or make changes."#,
            "input_schema": {
                "type": "object",
                "properties": {
                    "thought": {
                        "type": "string",
                        "description": "(required) Your thoughts."
                    }
                },
                "required": ["thought"],
            },
        })
    }
}
