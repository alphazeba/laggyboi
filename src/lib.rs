use log::{debug, error};
use tokio::join;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::io::{self, AsyncRead, AsyncWrite, AsyncWriteExt, AsyncReadExt};
use tokio::time::{Instant, Duration};

const IN_ADDR: &str = "127.0.0.1:4269";
const OUT_ADDR: &str = "127.0.0.1:8008";

pub async fn tcp_tunnel(lag_ms: u64) {
    let listener = TcpListener::bind(IN_ADDR)
        .await
        .expect("failed to bind tcp listener");
    loop {
        match listener.accept().await {
            Err(e) => {
                error!("listener failed to accept: {}", e);
            }
            Ok((down_stream, _)) => {
                debug!("accepted incoming traffic");
                // create out steam.
                // need to have a up_sender
                // need to have a down_receiver
                let up_stream = tokio::net::TcpStream::connect(OUT_ADDR)
                    .await
                    .expect("Failed to open the out stream");
                let (up_sender, up_reciever) = mpsc::unbounded_channel::<Data>();
                let (down_sender, down_reciever) = mpsc::unbounded_channel::<Data>();
                let (mut down_read, mut down_write) = io::split(down_stream);
                let (mut up_read, mut up_write) = io::split(up_stream);
                let _ = join!(
                    tokio::spawn(async move {
                        write_loop(&mut down_write, down_reciever, lag_ms).await;
                    }),
                    tokio::spawn(async move {
                        write_loop( &mut up_write, up_reciever, lag_ms).await;
                    }),
                    tokio::spawn(async move {
                        read_loop(&mut down_read, up_sender).await;
                    }),
                    tokio::spawn(async move {
                        read_loop(&mut up_read, down_sender).await;
                    })
                );
            }
        }
    }
}

async fn write_loop<'a, W>(writer: &'a mut W, mut receiver: UnboundedReceiver<Data>, lag_ms: u64) 
    where W: AsyncWrite + Unpin,
{
    debug!("starting write loop");
    loop {
        match receiver.recv().await {
            Some(data) => {
                let wake_time = data.recieve_time + Duration::from_millis(lag_ms);
                debug!("will be waking at: {:?}", wake_time);
                tokio::time::sleep_until(wake_time).await;
                match writer.write(&data.buf).await {
                    Ok(x) => (),
                    Err(e) => error!("there was error writing data: {}", e),
                }
            }
            None => break,
        };
    }
}

async fn read_loop<'a, R>(reader: &'a mut R, sender: UnboundedSender<Data>)
    where R: AsyncRead + Unpin,
{
    debug!("starting read loop");
    let mut buffer: [u8; 1024] = [0; 1024];
    loop {
        let num_bytes = reader.read(&mut buffer).await.expect("error in read loop");
        let output: Vec<u8> = Vec::from(&buffer[0..num_bytes]);
        match sender.send(Data::new(output)) {
            Ok(_) => (),
            Err(e) => {
                error!("there was an error sending data to lagger: {}", e);
                ()
            },
        }
    }
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