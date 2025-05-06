use crate::processes::stream_data::StreamData;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;

use log::{debug, error};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{Duration, sleep_until};

pub async fn write_loop<'a, W>(
    writer: &'a mut W,
    mut receiver: UnboundedReceiver<StreamData>,
    lag: Duration,
    name: &str,
    stop_flag: &AtomicBool,
) where
    W: AsyncWrite + Unpin,
{
    debug!("starting write loop: {name}");
    while !stop_flag.load(Relaxed) {
        match receiver.recv().await {
            Some(data) => {
                let wake_time = data.recieve_time + lag;
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
