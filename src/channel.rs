use futures::{Sink, Stream};
use std::future::Future;

use crate::RPCResult;

/// Define abstract transport channel for [`crate::Client`] and [`crate::Server`]
pub trait TransportChannel: 'static {
    /// Transport channel error type.
    type SinkError: std::error::Error + 'static + Sync + Send;

    type StreamError: std::error::Error + 'static + Sync + Send;

    /// Input stream must support [`Send`] + [`Sync`]
    type Input: Stream<Item = Result<Vec<u8>, Self::StreamError>> + Unpin + Send + 'static;

    type Output: Sink<Vec<u8>, Error = Self::SinkError> + Unpin + Send + 'static;

    fn spawn<Fut>(future: Fut)
    where
        Fut: Future<Output = RPCResult<()>> + Send + 'static;

    fn framed(self) -> (Self::Input, Self::Output);
}
