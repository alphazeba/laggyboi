use crate::processes::stream_data::StreamData;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

use log::{debug, error};
use tokio::sync::mpsc::UnboundedSender;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{timeout, Duration, Instant};


pub async fn read_loop<'a, R>(
    reader: &'a mut R,
    sender: UnboundedSender<StreamData>,
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
        let _ = timeout(
            Duration::from_millis(read_timeout_millis),
            read_future
        ).await
        .inspect(|x| {
            match x {
                Ok(num_bytes) if *num_bytes == 0 => (),
                Ok(num_bytes) => {
                    let output: Vec<u8> = Vec::from(&buffer[0..*num_bytes]);
                    debug!("received {} bytes from {}", num_bytes, name);
                    match sender.send(StreamData::new(output)) {
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