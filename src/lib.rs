mod processes;

use log::{debug, error};
use processes::connection::Connection;
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct TcpTunnel {
    lag_ms: u64,
    connection_timeout_ms: u64,
    check_period_ms: u64,
    up_addr: String,
    down_addr: String,
}

impl TcpTunnel {
    pub fn new(up_addr: &str, down_addr: &str, lag_ms: u64, connection_timeout_ms: u64) -> Self {
        Self {
            lag_ms,
            connection_timeout_ms,
            check_period_ms: 100,
            up_addr: up_addr.to_string(),
            down_addr: down_addr.to_string(),
        }
    }

    pub async fn start(&self) {
        let listener = TcpListener::bind(&self.down_addr)
            .await
            .expect("failed to bind tcp listener");
        loop {
            match listener.accept().await {
                Ok((down_stream, _)) => {
                    debug!("accepted incoming traffic");
                    let _ = Connection::handle(self, down_stream);
                }
                Err(e) => {
                    error!("listener failed to accept: {}", e);
                }
            }
        }
    }
}
