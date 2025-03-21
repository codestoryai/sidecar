# LARP Knowledge Graph

This document provides a comprehensive knowledge graph of the LARP codebase, showing the relationships between different components and modules.

## Repository Structure

```mermaid
graph TD
    Root["/ (Root)"] --> LARP["larp/"]  
    Root --> LLMClient["llm_client/"]  
    Root --> LLMPrompts["llm_prompts/"]  
    Root --> Logging["logging/"]  
    
    LARP --> SrcLARP["src/"]  
    SrcLARP --> Webserver["webserver/"]  
    SrcLARP --> MCTS["mcts/"]  
    SrcLARP --> Agentic["agentic/"]  
    SrcLARP --> Repomap["repomap/"]  
    SrcLARP --> LLM["llm/"]  
    SrcLARP --> Repo["repo/"]  
    SrcLARP --> Chunking["chunking/"]  
    SrcLARP --> Agent["agent/"]  
    SrcLARP --> Git["git/"]  
    SrcLARP --> Bin["bin/"]  
    
    LLMClient --> SrcLLM["src/"]  
    SrcLLM --> Clients["clients/"]  
    SrcLLM --> Format["format/"]  
    SrcLLM --> Tokenizer["tokenizer/"]  
    
    LLMPrompts --> SrcPrompts["src/"]  
    SrcPrompts --> FIM["fim/"]  
    SrcPrompts --> Chat["chat/"]  
    SrcPrompts --> InLineEdit["in_line_edit/"]  
    
    Logging --> SrcLogging["src/"]  
```

## Crate Dependencies

```mermaid
graph TD
    LARP["larp"] --> LLMClient["llm_client"] 
    LARP --> LLMPrompts["llm_prompts"]
    LARP --> Logging["logging"]
    LLMPrompts --> LLMClient
    Logging --> LLMClient
```

## Core Components and Their Relationships

```mermaid
flowchart TD
    subgraph Application
        App["Application Core"] --> Config["Configuration"]
        App --> RepoPool["Repository Pool"]
        App --> LangParsing["Language Parsing"]
        App --> LLMBroker["LLM Broker"]
        App --> SymbolMgr["Symbol Manager"]
        App --> ToolBox["Tool Box"]
    end
    
    subgraph WebServer
        Server["Web Server"] --> AgenticRouter["Agentic Router"]
        Server --> PlanRouter["Plan Router"]
        Server --> TreeSitterRouter["Tree Sitter Router"]
        Server --> FileOpsRouter["File Operations Router"]
    end
    
    subgraph Agentic
        SymbolMgr --> ToolBroker["Tool Broker"]
        ToolBox --> ToolBroker
        ToolBroker --> CodeEditBroker["Code Edit Broker"]
        SymbolMgr --> SymbolTracker["Symbol Tracker"]
        ToolBox --> SymbolTracker
        SymbolMgr --> EditorParsing["Editor Parsing"]
        ToolBox --> EditorParsing
    end
    
    subgraph LLM
        LLMBroker --> Clients["LLM Clients"]
        Clients --> OpenAI["OpenAI"]
        Clients --> Anthropic["Anthropic"]
        Clients --> GoogleAI["Google AI"]
        Clients --> Ollama["Ollama"]
        Clients --> Others["Other Providers"]
    end
    
    App --> Server
    SymbolMgr --> LLMBroker
    ToolBroker --> LLMBroker
```

## Detailed Module Relationships

### Webserver Module

```mermaid
classDiagram
    class Webserver {
        +start(app: Application)
        +run(app: Application)
    }
    
    class AgenticRouter {
        +probe_request_stop()
        +code_sculpting()
        +push_diagnostics()
        +agent_session_chat()
        +agent_session_edit_anchored()
        +agent_session_edit_agentic()
        +agent_session_plan()
        +agent_tool_use()
    }
    
    class TreeSitterRouter {
        +extract_documentation_strings()
        +extract_diagnostics_range()
        +tree_sitter_node_check()
        +check_valid_xml()
    }
    
    class FileOperationsRouter {
        +file_edit()
    }
    
    Webserver --> AgenticRouter
    Webserver --> TreeSitterRouter
    Webserver --> FileOperationsRouter
```

### Agentic Module

```mermaid
classDiagram
    class SymbolManager {
        +tool_broker: ToolBroker
        +symbol_tracker: SymbolTrackerInline
        +editor_parsing: EditorParsing
        +llm_properties: LLMProperties
    }
    
    class ToolBox {
        +tool_broker: ToolBroker
        +symbol_tracker: SymbolTrackerInline
        +editor_parsing: EditorParsing
    }
    
    class ToolBroker {
        +llm_broker: LLMBroker
        +code_edit_broker: CodeEditBroker
        +symbol_tracker: SymbolTrackerInline
        +language_parsing: TSLanguageParsing
        +configuration: ToolBrokerConfiguration
        +llm_properties: LLMProperties
    }
    
    class Memory {
        +store(key, value)
        +retrieve(key)
        +list_keys()
    }
    
    SymbolManager --> ToolBroker
    ToolBox --> ToolBroker
    SymbolManager --> Memory
```

### MCTS (Monte Carlo Tree Search) Module

```mermaid
classDiagram
    class ActionNode {
        +id: String
        +parent_id: Option<String>
        +children: Vec<String>
        +action: Action
        +state: ActionState
        +visits: u64
        +value: f64
    }
    
    class Selector {
        +select_node(nodes: &[ActionNode])
    }
    
    class Decider {
        +decide(nodes: &[ActionNode])
    }
    
    class Execution {
        +execute(action: Action)
    }
    
    class Feedback {
        +evaluate(action: Action, result: ActionResult)
    }
    
    class ValueFunction {
        +calculate(node: &ActionNode)
    }
    
    ActionNode --> Selector
    Selector --> Decider
    Decider --> Execution
    Execution --> Feedback
    Feedback --> ValueFunction
    ValueFunction --> ActionNode
```

### Repository Mapping Module

```mermaid
classDiagram
    class Analyser {
        +analyze_repository(repo_path: &Path)
    }
    
    class Graph {
        +add_node(node: Node)
        +add_edge(from: NodeId, to: NodeId)
        +calculate_page_rank()
    }
    
    class TreeWalker {
        +walk(root: &Path)
    }
    
    class Files {
        +list_files(root: &Path)
        +filter_files(files: &[PathBuf], ignore_patterns: &[String])
    }
    
    class TreeContext {
        +get_context(node: &Node, depth: usize)
    }
    
    Analyser --> Graph
    Analyser --> TreeWalker
    TreeWalker --> Files
    Graph --> TreeContext
```

### LLM Client Module

```mermaid
classDiagram
    class LLMBroker {
        +clients: Map<LLMType, LLMClient>
        +get_client(llm_type: LLMType)
        +generate_completion(prompt: String, llm_type: LLMType)
        +generate_chat_completion(messages: Vec<Message>, llm_type: LLMType)
    }
    
    class LLMClient {
        +generate_completion(prompt: String)
        +generate_chat_completion(messages: Vec<Message>)
        +stream_chat_completion(messages: Vec<Message>)
    }
    
    class OpenAIClient {
        +api_key: String
        +model: String
    }
    
    class AnthropicClient {
        +api_key: String
        +model: String
    }
    
    class GoogleAIClient {
        +api_key: String
        +model: String
    }
    
    class LLMTokenizer {
        +count_tokens(text: &str, model: &str)
        +truncate_text(text: &str, max_tokens: usize, model: &str)
    }
    
    LLMBroker --> LLMClient
    LLMClient <|-- OpenAIClient
    LLMClient <|-- AnthropicClient
    LLMClient <|-- GoogleAIClient
    LLMBroker --> LLMTokenizer
```

### Code Chunking Module

```mermaid
classDiagram
    class TSLanguageParsing {
        +parse_file(file_path: &Path, language: Language)
        +get_symbols(parsed: &Tree)
    }
    
    class EditorParsing {
        +parse_text(text: &str, language: Language)
        +get_symbols_at_position(parsed: &Tree, position: Position)
    }
    
    class FileContent {
        +path: PathBuf
        +content: String
        +language: Language
    }
    
    class TextDocument {
        +uri: String
        +version: i32
        +content: String
        +language_id: String
    }
    
    class ScopeGraph {
        +add_scope(scope: Scope)
        +connect_scopes(parent: ScopeId, child: ScopeId)
        +get_scope_at_position(position: Position)
    }
    
    TSLanguageParsing --> FileContent
    EditorParsing --> TextDocument
    TSLanguageParsing --> ScopeGraph
    EditorParsing --> ScopeGraph
```

## Data Flow Diagrams

### Request Processing Flow

```mermaid
sequenceDiagram
    participant Editor as Aide Editor
    participant API as Webserver API
    participant App as Application Core
    participant LLM as LLM Client
    participant Repo as Repository Analysis
    participant Agent as Agentic Tools
    
    Editor->>API: Request (code context, query)
    API->>App: Process request
    App->>Repo: Analyze repository context
    Repo-->>App: Repository context
    App->>Agent: Select appropriate tools
    Agent->>LLM: Generate prompt with context
    LLM-->>Agent: LLM response
    Agent->>App: Process LLM response
    App->>API: Formatted response
    API->>Editor: Display results to user
```

### Code Editing Flow

```mermaid
sequenceDiagram
    participant Editor as Aide Editor
    participant API as Webserver API
    participant Agent as Agentic System
    participant MCTS as MCTS Engine
    participant LLM as LLM Client
    participant FS as File System
    
    Editor->>API: Edit request with code context
    API->>Agent: Process edit request
    Agent->>MCTS: Generate possible edits
    
    loop Action Selection
        MCTS->>LLM: Generate candidate actions
        LLM-->>MCTS: Candidate actions
        MCTS->>MCTS: Evaluate actions
        MCTS->>MCTS: Select best action
    end
    
    MCTS-->>Agent: Best edit action
    Agent->>FS: Apply edit to file
    FS-->>Agent: Edit result
    Agent->>API: Edit response
    API->>Editor: Updated code
```

### Symbol Analysis Flow

```mermaid
sequenceDiagram
    participant Editor as Aide Editor
    participant API as Webserver API
    participant SymbolMgr as Symbol Manager
    participant Parser as Language Parser
    participant LLM as LLM Client
    
    Editor->>API: Symbol analysis request
    API->>SymbolMgr: Process symbol request
    SymbolMgr->>Parser: Parse code for symbols
    Parser-->>SymbolMgr: Symbol tree
    SymbolMgr->>LLM: Generate symbol analysis
    LLM-->>SymbolMgr: Symbol insights
    SymbolMgr->>API: Symbol analysis response
    API->>Editor: Display symbol insights
```

## Key Concepts and Abstractions

```mermaid
mindmap
  root((LARP))
    Application
      Configuration
      Repository Pool
      Language Parsing
    AI Components
      LLM Integration
        OpenAI
        Anthropic
        Google AI
        Others
      Agentic System
        Tool Box
        Symbol Manager
        Memory
      MCTS
        Action Nodes
        Selection Strategy
        Execution
    Code Understanding
      Repository Mapping
        Page Rank
        Symbol Graph
      Code Chunking
        Language Parsers
        Symbol Extraction
      Tree-sitter Integration
    Web Interface
      API Endpoints
      Request Handling
      Response Streaming
```

## Feature Dependency Graph

```mermaid
graph TD
    CodeEditing["Code Editing"] --> SymbolAnalysis["Symbol Analysis"]
    CodeEditing --> LanguageParsing["Language Parsing"]
    CodeEditing --> LLMIntegration["LLM Integration"]
    
    SymbolAnalysis --> RepositoryMapping["Repository Mapping"]
    SymbolAnalysis --> TreeSitter["Tree-sitter Parsing"]
    
    AgentChat["Agent Chat"] --> LLMIntegration
    AgentChat --> ContextGathering["Context Gathering"]
    
    ContextGathering --> RepositoryMapping
    ContextGathering --> FileAnalysis["File Analysis"]
    
    MCTSDecision["MCTS Decision Making"] --> LLMIntegration
    MCTSDecision --> ToolExecution["Tool Execution"]
    
    ToolExecution --> FileSystem["File System Access"]
    ToolExecution --> GitIntegration["Git Integration"]
    
    InlineCompletion["Inline Completion"] --> LanguageParsing
    InlineCompletion --> LLMIntegration
    
    LanguageParsing --> TreeSitter
```

## Conclusion

This knowledge graph provides a comprehensive view of the LARP codebase structure and relationships between different components. It should help developers understand how the various parts of the system interact and how data flows through the application.
