//! We create a state manager here which holds the abort handlers
//! for the various completions which might be running in the background.
//! Since this is a high throughput system, we need to make sure that
//! we cancel the completions very very quickly.

use dashmap::DashMap;

use futures::stream::{AbortHandle, AbortRegistration};

pub struct FillInMiddleState {
    pub abort_handles: DashMap<String, AbortHandle>,
}

impl FillInMiddleState {
    pub fn new() -> Self {
        Self {
            abort_handles: DashMap::new(),
        }
    }

    pub fn insert(&self, request_id: String) -> AbortRegistration {
        let (abort_handle, registration) = AbortHandle::new_pair();
        self.abort_handles.insert(request_id, abort_handle);
        registration
    }

    // check if the request is already running
    pub fn contains(&self, request_id: &str) -> bool {
        self.abort_handles.contains_key(request_id)
    }

    // get the abort handle back here and handle the termination request
    // coming from the editor correctly, for now its just cancelling the stream
    fn get(&self, request_id: &str) -> Option<AbortHandle> {
        self.abort_handles
            .get(request_id)
            .map(|guard| guard.clone())
    }

    pub fn cancel(&self, request_id: &str) {
        if let Some(abort_handle) = self.get(request_id) {
            // abort the ongoing request
            abort_handle.abort();
        }
    }
}
