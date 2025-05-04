
#[tokio::main]
async fn main() {
    println!("Hello, world!");
    laggyboi::tcp_tunnel(400).await;
    println!("exiting");
}
