use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::Duration,
};

use once_cell::sync::OnceCell;

use self::timerwheel::TimeWheel;

pub mod timerwheel;

#[derive(Clone)]
pub struct TimerExecutor {
    inner: Arc<Mutex<TimerExecutorImpl>>,
}

struct TimerExecutorImpl {
    timer_id_seq: usize,
    wheel: TimeWheel<usize>,
    wakers: HashMap<usize, std::task::Waker>,
    fired: HashSet<usize>,
}

impl Default for TimerExecutorImpl {
    fn default() -> Self {
        Self {
            timer_id_seq: 0,
            wheel: TimeWheel::new(3600),
            wakers: Default::default(),
            fired: Default::default(),
        }
    }
}

impl TimerExecutorImpl {
    fn create_timer(&mut self, duration: Duration) -> usize {
        self.timer_id_seq += 1;

        let timer = self.timer_id_seq;

        self.wheel.add(duration.as_secs(), timer);

        timer
    }

    fn poll(&mut self, timer: usize, waker: Waker) -> Poll<()> {
        if self.fired.remove(&timer) {
            Poll::Ready(())
        } else {
            log::debug!("inser timer {} waker", timer);
            self.wakers.insert(timer, waker);
            Poll::Pending
        }
    }

    fn tick(&mut self) {
        if let Poll::Ready(timers) = self.wheel.tick() {
            log::debug!("ready timers {:?}", timers);
            for timer in timers {
                self.fired.insert(timer);

                if let Some(waker) = self.wakers.remove(&timer) {
                    log::debug!("wake up timer {}", timer);
                    waker.wake_by_ref();
                }
            }
        }
    }
}

impl TimerExecutor {
    pub fn new() -> Self {
        let inner: Arc<Mutex<TimerExecutorImpl>> = Default::default();

        let inner_tick = inner.clone();

        std::thread::spawn(move || {
            let duration = std::time::Duration::new(1, 0);

            // When no other strong reference is alive, stop tick thread
            while Arc::strong_count(&inner_tick) > 1 {
                inner_tick.lock().unwrap().tick();

                std::thread::sleep(duration);
            }
        });

        Self { inner }
    }

    /// Create a new timeout future instance.
    pub fn timeout(&self, duration: Duration) -> Timeout {
        let timer_id = self.inner.lock().unwrap().create_timer(duration);

        Timeout {
            timer_id,
            executor: self.inner.clone(),
        }
    }
}

#[derive(Default, Clone)]
pub struct Timeout {
    timer_id: usize,
    executor: Arc<Mutex<TimerExecutorImpl>>,
}

impl std::future::Future for Timeout {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.executor
            .lock()
            .unwrap()
            .poll(self.timer_id, cx.waker().clone())
    }
}

#[cfg(test)]
mod tests {

    use crate::TimerExecutor;

    #[async_std::test]
    async fn test_timeout() {
        _ = pretty_env_logger::try_init();

        let executor = TimerExecutor::new();

        let timeout = executor.timeout(std::time::Duration::from_secs(5));

        let start = std::time::SystemTime::now();

        timeout.await;

        assert_eq!(
            std::time::SystemTime::now()
                .duration_since(start)
                .unwrap()
                .as_secs(),
            5
        );
    }
}

/// Accesss global static timer executor instance
pub fn global_timer_executor() -> &'static TimerExecutor {
    static INSTANCE: OnceCell<TimerExecutor> = OnceCell::new();

    INSTANCE.get_or_init(|| TimerExecutor::new())
}
