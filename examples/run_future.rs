use std::{task::Poll, time::Duration};

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let one = CounterFuture::default();
    let two = CounterFuture::default();
    let handle_1 = tokio::task::spawn(async move { one.await });
    let handle_2 = tokio::task::spawn(async move { two.await });
    let (_, _) = tokio::join!(handle_1, handle_2);
    Ok(())
}

#[derive(Debug, Default)]
struct CounterFuture {
    count: u32,
}

impl Future for CounterFuture {
    type Output = u32;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.count += 1;
        println!("current thread id: {:?}", std::thread::current().id());
        println!("polling with result: {}", self.count);
        std::thread::sleep(Duration::from_secs(1));
        if self.count < 5 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        Poll::Ready(self.count)
    }
}
