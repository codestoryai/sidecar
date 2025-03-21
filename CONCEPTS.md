# Sidecar Key Concepts and Terminology

This document explains the key concepts and terminology used in the Sidecar project. Understanding these concepts will help you navigate the codebase and contribute effectively.

## Table of Contents

- [AI Concepts](#ai-concepts)
- [Code Understanding](#code-understanding)
- [System Architecture](#system-architecture)
- [Development Concepts](#development-concepts)
- [Integration Concepts](#integration-concepts)

## AI Concepts

### Large Language Models (LLMs)

**Definition**: Large Language Models are AI models trained on vast amounts of text data that can generate human-like text, understand context, and perform various language tasks.

**In Sidecar**: LLMs are used for code understanding, generation, and editing. Sidecar supports multiple LLM providers, including OpenAI, Anthropic, and Google AI.

**Key Components**:
- `llm_client` crate: Handles communication with LLM providers
- `LLMBroker`: Manages different LLM clients and routes requests
- `LLMClient` trait: Interface for different LLM providers

### Agentic System

**Definition**: An agentic system is an AI system that can take actions autonomously to achieve specific goals. It can use tools, make decisions, and interact with its environment.

**In Sidecar**: The agentic system allows AI to perform complex code operations, such as editing, refactoring, and analyzing code.

**Key Components**:
- `agentic` module: Contains the core agentic functionality
- `ToolBox`: Collection of tools available to the agent
- `SymbolManager`: Manages code symbols and their relationships
- `Memory`: Stores context and information for the agent

### Monte Carlo Tree Search (MCTS)

**Definition**: MCTS is a heuristic search algorithm for decision processes, particularly useful for complex decision spaces where evaluating all possibilities is impractical.

**In Sidecar**: MCTS is used to explore possible code changes and select the most promising ones for implementation.

**Key Components**:
- `mcts` module: Contains the MCTS implementation
- `ActionNode`: Represents a node in the MCTS tree
- `Selector`: Selects nodes for exploration
- `ValueFunction`: Evaluates the value of nodes

### Tool-Based Approach

**Definition**: A tool-based approach allows an AI agent to use specific tools to interact with its environment and accomplish tasks.

**In Sidecar**: Tools are used by the agent to perform specific operations, such as editing code, searching repositories, and analyzing symbols.

**Key Components**:
- `tool` module: Contains tool implementations
- `Tool` trait: Interface for different tools
- `ToolBroker`: Manages tool selection and execution

## Code Understanding

### Symbol

**Definition**: A symbol is a named entity in code, such as a function, class, variable, or module.

**In Sidecar**: Symbols are the basic units of code understanding. Sidecar extracts symbols from code and analyzes their relationships.

**Key Components**:
- `symbol` module: Contains symbol-related functionality
- `SymbolManager`: Manages symbols and their relationships
- `SymbolTrackerInline`: Tracks symbols in editor sessions

### Repository Mapping

**Definition**: Repository mapping is the process of analyzing a code repository to understand its structure, dependencies, and important components.

**In Sidecar**: Repository mapping is used to build a graph representation of the codebase, which helps in understanding context and relationships.

**Key Components**:
- `repomap` module: Contains repository mapping functionality
- `Analyser`: Analyzes repositories and builds graphs
- `Graph`: Represents the repository as a graph
- `PageRank`: Calculates importance scores for symbols

### Code Chunking

**Definition**: Code chunking is the process of breaking down code into meaningful segments for analysis and understanding.

**In Sidecar**: Code chunking is used to parse and analyze code in a structured way, extracting symbols and their relationships.

**Key Components**:
- `chunking` module: Contains code chunking functionality
- `TSLanguageParsing`: Uses Tree-sitter for language parsing
- `EditorParsing`: Parses code in editor sessions
- Language-specific parsers (e.g., `rust.rs`, `python.rs`)

### Tree-sitter

**Definition**: Tree-sitter is a parser generator tool and incremental parsing library that can build a concrete syntax tree for source code.

**In Sidecar**: Tree-sitter is used for accurate and efficient parsing of multiple programming languages.

**Key Components**:
- `tree-sitter` dependency: The core parsing library
- Language-specific grammars (e.g., `tree-sitter-rust`, `tree-sitter-python`)
- `TSLanguageParsing`: Wrapper for Tree-sitter functionality

## System Architecture

### Application Core

**Definition**: The application core is the central component that coordinates all other parts of the system.

**In Sidecar**: The application core initializes and manages the various subsystems, handles configuration, and provides a unified interface for the webserver.

**Key Components**:
- `application` module: Contains the application core
- `Application` struct: Main application class
- `Configuration` struct: Application configuration

### Webserver

**Definition**: The webserver provides HTTP endpoints for external systems to communicate with Sidecar.

**In Sidecar**: The webserver handles requests from the Aide editor, routes them to the appropriate components, and returns responses.

**Key Components**:
- `webserver` module: Contains the webserver implementation
- `webserver.rs` binary: Main entry point for the webserver
- Various router functions (e.g., `agentic_router`, `tree_sitter_router`)

### Repository Pool

**Definition**: The repository pool manages access to code repositories and their state.

**In Sidecar**: The repository pool provides a unified interface for accessing and manipulating repositories.

**Key Components**:
- `repo` module: Contains repository-related functionality
- `RepositoryPool` struct: Manages repository access
- `state` module: Manages repository state

### LLM Broker

**Definition**: The LLM broker manages communication with different LLM providers and routes requests to the appropriate client.

**In Sidecar**: The LLM broker provides a unified interface for generating text, chat completions, and other LLM operations.

**Key Components**:
- `LLMBroker` struct: Main broker class
- `LLMClient` trait: Interface for LLM providers
- Provider-specific clients (e.g., `OpenAIClient`, `AnthropicClient`)

## Development Concepts

### Rust Workspace

**Definition**: A Rust workspace is a collection of packages that share dependencies and configuration.

**In Sidecar**: Sidecar is organized as a Rust workspace with multiple crates, each responsible for a specific aspect of functionality.

**Key Components**:
- `Cargo.toml` at the root: Defines the workspace
- Individual crates: `sidecar`, `llm_client`, `llm_prompts`, `logging`
- Shared dependencies and configuration

### Asynchronous Programming

**Definition**: Asynchronous programming allows operations to run concurrently without blocking the main thread.

**In Sidecar**: Asynchronous programming is used extensively for handling concurrent requests, I/O operations, and long-running tasks.

**Key Components**:
- `tokio` dependency: Asynchronous runtime
- `async`/`await` syntax: Used for asynchronous functions
- `Future` trait: Represents asynchronous computations

### Error Handling

**Definition**: Error handling is the process of managing and responding to error conditions in a program.

**In Sidecar**: Error handling is done using the `anyhow` and `thiserror` crates, which provide flexible and ergonomic error handling.

**Key Components**:
- `anyhow` dependency: For general error handling
- `thiserror` dependency: For defining specific error types
- `Result` type: Used for functions that can fail

### Tracing and Logging

**Definition**: Tracing and logging are techniques for recording information about a program's execution for debugging and monitoring.

**In Sidecar**: Tracing and logging are used to record information about the system's operation, errors, and performance.

**Key Components**:
- `tracing` dependency: For structured logging and tracing
- `logging` crate: Custom logging utilities
- `tracing_subscriber`: For configuring tracing output

## Integration Concepts

### Aide Editor Integration

**Definition**: Aide is a code editor that integrates with Sidecar for AI-powered code assistance.

**In Sidecar**: Sidecar provides API endpoints for the Aide editor to request AI assistance and receive responses.

**Key Components**:
- Webserver API endpoints
- Request and response formats
- Streaming response support

### LLM Provider Integration

**Definition**: LLM providers are services that offer access to large language models through APIs.

**In Sidecar**: Sidecar integrates with multiple LLM providers to leverage their models for code understanding and generation.

**Key Components**:
- Provider-specific clients
- API key management
- Request and response formatting

### Git Integration

**Definition**: Git is a distributed version control system used for tracking changes in source code.

**In Sidecar**: Sidecar integrates with Git to understand repository history, changes, and structure.

**Key Components**:
- `git` module: Contains Git-related functionality
- `gix` dependency: Git implementation in Rust
- Repository analysis based on Git history

## Conclusion

Understanding these key concepts and terminology will help you navigate the Sidecar codebase and contribute effectively to the project. If you encounter terms or concepts that are not explained here, please consider adding them to this document to help future contributors.