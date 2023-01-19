mod handler;
use std::sync::atomic::{AtomicUsize, Ordering};

use handler::*;

mod session;
use session::ServiceSession;

use serde::{Deserialize, Serialize};

use crate::{channel::TransportChannel, RPCResult};

/// JSONRPC server context structure.
///
#[derive(Default, Clone)]
pub struct Server {
    tag: String,
    methods: HandlerClonerRegister<ServerHandler>,
    async_methods: HandlerClonerRegister<AsyncServerHandler>,
}

impl Server {
    pub fn new<S>(tag: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            tag: tag.into(),
            ..Default::default()
        }
    }
    /// Register jsonrpc server sync handler
    pub fn handle<P, R, F>(&mut self, method: &'static str, f: F) -> &mut Self
    where
        F: FnMut(P) -> RPCResult<Option<R>> + 'static + Clone + Sync + Send,
        for<'a> P: Deserialize<'a> + Serialize,
        R: Serialize + Default,
    {
        self.methods.register_handler(method, to_handler(method, f));

        self
    }

    /// Register jsonrpc server async handler
    ///
    /// The register async handler be required to implement [`Clone`] trait.
    ///
    ///
    pub fn async_handle<P, R, F, FR>(&mut self, method: &'static str, f: F) -> &mut Self
    where
        F: FnMut(P) -> FR + 'static + Sync + Send + Clone,
        FR: std::future::Future<Output = RPCResult<Option<R>>> + Sync + Send + 'static,
        for<'a> P: Deserialize<'a> + Serialize + Send,
        R: Serialize + Default,
    {
        self.async_methods
            .register_handler(method, to_async_handler(method, f));

        self
    }

    pub fn accept<C: TransportChannel>(&mut self, channel: C) {
        static INSTANCE: AtomicUsize = AtomicUsize::new(1);

        let id = format!("{}_{}", self.tag, INSTANCE.fetch_add(1, Ordering::SeqCst));

        let (input, output) = channel.framed();

        let mut session = ServiceSession::<C>::new(
            id,
            input,
            output,
            self.methods.clone(),
            self.async_methods.clone(),
        );

        C::spawn(async move { session.run().await });
    }
}
