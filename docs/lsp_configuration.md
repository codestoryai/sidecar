# Sidecar LSP Configuration Guide

## Overview
The sidecar service supports flexible Language Server Protocol (LSP) configuration through JSON configuration files. This allows you to:
- Configure language servers for different programming languages
- Set custom initialization options
- Define server-specific settings
- Manage server lifecycle and capabilities

## Configuration File
Place your configuration in `lsp_config.json`:

```json
{
  "language_servers": {
    "rust": {
      "command": "rust-analyzer",
      "args": [],
      "initialization_options": {
        "checkOnSave": true,
        "procMacro": true
      },
      "root_markers": ["Cargo.toml"],
      "capabilities": [
        "completions",
        "diagnostics",
        "formatting",
        "references",
        "definition"
      ]
    },
    "typescript": {
      "command": "typescript-language-server",
      "args": ["--stdio"],
      "initialization_options": {
        "preferences": {
          "importModuleSpecifierPreference": "relative"
        }
      },
      "root_markers": ["package.json", "tsconfig.json"],
      "capabilities": [
        "completions",
        "diagnostics",
        "formatting",
        "references"
      ]
    }
  },
  "global_settings": {
    "workspace_folders": ["src", "tests"],
    "sync_kind": "full",
    "completion_trigger_characters": [".", ":", ">"],
    "signature_trigger_characters": ["(", ","]
  }
}
```

## Configuration Options

### Language Server Configuration
Configure individual language servers:
- `command`: Executable name or path
- `args`: Command line arguments
- `initialization_options`: LSP initialization parameters
- `root_markers`: Files indicating project root
- `capabilities`: Supported LSP features

### Global Settings
Control LSP behavior across all servers:
- `workspace_folders`: Default workspace directories
- `sync_kind`: Document sync type (none/full/incremental)
- `completion_trigger_characters`: Characters triggering completion
- `signature_trigger_characters`: Characters triggering signature help

## Supported Languages

### Built-in Support
- Rust (rust-analyzer)
- TypeScript/JavaScript (typescript-language-server)
- Python (pyright)
- Go (gopls)
- Java (jdtls)
- C/C++ (clangd)
- HTML/CSS (vscode-html-language-server)
- JSON (vscode-json-language-server)
- YAML (yaml-language-server)
- PHP (intelephense)

### Custom Server Configuration
Example adding a custom language server:
```json
{
  "language_servers": {
    "custom_lang": {
      "command": "/path/to/custom-ls",
      "args": ["--custom-arg"],
      "initialization_options": {
        "customSetting": true
      },
      "root_markers": ["custom.config"],
      "capabilities": ["completions", "diagnostics"]
    }
  }
}
```

## Usage Example

1. Create configuration file:
```bash
echo '{
  "language_servers": {
    "rust": {
      "command": "rust-analyzer",
      "initialization_options": {
        "checkOnSave": true
      }
    }
  }
}' > lsp_config.json
```

2. Load configuration:
```rust
let state = LspState::new().await?;
state.load_configuration(Path::new("lsp_config.json")).await?;
```

The configuration will be applied automatically to all LSP operations.