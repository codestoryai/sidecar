use std::time::Duration;

use futures::FutureExt;

#[tokio::main]
async fn main() {
    let (sender, reciever) = tokio::sync::oneshot::channel::<String>();
    let shared_receiver = reciever.shared();
    let first_receiver = shared_receiver.clone();
    let second_reciever = shared_receiver.clone();
    let _ = tokio::spawn(async move {
        let result = first_receiver.await;
        println!("First scope: {}", result.expect("to work"));
    });
    let _ = tokio::spawn(async move {
        let result = second_reciever.await;
        println!("Second scope: {}", result.expect("to work"));
    });
    tokio::time::sleep(Duration::from_secs(1)).await;
    println!("sending pawn signal");
    let _ = sender.send("pawnn".to_owned());
    tokio::time::sleep(Duration::from_secs(1)).await;
}
