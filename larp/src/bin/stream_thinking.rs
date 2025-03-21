use std::time::Duration;

use async_stream::stream;
use futures::{pin_mut, Stream, StreamExt};
use serde_xml_rs::from_str;
use sidecar::agentic::tool::{
    code_edit::xml_processor::XmlProcessor, code_symbol::models::anthropic::StepListItem,
};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let input = r#"<reply>
    <thinking>
    We need to add a new endpoint for code_request_stop, similar to probe_request_stop, in the agentic router.
    </thinking>
    <step_by_step>
    <step_list>
    <name>
    agentic_router
    </name>
    <file_path>
    /Users/skcd/test_repo/sidecar/sidecar/src/bin/webserver.rs
    </file_path>
    <step>
    Add a new route for code_request_stop in the agentic_router function
    </step>
    </step_list>
    <step_list>
    <name>
    code_request_stop
    </name>
    <file_path>
    /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
    </file_path>
    <new>true</new>
    <step>
    Implement the code_request_stop function, reusing logic from probe_request_stop
    </step>
    </step_list>
    <step_list>
    <name>
    CodeRequestStop
    </name>
    <file_path>
    /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
    </file_path>
    <new>true</new>
    <step>
    Create a new struct CodeRequestStop similar to ProbeStopRequest
    </step>
    </step_list>
    <step_list>
    <name>
    CodeRequestStopResponse
    </name>
    <file_path>
    /Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
    </file_path>
    <new>true</new>
    <step>
    Create a new struct CodeRequestStopResponse similar to ProbeStopResponse
    </step>
    </step_list>
    </step_by_step>
    </reply>
    "#;

    let chunk_size = 10;
    let stream = simulate_stream(input.to_owned(), chunk_size);

    pin_mut!(stream);

    let mut xml_processor = XmlProcessor::new();
    let mut thinking_extracted = false;
    let mut step_list_extracted = Vec::new();

    while let Some(chunk) = stream.next().await {
        println!("Received chunk: {}", chunk);
        xml_processor.append(&chunk);

        // Attempt to extract the thinking tag's content
        if !thinking_extracted {
            if let Some(content) = xml_processor.extract_tag_content("thinking") {
                println!("Extracted thinking content: {}", content);
                // Process or store the thinking content as needed
                thinking_extracted = true;
            }
        }

        // Extract step_list items
        let step_lists = xml_processor.extract_all_tag_contents("step_list");
        for step_list in step_lists {
            println!("Extracted step_list content:\n{}", step_list);
            step_list_extracted.push(step_list);
        }
    }

    // Now, step_list_extracted contains all the extracted <step_list> contents
    println!("All extracted step_list items:");
    for step in &step_list_extracted {
        println!("{}", step);
        let wrapped_step = XmlProcessor::wrap_xml("step_list", &step);
        let output = from_str::<StepListItem>(&wrapped_step);

        match output {
            Ok(step_list_item) => {
                println!("Parsed StepListItem: {:?}", step_list_item);
            }
            Err(e) => {
                eprintln!("Failed to parse StepListItem: {:?}", e);
            }
        }
    }
}

fn simulate_stream(input: String, chunk_size: usize) -> impl Stream<Item = String> {
    stream! {
        let mut index = 0;
        let len = input.len();
        while index < len {
            let end = (index + chunk_size).min(len);
            let chunk = &input[index..end];
            yield chunk.to_string();
            index = end;
            sleep(Duration::from_millis(50)).await;
        }
    }
}
