# Sidecar Model Configuration Guide

## Overview
The sidecar service supports flexible model configuration through JSON configuration files. This allows you to:
- Enable/disable specific providers or models
- Override model parameters
- Configure custom endpoints
- Set provider-specific settings

## Configuration File
Place your configuration in `models_config.json`:

```json
{
  "config_path": "/path/to/config",
  "model_overrides": {
    "gpt-4": {
      "config": {
        "temperature": 0.7,
        "max_tokens": 4096,
        "top_p": 1.0,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0
      },
      "enabled": true,
      "endpoint": "https://custom-endpoint/v1"
    },
    "claude-3-opus": {
      "config": {
        "temperature": 0.8,
        "max_tokens": 8192
      },
      "enabled": true
    }
  },
  "enabled_providers": ["OpenAI", "Anthropic", "TogetherAI"],
  "provider_endpoints": {
    "OpenAI": "https://api.openai.com/v1",
    "Anthropic": "https://api.anthropic.com/v1"
  }
}
```

## Configuration Options

### Model Overrides
Override settings for specific models:
- `config`: Model-specific parameters
  - `temperature`: Sampling temperature (0.0-1.0)
  - `max_tokens`: Maximum tokens to generate
  - `top_p`: Nucleus sampling parameter
  - `frequency_penalty`: Frequency penalty for token selection
  - `presence_penalty`: Presence penalty for token selection
- `enabled`: Enable/disable specific model
- `endpoint`: Custom endpoint for this model

### Provider Configuration
Control provider availability:
- `enabled_providers`: List of enabled providers (omit to enable all)
- `provider_endpoints`: Custom endpoints for providers

## Supported Providers & Models

### OpenAI
- gpt-4-32k
- gpt-4-preview
- gpt-4
- gpt-3.5-turbo-16k
- gpt-3.5-turbo

### Anthropic
- claude-3-opus
- claude-3-sonnet
- claude-3-haiku

### Together AI
- codellama-70b-instruct
- codellama-34b-instruct
- codellama-13b-instruct
- llama2-70b
- llama2-13b

### Google
- gemini-pro
- gemini-ultra

### Cohere
- command-r
- command

### Mistral
- mistral-large
- mistral-medium
- mistral-small

### Meta
- llama3-70b
- llama3-13b

## Usage Example

1. Create configuration file:
```bash
echo '{
  "enabled_providers": ["OpenAI", "Anthropic"],
  "model_overrides": {
    "gpt-4": {
      "config": {
        "temperature": 0.5
      },
      "enabled": true
    }
  }
}' > models_config.json
```

2. Load configuration:
```rust
let state = ModelState::new().await?;
state.load_configuration(Path::new("models_config.json")).await?;
```

The configuration will be applied automatically to all model operations.