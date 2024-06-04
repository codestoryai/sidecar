use axum::Extension;

use crate::application::application::Application;

/// We send a HC check over here

pub async fn health(Extension(app): Extension<Application>) {
    // if let Some(ref semantic) = app.semantic_client {
    //     // panic is fine here, we don't need exact reporting of
    //     // subsystem checks at this stage
    //     semantic.health_check().await.unwrap()
    // }
}
