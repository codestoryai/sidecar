use std::path::PathBuf;
pub use sidecar::agentic::tool::r#type::ToolType;

/// Represents an action that the agent can take
#[derive(Debug, Clone)]
pub enum AgentAction {
    /// Use a tool with the given parameters
    UseTool {
        tool_type: ToolType,
        parameters: serde_json::Value,
        thinking: String,
    },
    /// Complete the agent's task with a final message
    Complete {
        message: String,
    },
    /// Report an error
    Error {
        message: String,
    },
}

/// Represents a response from the agent to the REPL
#[derive(Debug, Clone)]
pub enum AgentResponse {
    /// The agent is using a tool
    ToolUse {
        tool_type: ToolType,
        thinking: String,
    },
    /// The result of using a tool
    ToolResult {
        result: String,
    },
    /// Token usage information
    TokenUsage {
        usage: TokenUsage,
    },
    /// A file was opened
    FileOpened {
        path: PathBuf,
    },
    /// A file was edited
    FileEdited {
        path: PathBuf,
    },
    /// The agent has completed its task
    Completion {
        message: String,
    },
    /// An error occurred
    Error {
        message: String,
    },
}

/// Represents token usage information
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    input_tokens: usize,
    output_tokens: usize,
}

impl TokenUsage {
    /// Create a new TokenUsage
    pub fn new(input_tokens: usize, output_tokens: usize) -> Self {
        Self {
            input_tokens,
            output_tokens,
        }
    }
    
    /// Get the total number of tokens used
    pub fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
    
    /// Get the number of input tokens used
    pub fn input_tokens(&self) -> usize {
        self.input_tokens
    }
    
    /// Get the number of output tokens used
    pub fn output_tokens(&self) -> usize {
        self.output_tokens
    }
    
    /// Add another TokenUsage to this one
    pub fn add(&mut self, other: TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }
}