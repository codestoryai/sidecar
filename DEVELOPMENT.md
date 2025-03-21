# Sidecar Development Guide

This document provides a comprehensive guide for developers who want to contribute to the Sidecar project. It covers setup, development workflows, testing, and best practices.

## Table of Contents

- [Development Environment Setup](#development-environment-setup)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Testing](#testing)
- [Debugging](#debugging)
- [Code Style and Guidelines](#code-style-and-guidelines)
- [Documentation](#documentation)
- [Common Tasks](#common-tasks)
- [Troubleshooting](#troubleshooting)

## Development Environment Setup

### Prerequisites

- **Rust**: Version 1.79 or later
- **Cargo**: Latest version compatible with your Rust installation
- **Git**: For version control
- **SQLite**: For database operations
- **Tree-sitter**: For code parsing (installed automatically via Cargo)

### Setup Steps

1. **Clone the repository**:
   ```bash
   git clone https://github.com/codestoryai/sidecar.git
   cd sidecar
   ```

2. **Install Rust dependencies**:
   ```bash
   rustup update
   rustup component add rustfmt
   ```

3. **Build the project**:
   ```bash
   cargo build
   ```

4. **Run the webserver**:
   ```bash
   cargo run --bin webserver
   ```

### Environment Variables

Sidecar uses environment variables for configuration. Here are the most important ones:

- `OPENAI_API_KEY`: Your OpenAI API key
- `ANTHROPIC_API_KEY`: Your Anthropic API key
- `GOOGLE_AI_API_KEY`: Your Google AI API key
- `SIDECAR_PORT`: Port for the webserver (default: 3000)
- `SIDECAR_HOST`: Host for the webserver (default: 127.0.0.1)
- `SIDECAR_LOG_LEVEL`: Log level (default: info)

You can set these variables in your shell or create a `.env` file in the project root.

## Project Structure

Sidecar is organized as a Rust workspace with multiple crates:

```
sidecar/               # Main crate with core functionality
├── src/               # Source code
│   ├── agentic/       # AI agent system
│   ├── agent/         # Agent implementation
│   ├── application/   # Application core
│   ├── bin/           # Binary entry points
│   ├── chunking/      # Code chunking and parsing
│   ├── git/           # Git integration
│   ├── llm/           # LLM integration
│   ├── mcts/          # Monte Carlo Tree Search
│   ├── repo/          # Repository management
│   ├── repomap/       # Repository mapping
│   ├── webserver/     # Web API
│   └── lib.rs         # Library entry point
├── Cargo.toml         # Crate manifest
└── build.rs           # Build script

llm_client/            # LLM client crate
├── src/               # Source code
│   ├── clients/       # LLM provider clients
│   ├── format/        # Request/response formatting
│   ├── tokenizer/     # Token counting and management
│   └── lib.rs         # Library entry point
└── Cargo.toml         # Crate manifest

llm_prompts/           # LLM prompt generation crate
├── src/               # Source code
│   ├── chat/          # Chat prompt generation
│   ├── fim/           # Fill-in-middle prompt generation
│   ├── in_line_edit/  # Inline editing prompt generation
│   └── lib.rs         # Library entry point
└── Cargo.toml         # Crate manifest

logging/               # Logging utilities crate
├── src/               # Source code
│   └── lib.rs         # Library entry point
└── Cargo.toml         # Crate manifest
```

## Development Workflow

### Feature Development

1. **Create a new branch**:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Implement your changes**:
   - Make the necessary code changes
   - Add tests for your changes
   - Update documentation as needed

3. **Run tests**:
   ```bash
   cargo test
   ```

4. **Format your code**:
   ```bash
   cargo fmt
   ```

5. **Commit your changes**:
   ```bash
   git add .
   git commit -m "Add your feature description"
   ```

6. **Push your branch**:
   ```bash
   git push origin feature/your-feature-name
   ```

7. **Create a pull request**:
   - Go to the GitHub repository
   - Click on "Pull Requests" and then "New Pull Request"
   - Select your branch and provide a description of your changes

### Code Review Process

1. **Automated checks**:
   - CI will run tests and linting on your PR
   - Address any issues reported by CI

2. **Peer review**:
   - A maintainer will review your code
   - Address any feedback from the review

3. **Approval and merge**:
   - Once approved, your PR will be merged
   - The branch will be deleted after merging

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p sidecar

# Run tests for a specific module
cargo test -p sidecar -- agentic::symbol

# Run a specific test
cargo test -p sidecar -- agentic::symbol::test_symbol_manager
```

### Writing Tests

Tests should be placed in the same file as the code they're testing, using the `#[cfg(test)]` attribute:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_something() {
        // Test code here
        assert_eq!(2 + 2, 4);
    }
}
```

For integration tests, create files in the `tests/` directory of the respective crate.

### Test Coverage

You can generate test coverage reports using `cargo-tarpaulin`:

```bash
cargo install cargo-tarpaulin
cargo tarpaulin -p sidecar
```

## Debugging

### Logging

Sidecar uses the `tracing` crate for logging. You can control the log level using the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug cargo run --bin webserver
```

Log levels from most to least verbose: `trace`, `debug`, `info`, `warn`, `error`.

### Debugging with VS Code

1. Install the [Rust Analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer) extension
2. Create a `.vscode/launch.json` file with the following content:

```json
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "lldb",
            "request": "launch",
            "name": "Debug webserver",
            "cargo": {
                "args": ["build", "--bin=webserver", "--package=sidecar"],
                "filter": {
                    "name": "webserver",
                    "kind": "bin"
                }
            },
            "args": [],
            "cwd": "${workspaceFolder}"
        }
    ]
}
```

3. Set breakpoints in your code
4. Press F5 to start debugging

## Code Style and Guidelines

### Formatting

Sidecar uses `rustfmt` for code formatting. Format your code before committing:

```bash
cargo fmt
```

### Naming Conventions

- **Types** (structs, enums, traits): `PascalCase`
- **Variables and functions**: `snake_case`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Modules**: `snake_case`

### Documentation

All public items should be documented using rustdoc comments:

```rust
/// This function does something useful.
///
/// # Arguments
///
/// * `arg1` - The first argument
/// * `arg2` - The second argument
///
/// # Returns
///
/// A result containing the output or an error
///
/// # Examples
///
/// ```
/// let result = do_something(1, 2);
/// assert_eq!(result, Ok(3));
/// ```
pub fn do_something(arg1: i32, arg2: i32) -> Result<i32, Error> {
    // Implementation
}
```

### Error Handling

Use the `anyhow` crate for error handling in most cases. For library code that needs to define specific error types, use `thiserror`.

```rust
// Using anyhow
use anyhow::{Result, Context};

fn do_something() -> Result<()> {
    let file = std::fs::File::open("file.txt")
        .context("Failed to open file.txt")?;
    // More code...
    Ok(())
}

// Using thiserror
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid value: {0}")]
    InvalidValue(String),
}
```

## Documentation

### Generating Documentation

You can generate and view the API documentation locally:

```bash
cargo doc --open
```

### Writing Documentation

- Document all public items (functions, structs, enums, traits)
- Include examples where appropriate
- Explain the purpose and behavior of each item
- Document error conditions and return values

## Common Tasks

### Adding a New LLM Provider

1. Create a new file in `llm_client/src/clients/` for your provider
2. Implement the `LLMClient` trait for your provider
3. Add your provider to the `LLMType` enum in `llm_client/src/clients/types.rs`
4. Register your provider in the `LLMBroker::new()` method in `llm_client/src/broker.rs`

### Adding a New Language Parser

1. Add the tree-sitter grammar dependency to `sidecar/Cargo.toml`
2. Create a new file in `sidecar/src/chunking/` for your language
3. Implement the parsing logic for your language
4. Register your language in `sidecar/src/chunking/languages.rs`

### Adding a New Tool

1. Create a new file in `sidecar/src/agentic/tool/` for your tool
2. Implement the `Tool` trait for your tool
3. Register your tool in the `ToolBox::new()` method in `sidecar/src/agentic/symbol/tool_box.rs`

## Troubleshooting

### Common Issues

#### Build Failures

- **Missing dependencies**: Make sure you have all required system dependencies installed
- **Incompatible Rust version**: Ensure you're using Rust 1.79 or later
- **Cargo.lock conflicts**: Try running `cargo clean` and then `cargo build`

#### Runtime Errors

- **API key issues**: Check that you've set the required API keys as environment variables
- **Port conflicts**: If the port is already in use, change it using the `SIDECAR_PORT` environment variable
- **Database errors**: Check that SQLite is installed and working correctly

### Getting Help

If you're stuck, you can get help from the community:

- **GitHub Issues**: Search existing issues or create a new one
- **Discord**: Join our [Discord server](https://discord.gg/mtgrhXM5Xf) for real-time help

## Conclusion

This development guide should help you get started with contributing to Sidecar. If you have any questions or suggestions for improving this guide, please open an issue or pull request.