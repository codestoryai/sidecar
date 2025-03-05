# Understanding the Agent Loop in Sidecar

This document explains how the agent loop works in the Sidecar codebase, specifically focusing on how it traverses functions to call into the agent and select the next action to take.

## Overview

The agent loop is implemented in the `SessionService` class, which manages the interaction between the user and the AI agent. The main entry point for the agent loop is the `tool_use_agentic` method, which sets up the initial state and then calls into the `agent_loop` method to repeatedly select and execute tools until a terminating condition is met.

## Key Components

### SessionService

The `SessionService` is responsible for managing sessions, which represent conversations between the user and the agent. It provides methods for:
- Creating new sessions
- Handling user messages
- Managing the agent loop
- Saving and loading sessions from storage

### Session

The `Session` class represents a conversation between the user and the agent. It maintains:
- A list of exchanges (messages between the user and agent)
- A list of action nodes (representing tools used by the agent)
- Project metadata (labels, repository reference)
- User context

### ToolUseAgent

The `ToolUseAgent` is responsible for determining which tool to use next based on the current state of the session. It uses an LLM to make this decision, taking into account:
- The conversation history
- Available tools
- User context
- Previous tool outputs

### Agent Loop Flow

The agent loop follows these steps:

1. **Initialization**:
   - A new session is created or loaded from storage
   - The user's message is added to the session
   - The agent loop is started

2. **Loop Execution**:
   - The `agent_loop` method is called, which repeatedly:
     - Saves the current session state to storage
     - Creates a new exchange ID for the next tool use
     - Gets the next tool to use by calling `session.get_tool_to_use()`
     - Invokes the selected tool with `session.invoke_tool()`
     - Processes the result and updates the session state
     - Continues until a terminating condition is met (like AttemptCompletion or AskFollowupQuestions)

3. **Tool Selection**:
   - The `get_tool_to_use` method in `Session` converts the session exchanges to a format the LLM can understand
   - It calls the `ToolUseAgent.invoke()` method to get the next tool to use
   - The LLM decides which tool to use based on the conversation history and available tools
   - The selected tool and its parameters are returned

4. **Tool Execution**:
   - The `invoke_tool` method in `Session` executes the selected tool with the provided parameters
   - The result is added to the session as a new exchange
   - An action node is created to track the tool use and its result

5. **Termination**:
   - The loop continues until a terminating tool is selected (AttemptCompletion or AskFollowupQuestions)
   - When a terminating tool is selected, the loop breaks and control is returned to the caller

## Code Flow

Here's the sequence of function calls that happen during the agent loop:

1. `SessionService.tool_use_agentic()` - Entry point for the agent loop
2. `SessionService.agent_loop()` - Main loop that repeatedly selects and executes tools
3. `Session.get_tool_to_use()` - Determines the next tool to use
4. `ToolUseAgent.invoke()` - Uses an LLM to decide which tool to use
5. `Session.invoke_tool()` - Executes the selected tool
6. Back to step 2 until a terminating condition is met

## Key Insights

- The agent loop is stateful, with the state maintained in the `Session` object
- The LLM is used to decide which tool to use next, based on the conversation history
- The loop continues until a terminating tool is selected
- The session state is saved to storage after each tool use
- The agent can be interrupted by the user at any point