```

 ██████╗ ██████╗ ██████╗ ███████╗███████╗████████╗ ██████╗ ██████╗ ██╗   ██╗
██╔════╝██╔═══██╗██╔══██╗██╔════╝██╔════╝╚══██╔══╝██╔═══██╗██╔══██╗╚██╗ ██╔╝
██║     ██║   ██║██║  ██║█████╗  ███████╗   ██║   ██║   ██║██████╔╝ ╚████╔╝ 
██║     ██║   ██║██║  ██║██╔══╝  ╚════██║   ██║   ██║   ██║██╔══██╗  ╚██╔╝  
╚██████╗╚██████╔╝██████╔╝███████╗███████║   ██║   ╚██████╔╝██║  ██║   ██║   
 ╚═════╝ ╚═════╝ ╚═════╝ ╚══════╝╚══════╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝   ╚═╝   
                                                                            
```

<div id="vscodium-logo" align="center">
    <img src="./media/logo.svg" alt="VSCodium Logo" width="200"/>
    <h1>Sidecar</h1>
</div>

![Latest release](https://img.shields.io/github/v/release/codestoryai/binaries?label=version)
![Discord Shield](https://discord.com/api/guilds/1138070673756004464/widget.png?style=shield)


## Overview

Sidecar is the AI brains of Aide, providing intelligent code assistance through:
- Code completion and suggestions
- Repository analysis and symbol mapping
- Git integration and version control
- Real-time code intelligence

## Architecture

Sidecar consists of several key components:

### Core Components
- `tool_box.rs` - Collection of language-specific tools and utilities
- `symbol/` - Smart symbol management from file to function level
- `llm_prompts/` - LLM integration and prompt management
- `repomap/` - Repository mapping using PageRank on code symbols

### Additional Components
- `webserver/` - API endpoints and WebSocket communication
- `chunking/` - Code analysis and segmentation
- `git/` - Version control integration
- `agentic/` - AI agent implementation

## Features

1. **Code Intelligence**
   - Smart code completion
   - Symbol analysis and navigation
   - Context-aware suggestions
   - Multi-language support

2. **Repository Analysis**
   - Code structure mapping
   - Dependency tracking
   - Symbol relationship analysis
   - Intelligent code navigation

3. **AI Integration**
   - Multiple LLM provider support
   - Customizable prompts
   - Context-aware responses
   - Efficient token management

## Quick Start

1. **Prerequisites**
   ```bash
   # Ensure Rust 1.73
   rustup default 1.73
   
   # Clone repository
   git clone https://github.com/codestoryai/sidecar.git
   cd sidecar
   ```

2. **Build and Run**
   ```bash
   # Build the binary
   cargo build --bin webserver
   
   # Run Sidecar
   ./target/debug/webserver
   ```

3. **Configuration**
   - Copy `.env.example` to `.env`
   - Set required API keys
   - Configure model preferences

## Integration with Aide Editor

1. Run the Aide production build or build from source using [this](https://github.com/codestoryai/ide)
2. Run the sidecar binary
3. Since you have a sidecar binary already running, the editor will prefer to use this over starting its own process
4. Your Aide editor is now using your local Sidecar instance!

## Documentation

- [Standalone Sidecar Guide](standalone-sidecar.md) - Build standalone version
- [Settings Guide](sidecar-settings.md) - Configure models and preferences
- [API Documentation](api-documentation.md) - REST and WebSocket APIs
- [Development Guide](development-guide.md) - Development workflow
- [Testing Guide](testing-guide.md) - Testing practices
- [Deployment Guide](deployment-guide.md) - Deployment options

## Contributing

There are many ways in which you can participate in this project, for example:

* [Submit bugs and feature requests](https://github.com/codestoryai/sidecar/issues), and help us verify as they are checked in
* Review [source code changes](https://github.com/codestoryai/sidecar/pulls)

If you are interested in fixing issues and contributing directly to the code base,
please see the document [How to Contribute](HOW_TO_CONTRIBUTE.md), which covers the following:

* [How to build and run from source](HOW_TO_CONTRIBUTE.md)
* [The development workflow, including debugging and running tests](HOW_TO_CONTRIBUTE.md#debugging)
* [Submitting pull requests](HOW_TO_CONTRIBUTE.md#pull-requests)

## Feedback

* [File an issue](https://github.com/codestoryai/sidecar/issues)
* [Request a new feature](CONTRIBUTING.md)
* Upvote [popular feature requests](https://github.com/codestoryai/sidecar/issues?q=is%3Aopen+is%3Aissue+label%3Afeature-request+sort%3Areactions-%2B1-desc)
* Join our community: [Discord](https://discord.gg/mtgrhXM5Xf)

## Code of Conduct

This project has adopted the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). Please read the Code of Conduct before contributing to this project.

## License

Copyright (c) 2024 CodeStory AI. All rights reserved.
Licensed under the [GNU Affero General Public License v3.0](LICENSE.md).