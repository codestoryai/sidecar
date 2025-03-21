use sidecar::agentic::tool::{
    input::ToolInput,
    lsp::file_diagnostics::{FileDiagnostics, FileDiagnosticsInput},
    r#type::Tool,
};

#[tokio::main]
async fn main() {
    let path = "/Users/zi/codestory/sidecar/sidecar/src/agentic/tool/plan/plan.rs";
    let editor_url = "http://localhost:42427".to_owned();

    let file_diagnostic_input =
        FileDiagnosticsInput::new(path.to_owned(), editor_url, true, None, false);
    let file_diagnostic_client = FileDiagnostics::new();

    let _response = file_diagnostic_client
        .invoke(ToolInput::FileDiagnostics(file_diagnostic_input))
        .await;

    // dbg!(response);
}
