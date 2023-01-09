use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::{ErrorCode, Response};

pub type ServerHandler = Box<
    dyn FnMut(Option<usize>, serde_json::Value) -> Result<Option<String>, ErrorCode>
        + Sync
        + Send
        + 'static,
>;

pub type AsyncServerHandler = Box<
    dyn FnMut(
            Option<usize>,
            serde_json::Value,
        ) -> BoxFuture<'static, Result<Option<String>, ErrorCode>>
        + Sync
        + Send
        + 'static,
>;

pub type HandlerCloner<Handler> = Box<dyn FnMut() -> Handler + Sync + Send>;

pub(crate) struct HandlerClonerRegister<Handler> {
    cloners: Arc<Mutex<HashMap<String, HandlerCloner<Handler>>>>,
}

impl<Handler> Default for HandlerClonerRegister<Handler> {
    fn default() -> Self {
        Self {
            cloners: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<Handler> Clone for HandlerClonerRegister<Handler> {
    fn clone(&self) -> Self {
        Self {
            cloners: self.cloners.clone(),
        }
    }
}

impl<Handler> HandlerClonerRegister<Handler> {
    /// Clone server method handler by `method_name`.
    pub(crate) fn clone_from(&self, method_name: &str) -> Option<Handler> {
        self.cloners
            .lock()
            .unwrap()
            .get_mut(method_name)
            .map(|h| h())
    }

    pub(crate) fn register_handler(
        &self,
        method_name: &str,
        handler_cloner: HandlerCloner<Handler>,
    ) {
        self.cloners
            .lock()
            .unwrap()
            .insert(method_name.to_string(), handler_cloner);
    }
}

pub(crate) fn to_handler<P, R, F>(method: &'static str, mut f: F) -> HandlerCloner<ServerHandler>
where
    F: FnMut(P) -> Result<Option<R>, ErrorCode> + 'static + Clone + Sync + Send,
    for<'a> P: Deserialize<'a> + Serialize,
    R: Serialize + Default,
{
    let handler = move |id, value: serde_json::Value| {
        let request = serde_json::from_value(value.clone()).map_err(|e| {
            log::error!(
                "parse method({}) params error: {}\r\t origin: {}",
                method,
                e,
                value
            );
            ErrorCode::InvalidParams
        })?;

        let response = f(request)?;

        if let Some(id) = id {
            if let Some(r) = response {
                let resp = Response::<String, R, ()> {
                    id,
                    result: Some(r),
                    ..Default::default()
                };

                let result = serde_json::to_string(&resp).map_err(|e| {
                    log::error!(
                        "parse method({}) response error: {}\r\t origin: {}",
                        method,
                        e,
                        value
                    );
                    ErrorCode::InternalError
                })?;

                return Ok(Some(result));
            }
        }

        Ok(None)
    };

    Box::new(move || Box::new(handler.clone()))
}

pub(crate) fn to_async_handler<P, R, F, FR>(
    method: &'static str,
    f: F,
) -> HandlerCloner<AsyncServerHandler>
where
    F: FnMut(P) -> FR + 'static + Sync + Send + Clone,
    FR: std::future::Future<Output = Result<Option<R>, ErrorCode>> + Sync + Send + 'static,
    for<'a> P: Deserialize<'a> + Serialize + Send,
    R: Serialize + Default,
{
    let handler = move |id,
                        value: serde_json::Value|
          -> BoxFuture<'static, Result<Option<String>, ErrorCode>> {
        let mut f_call = f.clone();
        let method_name = method.clone();
        Box::pin(async move {
            let request = serde_json::from_value(value.clone()).map_err(|e| {
                log::error!(
                    "parse method({}) params error: {}\r\t origin: {}",
                    method_name,
                    e,
                    value
                );
                ErrorCode::InvalidParams
            })?;

            let response = f_call(request).await?;

            if let Some(id) = id {
                if let Some(r) = response {
                    let resp = Response::<String, R, ()> {
                        id,
                        result: Some(r),
                        ..Default::default()
                    };

                    let result = serde_json::to_string(&resp).map_err(|e| {
                        log::error!(
                            "parse method({}) response error: {}\r\t origin: {}",
                            method_name,
                            e,
                            value
                        );
                        ErrorCode::InternalError
                    })?;

                    return Ok(Some(result));
                }
            }

            Ok::<Option<String>, ErrorCode>(None)
        })
    };

    Box::new(move || Box::new(handler.clone()))
}
