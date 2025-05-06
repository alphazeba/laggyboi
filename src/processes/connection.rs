use crate::TcpTunnel;
use crate::processes::read_loop::read_loop;
use crate::processes::stream_data::StreamData;
use crate::processes::write_loop::write_loop;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

use log::{debug, error};
use tokio::io;
use tokio::join;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};

pub struct Connection {
    config: TcpTunnel,
    last_received_instant: Arc<Mutex<Instant>>,
    stop_flag: Arc<AtomicBool>,
}

impl Connection {
    fn new(config: &TcpTunnel) -> Self {
        Self {
            config: config.clone(),
            last_received_instant: Arc::new(Mutex::new(Instant::now())),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn handle(config: &TcpTunnel, down_stream: TcpStream) -> JoinHandle<()> {
        let me = Self::new(config);
        tokio::spawn(async move {
            me.handle_inner(down_stream).await;
        })
    }

    async fn handle_inner(&self, down_stream: TcpStream) {
        let up_stream = TcpStream::connect(&self.config.up_addr)
            .await
            .expect("Failed to open the out stream");
        let connection_handles = self.setup_connection_handlers(down_stream, up_stream);
        self.wait_until_connection_is_inactive().await;
        connection_handles.join().await;
    }

    fn setup_connection_handlers(
        &self,
        down_stream: TcpStream,
        up_stream: TcpStream,
    ) -> ConnectionHandles {
        let (up_sender, up_reciever) = mpsc::unbounded_channel::<StreamData>();
        let (down_sender, down_reciever) = mpsc::unbounded_channel::<StreamData>();
        let (mut down_read, mut down_write) = io::split(down_stream);
        let (mut up_read, mut up_write) = io::split(up_stream);
        let lag = self.config.lag;
        let down_write_task = {
            let stop_flag = self.stop_flag.clone();
            let name = self.config.down_addr.clone();
            tokio::spawn(async move {
                write_loop(
                    &mut down_write,
                    down_reciever,
                    lag,
                    &name,
                    stop_flag.clone().as_ref(),
                )
                .await;
            })
        };
        let up_write_task = {
            let stop_flag = self.stop_flag.clone();
            let name = self.config.up_addr.clone();
            tokio::spawn(async move {
                write_loop(
                    &mut up_write,
                    up_reciever,
                    lag,
                    &name,
                    stop_flag.clone().as_ref(),
                )
                .await;
            })
        };
        let up_read_task = {
            let stop_flag = self.stop_flag.clone();
            let name = self.config.up_addr.clone();
            let last_received_instant = self.last_received_instant.clone();
            tokio::spawn(async move {
                read_loop(
                    &mut up_read,
                    down_sender,
                    &name,
                    stop_flag.as_ref(),
                    last_received_instant,
                )
                .await;
            })
        };
        let down_read_task = {
            let stop_flag = self.stop_flag.clone();
            let name = self.config.down_addr.clone();
            let last_received_instant = self.last_received_instant.clone();
            tokio::spawn(async move {
                read_loop(
                    &mut down_read,
                    up_sender,
                    &name,
                    stop_flag.as_ref(),
                    last_received_instant,
                )
                .await;
            })
        };
        ConnectionHandles {
            down_write_task,
            down_read_task,
            up_write_task,
            up_read_task,
        }
    }

    async fn wait_until_connection_is_inactive(&self) {
        while !self.stop_flag.load(Relaxed) {
            sleep(self.config.check_period).await;
            match self.last_received_instant.lock() {
                Ok(last_received_instant) => {
                    if self.has_timed_out(&last_received_instant) {
                        debug!("have not seen an incoming data in too long, shutting down");
                        self.stop_flag.store(true, Relaxed);
                    }
                }
                Err(e) => {
                    error!("failed to get lock on last received instant: {}", e);
                    self.stop_flag.store(true, Relaxed);
                }
            }
        }
    }

    fn has_timed_out(&self, last_received_instant: &Instant) -> bool {
        let timeout_instant = last_received_instant
            .checked_add(self.config.connection_timeout + self.config.lag)
            .expect("somehow the instant is not valid");
        timeout_instant < Instant::now()
    }
}

struct ConnectionHandles {
    down_write_task: JoinHandle<()>,
    down_read_task: JoinHandle<()>,
    up_write_task: JoinHandle<()>,
    up_read_task: JoinHandle<()>,
}

impl ConnectionHandles {
    pub async fn join(self) {
        let _ = join!(
            self.down_write_task,
            self.down_read_task,
            self.up_write_task,
            self.up_read_task
        );
        debug!("joined all threads :)");
    }
}
