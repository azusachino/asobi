#[cfg(test)]
mod tests_vol1 {
    use std::{
        sync::{Arc, Mutex},
        task::{Poll, Waker},
        time::Duration,
    };
    use tokio::{sync::mpsc, task, time::*};

    async fn slow_task() -> &'static str {
        sleep(Duration::from_secs(10)).await;
        "slow task completed"
    }

    #[tokio::test]
    async fn test_1() {
        let duration = Duration::from_secs(3);
        let result = timeout(duration, slow_task()).await;
        match result {
            Ok(v) => println!("task succeed: {}", v),
            Err(_) => println!("task time out"),
        }
    }

    struct MyFutureState {
        data: Option<Vec<u8>>,
        waker: Option<Waker>,
    }

    struct MyFuture {
        state: Arc<Mutex<MyFutureState>>,
    }

    impl MyFuture {
        fn new() -> (Self, Arc<Mutex<MyFutureState>>) {
            let state = Arc::new(Mutex::new(MyFutureState {
                data: None,
                waker: None,
            }));
            (
                MyFuture {
                    state: state.clone(),
                },
                state,
            )
        }
    }

    impl Future for MyFuture {
        type Output = String;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            println!("polling the future");
            let mut state = self.state.lock().unwrap();
            if state.data.is_some() {
                let data = state.data.take().unwrap();
                Poll::Ready(String::from_utf8(data).unwrap())
            } else {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    #[tokio::test]
    async fn test_2() {
        let (my_future, state) = MyFuture::new();
        let (tx, mut rx) = mpsc::channel::<()>(1);
        let task_handler = task::spawn(async { my_future.await });
        sleep(Duration::from_secs(3)).await;
        println!("spawning trigger task");

        let trigger_task = task::spawn(async move {
            rx.recv().await;
            let mut state = state.lock().unwrap();
            state.data = Some(b"hello from the outside".to_vec());
            loop {
                if let Some(waker) = state.waker.take() {
                    waker.wake();
                    break;
                }
            }
        });
        tx.send(()).await.unwrap();

        let outcome = task_handler.await.unwrap();
        println!("task completed with outcome: {}", outcome);
        trigger_task.await.unwrap();
    }
}
