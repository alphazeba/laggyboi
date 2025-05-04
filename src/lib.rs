use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::thread::sleep;

use log::{debug, error};
use tokio::join;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::io::{self, AsyncRead, AsyncWrite, AsyncWriteExt, AsyncReadExt};
use tokio::time::{Instant, Duration};

pub struct TcpTunnel {
    lag_ms: u64,
    connection_timeout_ms: u64,
    check_period_ms: u64,
    up_addr: String,
    down_addr: String,
}

impl TcpTunnel {
    pub fn new(up_addr: &str, down_addr: &str, lag_ms: u64) -> Self {
        Self {
            lag_ms,
            connection_timeout_ms: 250,
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
            let stop_flag = Arc::new(
                AtomicBool::new(false));
            let last_received_instant = Arc::new(Mutex::new(Instant::now()));
            match listener.accept().await {
                Err(e) => {
                    error!("listener failed to accept: {}", e);
                }
                Ok((down_stream, _)) => {
                    debug!("accepted incoming traffic");
                    let up_stream = tokio::net::TcpStream::connect(&self.up_addr).await
                        .expect("Failed to open the out stream");
                    let (up_sender, up_reciever) = mpsc::unbounded_channel::<Data>();
                    let (down_sender, down_reciever) = mpsc::unbounded_channel::<Data>();
                    let (mut down_read, mut down_write) = io::split(down_stream);
                    let (mut up_read, mut up_write) = io::split(up_stream);
                    let lag_ms = self.lag_ms;
                    let down_write_thread = {
                        let stop_flag = stop_flag.clone();
                        let name = self.down_addr.clone();
                        tokio::spawn(async move {
                            write_loop(&mut down_write, down_reciever, lag_ms, &name, stop_flag.clone().as_ref()).await;})
                    };
                    let up_write_thread = {
                        let stop_flag = stop_flag.clone();
                        let name = self.up_addr.clone();
                        tokio::spawn(async move {
                            write_loop(&mut up_write, up_reciever, lag_ms, &name, stop_flag.clone().as_ref()).await;})
                    };
                    let down_read_thread = {
                        let stop_flag = stop_flag.clone();
                        let name = self.down_addr.clone();
                        let last_received_instant = last_received_instant.clone();
                        tokio::spawn(async move {
                            read_loop(&mut down_read, up_sender, &name, stop_flag.as_ref(), last_received_instant).await;})
                    };
                    let up_read_thread = {
                        let stop_flag = stop_flag.clone();
                        let name = self.up_addr.clone();
                        let last_received_instant = last_received_instant.clone();
                        tokio::spawn(async move {
                            read_loop(&mut up_read, down_sender, &name, stop_flag.as_ref(), last_received_instant).await;})
                    };
                    while !stop_flag.load(Relaxed) {
                        sleep(Duration::from_millis(self.check_period_ms));
                        match last_received_instant.lock() {
                            Ok(last_received_instant) => {
                                let timeout_instant = last_received_instant
                                    .checked_add(Duration::from_millis(self.connection_timeout_ms + self.lag_ms))
                                    .expect("somehow the instant is not valid");
                                if timeout_instant < Instant::now() {
                                    debug!("have not seen an incoming packet in too long, shutting down");
                                    stop_flag.store(true, Relaxed);
                                }
                            },
                            Err(e) => {
                                error!("failed to get lock on last received instant: {}", e);
                                stop_flag.store(true, Relaxed);
                            }
                        }
                    }
                    // the up read thread gets stuck in the .read method.
                    // it continuously waits receiving no values.  Unlike the down read which continuously receives 0 bytes.
                    debug!("aborting the up read thread");
                    up_read_thread.abort();
                    let _ = join!(down_write_thread, up_write_thread, down_read_thread, up_read_thread);
                    debug!("joined all threads :)");
                }
            }
        }
    }
}

async fn write_loop<'a, W>(writer: &'a mut W, mut receiver: UnboundedReceiver<Data>, lag_ms: u64, name: &str, stop_flag: &AtomicBool) 
    where W: AsyncWrite + Unpin,
{
    debug!("starting write loop: {name}");
    while !stop_flag.load(Relaxed) {
        match receiver.recv().await {
            Some(data) => {
                let wake_time = data.recieve_time + Duration::from_millis(lag_ms);
                tokio::time::sleep_until(wake_time).await;
                match writer.write(&data.buf).await {
                    Ok(_) => (),
                    Err(e) => error!("there was error writing data: {}", e),
                }
            }
            None => break,
        };
    }
    debug!("exiting write loop: {name}");
}

async fn read_loop<'a, R>(
    reader: &'a mut R,
    sender: UnboundedSender<Data>,
    name: &str,
    stop_flag: &AtomicBool,
    last_read_instant: Arc<Mutex<Instant>>,
)
    where R: AsyncRead + Unpin,
{
    debug!("starting read loop: {name}");
    let mut buffer: [u8; 1024] = [0; 1024];
    while !stop_flag.load(Relaxed) {
        let num_bytes = reader.read(&mut buffer).await.expect("error in read loop");
        if num_bytes == 0 {
            continue;
        }
        let output: Vec<u8> = Vec::from(&buffer[0..num_bytes]);
        debug!("received {} bytes from {}", num_bytes, name);
        match sender.send(Data::new(output)) {
            Ok(_) => {
                match last_read_instant.lock() {
                    Ok(mut last_read_instant) => {
                        *last_read_instant = Instant::now();
                    }
                    Err(e) => {
                        error!("failed to get lock on last read instant in read loop: {}", e);
                        ()
                    }
                }
            },
            Err(e) => {
                error!("there was an error sending data to lagger: {}", e);
                ()
            },
        }
    }
    debug!("exiting read loop: {name}");
}

struct Data {
    buf: Vec<u8>,
    recieve_time: Instant,
}

impl Data {
    pub fn new(buf: Vec<u8>) -> Self {
        Data {
            buf,
            recieve_time: Instant::now(),
        }
    }
}