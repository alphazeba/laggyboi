use log::debug;
#[tokio::main]
async fn main() {
    env_logger::init();
    debug!("Hello, world!");
    let tunnel = laggyboi::TcpTunnel::new("127.0.0.1:8008", "127.0.0.1:4269", 500);
    tunnel.start().await;
    debug!("exiting");
}
