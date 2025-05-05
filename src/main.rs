use log::debug;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    up_addr: String,
    #[arg(short, long)]
    down_addr: String,
    #[arg(short, long)]
    lag_ms: u64,
    #[arg(short, long)]
    connection_timeout_ms: u64,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    debug!("starting tunnel from {} to {} with {} ms of lag (conn timeout: {} ms)", args.down_addr, args.up_addr, args.lag_ms, args.connection_timeout_ms);
    let tunnel = laggyboi::TcpTunnel::new(&args.up_addr, &args.down_addr, args.lag_ms, args.connection_timeout_ms);
    tunnel.start().await;
    debug!("exiting");
}
