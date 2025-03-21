use llm_prompts::reranking::types::CodeSpan;

#[tokio::main]
async fn main() {
    let folder_path = "/Users/skcd/scratch/sidecar/llm_client";
    let output = CodeSpan::read_folder_selection(folder_path).await;
    println!("{:?}", &output);
    let mut entries = tokio::fs::read_dir(&folder_path).await.expect("to work");
    while let Some(entry) = entries.next_entry().await.expect("to work") {
        dbg!("entry", &entry.path());
        dbg!(
            "entry.type",
            &entry.path().is_file(),
            &entry.path().is_dir()
        );
    }
}
