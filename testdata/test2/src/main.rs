use tokio_stream::Stream;
use tokio_stream::StreamExt; // Provides convenience methods such as next()
use std::pin::Pin;
use std::task::{Context, Poll};

// Define a simple asynchronous counter stream
struct CounterStream {
    current: u32,
    limit: u32,
}

impl CounterStream {
    fn new(limit: u32) -> Self {
        Self { current: 0, limit }
    }
}

// Implement tokio_stream::Stream (poll_next is required)
impl Stream for CounterStream {
    type Item = u32;

    // poll_next: core method for asynchronously fetching the next element
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.current < this.limit {
            this.current += 1;
            Poll::Ready(Some(this.current)) // Return the next element
        } else {
            Poll::Ready(None) // End of stream
        }
    }
}

#[tokio::main]
async fn main() {
    // Create a stream that counts to 3
    let mut stream = CounterStream::new(3);

    // Iterate over the stream asynchronously
    while let Some(num) = stream.next().await {
        println!("Received: {}", num);
    }
}
