# Agent REPL

A REPL-like CLI tool for interacting with an AI agent that can analyze and modify code repositories.

## Features

- Point the agent to any repository
- Run queries against the repository
- Watch the agent's thought process and tool usage in real-time
- Track token usage
- Monitor files opened and edited
- Provide feedback to the agent
- Stop the agent at any point
- Set timeout for agent operations
- Select different LLM models

## Installation

```bash
cargo build --release
```

The binary will be available at `target/release/agent_repl`.

## Usage

```bash
# Run with a repository path
agent_repl --repo-path /path/to/repository --api-key your_api_key --timeout 300 --model claude-sonnet

# Or set these values in the REPL
agent_repl
```

## REPL Commands

- `repo <path>` - Set the repository path
- `key <api_key>` - Set the API key
- `timeout <seconds>` - Set the timeout in seconds (default: 300)
- `model <model_name>` - Set the LLM model to use
- `run <query>` - Run the agent with the given query
- `stop` - Stop the agent
- `feedback <message>` - Provide feedback to the agent
- `status` - Show the current agent status
- `help` - Show the help message
- `exit` - Exit the REPL

## Available Models

- `claude-sonnet` - Claude Sonnet model from Anthropic
- `claude-haiku` - Claude Haiku model from Anthropic
- `claude-opus` - Claude Opus model from Anthropic
- `gpt-4` - GPT-4 model from OpenAI
- `gpt-4o` - GPT-4o model from OpenAI
- `gemini-pro` - Gemini Pro model from Google
- Custom model names can also be provided

## Example

```
$ agent_repl
Welcome to the Agent REPL!
Type 'help' for a list of commands, 'exit' to quit
agent> repo /path/to/repository
Repository path set to: /path/to/repository
agent> key your_api_key
API key set
agent> timeout 600
Timeout set to: 600s
agent> model claude-sonnet
LLM model set to: claude-sonnet
agent> run Add error handling to the main function
Using tool: ListFiles
Thinking: I need to understand the repository structure first. Let me list the files.
Tool result: /path/to/repository/src/main.rs
/path/to/repository/src/lib.rs
/path/to/repository/Cargo.toml

Using tool: SearchFileContentWithRegex
Thinking: Now I need to find files that might be relevant to the query.
Tool result: /path/to/repository/src/main.rs:10: fn main() {
/path/to/repository/src/main.rs:11:     let result = do_something();
/path/to/repository/src/main.rs:12:     println!("Result: {}", result);
/path/to/repository/src/main.rs:13: }

Token usage: 300 tokens (total: 300)
...
```

## How It Works

The agent follows these steps:

1. Analyzes the repository structure
2. Identifies relevant files
3. Reads and understands the code
4. Makes necessary changes
5. Verifies the changes work as expected
6. Provides a summary of what was done

This process mirrors the agent loop in the sidecar codebase, where the agent repeatedly:
- Selects the next tool to use
- Executes the tool
- Processes the result
- Continues until the task is complete

## Implementation Details

This tool integrates with the sidecar codebase to leverage its agent loop implementation:

- Uses the `LLMBroker` from the llm_client crate to interact with various LLM providers
- Uses the `ToolType` enum from the sidecar crate to ensure compatibility
- Implements timeout settings to prevent the agent from running indefinitely
- Supports multiple LLM models through a unified interface