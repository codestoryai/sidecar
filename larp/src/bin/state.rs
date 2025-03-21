use sidecar::state::BINARY_VERSION_HASH;

#[tokio::main]
async fn main() {
    let binary_version = BINARY_VERSION_HASH;
    println!("{}", binary_version);
}
