use crate::processes::stream_data::StreamData;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

use log::{debug, error, trace};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{Duration, Instant, timeout};

const BUFFER_BYTES: usize = 1024;
const READ_TIMEOUT: Duration = Duration::from_millis(500);
type SENDER = UnboundedSender<StreamData>;

pub async fn read_loop<'a, R>(
    reader: &'a mut R,
    sender: UnboundedSender<StreamData>,
    name: &str,
    stop_flag: &AtomicBool,
    last_read_instant: Arc<Mutex<Instant>>,
) where
    R: AsyncRead + Unpin,
{
    debug!("starting read loop: {name}");
    let mut buffer: [u8; BUFFER_BYTES] = [0; BUFFER_BYTES];
    while !stop_flag.load(Relaxed) {
        let _ = read_loop_inner(reader, &sender, name, &last_read_instant, &mut buffer)
            .await
            .map_err(|e| {
                match e {
                    // timing out is desirable, prevents read call from blocking.
                    ReadLoopErr::ReadTimeout(e) => trace!("read timeout: {e}"),
                    e => error!("read loop error: {e}"),
                }
            });
    }
    debug!("exiting read loop: {name}");
}

async fn read_loop_inner<'a, R>(
    reader: &'a mut R,
    sender: &SENDER,
    name: &str,
    last_read_instant: &Mutex<Instant>,
    buffer: &mut [u8],
) -> Result<(), ReadLoopErr>
where
    R: AsyncRead + Unpin,
{
    let read_result = timeout(READ_TIMEOUT, reader.read(buffer))
        .await
        .map_err(|e| ReadLoopErr::ReadTimeout(e.to_string()))?;
    match read_result {
        Ok(num_bytes) if num_bytes == 0 => Ok(()),
        Ok(num_bytes) => {
            debug!("received {} bytes from {}", num_bytes, name);
            handle_receive_bytes(sender, &buffer[0..num_bytes])
                .and_then(|()| update_last_read_instant(last_read_instant))
        }
        Err(e) => Err(ReadLoopErr::ReadFail(e.to_string())),
    }
}

fn handle_receive_bytes(sender: &SENDER, bytes: &[u8]) -> Result<(), ReadLoopErr> {
    let to_send = StreamData::new(Vec::from(bytes));
    sender
        .send(to_send)
        .map_err(|e| ReadLoopErr::SendToSenderFail(e.to_string()))
}

fn update_last_read_instant(last_read_instant: &Mutex<Instant>) -> Result<(), ReadLoopErr> {
    match last_read_instant.lock() {
        Ok(mut last_read_instant) => {
            *last_read_instant = Instant::now();
            Ok(())
        }
        Err(e) => Err(ReadLoopErr::MutexLockFail(e.to_string())),
    }
}

#[derive(Error, Debug)]
enum ReadLoopErr {
    #[error("failed to get lock on last read instant in read loop: {0}")]
    MutexLockFail(String),
    #[error("there was an error sending data to lagger: {0}")]
    SendToSenderFail(String),
    #[error("Read timeout: {0}")]
    ReadTimeout(String),
    #[error("Read failed: {0}")]
    ReadFail(String),
}
