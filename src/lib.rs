mod processes;

use std::time::Duration;

use log::{debug, error};
use processes::connection::Connection;
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct TcpTunnel {
    lag: Duration,
    connection_timeout: Duration,
    check_period: Duration,
    up_addr: String,
    down_addr: String,
}

impl TcpTunnel {
    pub fn new(up_addr: &str, down_addr: &str, lag_ms: u64, connection_timeout_ms: u64) -> Self {
        Self {
            lag: Duration::from_millis(lag_ms),
            connection_timeout: Duration::from_millis(connection_timeout_ms),
            check_period: Duration::from_millis(100),
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
