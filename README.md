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


## Sidecar

Sidecar is the AI brains of Aide the editor. To accomplish the work of creating the prompts, talking to LLM and everything else in between Sidecar is responsible for making sure it all works together.

Broadly speaking these are the following important bits in Sidecar:
- `tool_box.rs` - The collection of all and any tools AI might need is present here, all the language specific smartness is handled by `tool_box.rs`
- `symbol/` - The symbol folder contains the code which allows each individual symbol to be smart and independent. This can work on any granularity level, all the way from a file to a single function or function inside a class (its very versatile)
- `llm_prompts/` - This is a relic of the past (and somewhat in use still) for creating prompts especially for the inline completion bits. The inline completions bits are not maintained any longer but if you want to take a stab at working on it, please reach out to us on Discord, we are happy to support you.
- `repomap` - This creates a repository map using page rank on the code symbols. Most of the code here is a port of the python implementation done on Aider (do check it out if you are in the market for a CLI tool for code-generation)

## Getting Started
1. Ensure you are using Rust 1.73
2. Build the binary: `cargo build --bin webserver`
3. Run the binary: `./target/debug/webserver`
4. Profit!

## Bonus on how to get your Aide editor to talk to Sidecar:
1. Run the Aide production build or build from source using [this](https://github.com/codestoryai/ide)
2. Run the sidecar binary
3. Since you have a sidecar binary already running, the editor will prefer to use this over starting its own process.
4. Congratulations! You are now running sidecar for Aide locally with your own built binary.

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
