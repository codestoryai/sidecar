//! Contains the function which adds random jitter when doing retries
//! This can be useful for rate-limiting when we are nearing token limit.
//! The smarter way would be to to count the number of tokens we are using
//! per minute and manage that, but this is okay for now

use std::time::Duration;

use rand::Rng;
use tokio::time::sleep;

pub async fn jitter_sleep(base_delay: f64, jitter_factor: f64) {
    // Create a random number generator
    let actual_delay = {
        let mut rng = rand::thread_rng();

        // Calculate the actual delay by adding random jitter
        base_delay + rng.gen_range(-jitter_factor..jitter_factor)
    };

    // Ensure the delay is non-negative
    let actual_delay = actual_delay.max(0.0);

    // Sleep for the calculated delay
    let _ = sleep(Duration::from_secs_f64(actual_delay)).await;
}
