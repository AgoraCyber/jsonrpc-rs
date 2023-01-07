use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Poll, Waker},
};

#[derive(Default)]
struct CallbackPoolImpl<Output> {
    wakers: HashMap<usize, Waker>,
    fired: HashMap<usize, Output>,
}

impl<Output> CallbackPoolImpl<Output> {
    fn new() -> Self {
        Self {
            wakers: Default::default(),
            fired: HashMap::new(),
        }
    }
}

impl<Output> CallbackPoolImpl<Output> {
    fn poll(&mut self, id: usize, waker: Waker) -> Poll<Output> {
        log::trace!("poll joinable {}", id);
        if let Some(data) = self.fired.remove(&id) {
            log::trace!("joinable {} is ready", id);
            Poll::Ready(data)
        } else {
            log::trace!("joinable {} is waiting", id);
            self.wakers.insert(id, waker);
            Poll::Pending
        }
    }

    fn complete(&mut self, id: usize, output: Output) {
        log::trace!("wakeup joinable {}", id);
        self.fired.insert(id, output);

        if let Some(waker) = self.wakers.remove(&id) {
            waker.wake_by_ref();
        }
    }
}

#[derive(Clone)]
pub struct CallbackPool<Output> {
    seq: Arc<AtomicUsize>,
    inner: Arc<Mutex<CallbackPoolImpl<Output>>>,
}

impl<Output> Default for CallbackPool<Output> {
    fn default() -> Self {
        Self {
            seq: Arc::new(AtomicUsize::new(1)),
            inner: Arc::new(Mutex::new(CallbackPoolImpl::new())),
        }
    }
}

impl<Output> CallbackPool<Output> {
    pub fn join(&self) -> Joinable<Output> {
        let id = self.seq.fetch_add(1, Ordering::SeqCst);
        Joinable {
            id,
            inner: self.inner.clone(),
        }
    }

    pub fn complete(&self, id: usize, output: Output) {
        self.inner.lock().unwrap().complete(id, output)
    }
}

pub struct Joinable<Output> {
    pub id: usize,
    inner: Arc<Mutex<CallbackPoolImpl<Output>>>,
}

impl<Output> std::future::Future for Joinable<Output> {
    type Output = Output;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.inner.lock().unwrap().poll(self.id, cx.waker().clone())
    }
}
