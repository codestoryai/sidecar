use std::{env, process::Stdio};

use serde_json::json;
use tokio::{io::AsyncReadExt, process::Command};

async fn get_diff_patch(git_dname: &str) -> String {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(git_dname)
        .arg("--no-pager") // Add this line to disable the pager
        .arg("diff")
        .stdout(Stdio::piped())
        .spawn()
        .expect("to work");
    let _ = child.wait().await;
    let mut stdout = child.stdout.take().expect("Failed to get stdout");
    let mut output = Vec::new();
    stdout.read_to_end(&mut output).await.expect("to work");

    let output_string = String::from_utf8_lossy(&output);
    println!("Output: {}", output_string);
    output_string.to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SWEBenchInput {
    instance_id: String,
    gemini_api_key: String,
    repo_map_fs_path: String,
    repo_path: String,
    problem_statement: String,
}

#[tokio::main]
async fn main() {
    let instance_id = env::var("swe_bench_test_suite").expect("to always be present");
    let path = format!("/Users/skcd/scratch/swe_bench/inputs/{}.json", instance_id);
    let input: SWEBenchInput = serde_json::from_slice(
        &tokio::fs::read(&path)
            .await
            .expect("file reading to always work"),
    )
    .expect("to work");
    let instance_id = input.instance_id;
    let folder_path = input.repo_path;
    let git_diff = get_diff_patch(&folder_path).await;
    println!("Whats the git diff\n");
    println!("{}", git_diff);
    let prediction_json = json!({
        "instance_id": instance_id.to_owned(),
        "model_name_or_path": "codestory-mixed".to_owned(),
        "model_patch": get_diff_patch(&folder_path).await,
    });

    let prediction_output = "/Users/skcd/scratch/swe_bench/predictions/full---gpt-4o/".to_owned()
        + &instance_id
        + ".jsonl";

    let _ = dbg!(
        tokio::fs::write(
            prediction_output,
            serde_json::to_string(&prediction_json).expect("serde to not fail"),
        )
        .await
    );
}
