use sidecar::agentic::tool::code_edit::search_and_replace::SearchAndReplaceAccumulator;
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn test_newline_preservation() {
    // Test case 1: File with existing trailing newline
    let original_code = "fn main() {\n    println!(\"Hello\");\n}\n";
    let edits = r#"```rust
<<<<<<< SEARCH
fn main() {
    println!("Hello");
}
=======
fn main() {
    println!("Updated");
}
>>>>>>> REPLACE
```"#;
    let (sender, _receiver) = unbounded_channel();
    let mut accumulator = SearchAndReplaceAccumulator::new(original_code.to_owned(), 0, sender);
    accumulator.add_delta(edits.to_owned()).await;
    let final_code = accumulator.code_lines.join("\n");
    assert_eq!(final_code, "fn main() {\n    println!(\"Updated\");\n}\n");

    // Test case 2: Empty file getting new content
    let empty_code = "";
    let edits = r#"```rust
<<<<<<< SEARCH
=======
fn test() {
    println!("New content");
}
>>>>>>> REPLACE
```"#;
    let (sender, _receiver) = unbounded_channel();
    let mut accumulator = SearchAndReplaceAccumulator::new(empty_code.to_owned(), 0, sender);
    accumulator.add_delta(edits.to_owned()).await;
    let final_code = accumulator.code_lines.join("\n");
    assert_eq!(final_code, "fn test() {\n    println!(\"New content\");\n}\n");
}
