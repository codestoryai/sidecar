//! Checking parea logging if it works properly

use logging::parea::{PareaClient, PareaLogEvent};

#[tokio::main]
async fn main() {
    let parea_client = PareaClient::new();
    let parent_id = uuid::Uuid::new_v4().to_string();
    let trace_id = uuid::Uuid::new_v4().to_string();
    println!("parent_id: {:?}", &parent_id);
    println!("trace_id: {:?}", &trace_id);
    parea_client
        .log_event(PareaLogEvent::new(
            "child_trace".to_owned(),
            parent_id.to_owned(),
            trace_id.to_owned(),
            Default::default(),
        ))
        .await;
    parea_client
        .log_event(PareaLogEvent::new(
            "parent_trace".to_owned(),
            parent_id.to_owned(),
            parent_id.to_owned(),
            Default::default(),
        ))
        .await;
}
