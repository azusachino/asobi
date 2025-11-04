use anyhow::Result;
use std::{
    future::Future,
    panic::catch_unwind,
    task::Poll,
    thread,
    time::{Duration, Instant},
};

use async_task::{Runnable, Task};
use futures_lite::future;

#[test]
fn test_futures() {}

fn spawn_task<F, T>(future: F) -> Result<Task<T>>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    anyhow::bail!("oops")
}

struct AsyncSleep {
    start_time: Instant,
    duration: Duration,
}

impl AsyncSleep {
    fn new(duration: Duration) -> Self {
        Self {
            start_time: Instant::now(),
            duration,
        }
    }
}

impl Future for AsyncSleep {
    type Output = bool;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let elapsed_time = self.start_time.elapsed();
        if elapsed_time >= self.duration {
            Poll::Ready(true)
        } else {
            cx.waker().wake_by_ref();
            // re-scheduled in-definitely
            // println!("manbo ~ hajimi");
            Poll::Pending
        }
    }
}

#[tokio::test]
async fn test_async_sleep() {
    let a_s = AsyncSleep::new(Duration::from_secs(5));
    let handle = tokio::task::spawn(async {
        a_s.await;
    });

    tokio::join!(handle);
    println!("the journey ends here")
}
