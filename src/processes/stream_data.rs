use tokio::time::Instant;

pub struct StreamData {
    pub buf: Vec<u8>,
    pub recieve_time: Instant,
}

impl StreamData {
    pub fn new(buf: Vec<u8>) -> Self {
        Self {
            buf,
            recieve_time: Instant::now(),
        }
    }
}