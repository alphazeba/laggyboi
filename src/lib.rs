use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

use log::{debug, error};
use tokio::join;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::io::{self, AsyncRead, AsyncWrite, AsyncWriteExt, AsyncReadExt};
use tokio::time::{sleep, sleep_until, timeout, Duration, Instant};

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
                Err(e) => {
                    error!("listener failed to accept: {}", e);
                }
                Ok((down_stream, _)) => {
                    let me = self.clone();
                    let _ = tokio::spawn(async move {
                        let stop_flag = Arc::new(
                            AtomicBool::new(false));
                        let last_received_instant = Arc::new(Mutex::new(Instant::now()));
                        debug!("accepted incoming traffic");
                        let up_stream = tokio::net::TcpStream::connect(&me.up_addr).await
                            .expect("Failed to open the out stream");
                        let (up_sender, up_reciever) = mpsc::unbounded_channel::<Data>();
                        let (down_sender, down_reciever) = mpsc::unbounded_channel::<Data>();
                        let (mut down_read, mut down_write) = io::split(down_stream);
                        let (mut up_read, mut up_write) = io::split(up_stream);
                        let lag_ms = me.lag_ms;
                        let down_write_thread = {
                            let stop_flag = stop_flag.clone();
                            let name = me.down_addr.clone();
                            tokio::spawn(async move {
                                write_loop(&mut down_write, down_reciever, lag_ms, &name, stop_flag.clone().as_ref()).await;})
                        };
                        let up_write_thread = {
                            let stop_flag = stop_flag.clone();
                            let name = me.up_addr.clone();
                            tokio::spawn(async move {
                                write_loop(&mut up_write, up_reciever, lag_ms, &name, stop_flag.clone().as_ref()).await;})
                        };
                        let up_read_thread = {
                            let stop_flag = stop_flag.clone();
                            let name = me.up_addr.clone();
                            let last_received_instant = last_received_instant.clone();
                            tokio::spawn(async move {
                                read_loop(&mut up_read, down_sender, &name, stop_flag.as_ref(), last_received_instant).await;})
                        };
                        let down_read_thread = {
                            let stop_flag = stop_flag.clone();
                            let name = me.down_addr.clone();
                            let last_received_instant = last_received_instant.clone();
                            tokio::spawn(async move {
                                read_loop(&mut down_read, up_sender, &name, stop_flag.as_ref(), last_received_instant).await;})
                        };
                        
                        while !stop_flag.load(Relaxed) {
                            sleep(Duration::from_millis(me.check_period_ms)).await;
                            match last_received_instant.lock() {
                                Ok(last_received_instant) => {
                                    let timeout_instant = last_received_instant
                                        .checked_add(Duration::from_millis(me.connection_timeout_ms + me.lag_ms))
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
                        // // the up read thread gets stuck in the .read(..).await method.
                        // // it continuously waits receiving no values.  Unlike the down read which continuously receives 0 bytes.
                        // debug!("aborting the up read thread");
                        // up_read_thread.abort();
                        let _ = join!(down_write_thread, up_write_thread, down_read_thread, up_read_thread);
                        debug!("joined all threads :)");
                    });
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
                sleep_until(wake_time).await;
                debug!("writing {} bytes to {}", data.buf.len(), name);
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
        let read_future = reader.read(&mut buffer);
        let read_timeout_millis = 500; // TODO should come from config
        let thing= timeout(
            Duration::from_millis(read_timeout_millis),
            read_future
        ).await
        .inspect(|x| {
            match x {
                Ok(num_bytes) if *num_bytes == 0 => (),
                Ok(num_bytes) => {
                    let output: Vec<u8> = Vec::from(&buffer[0..*num_bytes]);
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
                Err(e) => {
                    error!("error in read loop: {name}, {e}");
                    ()
                }
            }
        });
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