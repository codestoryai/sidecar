# Contributing to LARP
There are many ways to contribute to LARP: logging bugs, submitting pull requests, reporting issues, and creating suggestions.

After cloning and building the repo, check out the [issues list](https://github.com/codestoryai/larp/issues?utf8=%E2%9C%93&q=is%3Aopen+is%3Aissue). Issues labeled [`help wanted`](https://github.com/codestoryai/larp/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22) are good issues to submit a PR for. Issues labeled [`good first issue`](https://github.com/codestoryai/larp/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22) are great candidates to pick up if you are in the code for the first time. If you are contributing significant changes, or if the issue is already assigned to a specific month milestone, please discuss with the assignee of the issue first before starting to work on the issue.

## How to build locally
1. Ensure you are using Rust 1.73
2. Build the binary: `cargo build --bin webserver`
3. Run the binary: `./target/debug/webserver --qdrant-binary-directory ./larp/qdrant --dylib-directory ./larp/onnxruntime/ --model-dir ./larp/models/all-MiniLM-L6-v2/ --qdrant-url http://127.0.0.1:6334`
4. Profit!

## Your own ideas
We encourage you to poke around the codebase, find flaws and ways to improve it. The prompts and responses are also logged locally in your machine so you should be able to very quickly triage things.
If you want a new feature or want to change something, please reach out to us on [discrod](https://discord.gg/mtgrhXM5Xf) or create an issue on the repo.

## Debugging
The best way to debug is cowboy style, put print statments and check if your code is hitting the right branch and doing the right things.
Since you will be working on the debug build of the larp, iteration cycles are fast, just run `cargo buid --bin webserver` and you should see the log spam on stdout.

## Pull Requests
We use the [GitHub flow](https://guides.github.com/introduction/flow/) for pull requests. This means that you should fork the repo, create a branch, make your changes, and then create a pull request. We will review your PR and provide feedback. Once the PR is approved, we will merge it into the main branch.
