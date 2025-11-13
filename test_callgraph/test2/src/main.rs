use tokio_stream::Stream;
use tokio_stream::StreamExt; // 提供 next() 等便捷方法
use std::pin::Pin;
use std::task::{Context, Poll};

// 定义一个简单的异步计数器流
struct CounterStream {
    current: u32,
    limit: u32,
}

impl CounterStream {
    fn new(limit: u32) -> Self {
        Self { current: 0, limit }
    }
}

// 实现 tokio_stream::Stream trait（必须实现 poll_next）
impl Stream for CounterStream {
    type Item = u32;

    // poll_next：异步获取下一个元素的核心方法（MIR 会包含此调用）
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.current < this.limit {
            this.current += 1;
            Poll::Ready(Some(this.current)) // 返回下一个元素
        } else {
            Poll::Ready(None) // 流结束
        }
    }
}

#[tokio::main]
async fn main() {
    // 创建一个计数到 3 的流
    let mut stream = CounterStream::new(3);

    // 异步迭代流（内部通过 StreamExt::next() 调用 poll_next）
    while let Some(num) = stream.next().await {
        println!("获取到: {}", num);
    }
}