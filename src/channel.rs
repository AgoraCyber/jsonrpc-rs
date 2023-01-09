use futures::{Sink, Stream};
use std::future::Future;

/// Define abstract transport channel for [`crate::Client`] and [`crate::Server`]
pub trait TransportChannel {
    /// Transport channel error type.
    type Error: std::error::Error + 'static + Sync + Send;

    /// Input stream must support [`Send`] + [`Sync`]
    type Input: Stream<Item = Result<String, Self::Error>> + Unpin + Send + Sync + 'static;

    type Output: Sink<String, Error = Self::Error> + Unpin + Send + Sync + 'static;

    fn spawn<Fut>(&self, future: Fut)
    where
        Fut: Future<Output = ()> + Send + 'static;
}
