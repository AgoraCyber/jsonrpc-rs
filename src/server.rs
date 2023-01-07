use std::collections::HashMap;

use futures::{future::BoxFuture, Sink, SinkExt, Stream, TryStreamExt};
use serde::{Deserialize, Serialize};

use crate::{Error, ErrorCode, Request, Response};

type InnerServerHandler =
    Box<dyn FnMut(Option<usize>, serde_json::Value) -> Result<Option<String>, ErrorCode> + 'static>;

type InnerAsyncServerHandler = Box<
    dyn FnMut(
            Option<usize>,
            serde_json::Value,
        ) -> BoxFuture<'static, Result<Option<String>, ErrorCode>>
        + 'static,
>;

/// JSONRPC server context structure.
///
///
#[derive(Default)]
pub struct Server {
    methods: HashMap<String, InnerServerHandler>,
    async_methods: HashMap<String, InnerAsyncServerHandler>,
}

impl Server {
    /// Register jsonrpc server sync handler
    pub fn handle<P, R, F>(mut self, method: &'static str, mut f: F) -> Self
    where
        F: FnMut(P) -> Result<Option<R>, ErrorCode> + 'static,
        for<'a> P: Deserialize<'a> + Serialize,
        R: Serialize + Default,
    {
        // Create InnerServerHandler
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

        self.methods.insert(method.to_string(), Box::new(handler));

        self
    }

    /// Register jsonrpc server async handler
    ///
    /// The register async handler be required to implement [`Clone`] trait.
    pub fn async_handle<P, R, F, FR>(mut self, method: &'static str, f: F) -> Self
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

        self.async_methods
            .insert(method.to_string(), Box::new(handler));

        self
    }

    pub async fn accept<Input, Output, E>(
        &mut self,
        mut input: Input,
        mut output: Output,
    ) -> anyhow::Result<()>
    where
        Input: Stream<Item = Result<String, E>> + Unpin,
        Output: Sink<String, Error = E> + Unpin,
        E: std::error::Error + Sync + Send + 'static,
    {
        while let Some(next) = input.try_next().await? {
            match serde_json::from_str::<Request<&str, serde_json::Value>>(&next) {
                Ok(request) => {
                    if let Some(handler) = self.methods.get_mut(request.method) {
                        // Call sync server handler
                        Self::handle_resp(
                            request.id,
                            request.method,
                            handler(request.id, request.params),
                            &mut output,
                        )
                        .await?;
                    } else if let Some(handler) = self.async_methods.get_mut(request.method) {
                        // Call async server handler

                        Self::handle_resp(
                            request.id,
                            request.method,
                            handler(request.id, request.params).await,
                            &mut output,
                        )
                        .await?;
                    } else if let Some(id) = request.id {
                        let resp = Self::new_error_resp(
                            id,
                            ErrorCode::MethodNotFound,
                            Some(format!("Method not found {}", request.method)),
                        );

                        output.send(resp).await?;
                    } else {
                        log::trace!("Method not found {}", request.method);
                    }
                }
                Err(err) => {
                    log::error!("Invalid JSONRPC call, {}\r\t source: {}", err, next);
                }
            }
        }

        Ok(())
    }

    async fn handle_resp<Output, E>(
        id: Option<usize>,
        method: &str,
        result: Result<Option<String>, ErrorCode>,
        output: &mut Output,
    ) -> anyhow::Result<()>
    where
        Output: Sink<String, Error = E> + Unpin,
        E: std::error::Error + Sync + Send + 'static,
    {
        match result {
            Ok(Some(response)) => {
                output.send(response).await?;
            }
            Err(code) => {
                if let Some(id) = id {
                    let resp = Self::new_error_resp(id, code, None);
                    output.send(resp).await?;
                } else {
                    log::trace!("Method {} call return error, {}", method, code);
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn new_error_resp(id: usize, code: ErrorCode, message: Option<String>) -> String {
        let response = Response::<String, (), ()> {
            id,
            error: Some(Error {
                code: code.clone(),
                message: message.unwrap_or(code.to_string()),
                data: None,
            }),
            ..Default::default()
        };

        serde_json::to_string(&response).expect("Inner error, serialize jsonrpc response")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures::{sink, stream};
    use serde_json::json;

    use crate::Server;

    #[async_std::test]
    async fn test_server_async_handlers() {
        _ = pretty_env_logger::try_init();

        let mut server =
            Server::default().async_handle("echo", |msg: String| async { Ok(Some(msg)) });

        let input = stream::iter(vec![
            Ok(json!({
                "method":"echo",
                "params":"hello",
                "jsonrpc":"2.0",
                "id":1
            })
            .to_string()),
            Ok(json!({
                "method":"echo",
                "params":"world",
                "jsonrpc":"2.0",
                "id":2
            })
            .to_string()),
        ]);

        let result = Arc::new(Mutex::new(vec![]));

        let output = sink::unfold((), |_, msg: String| async {
            result.lock().unwrap().push(msg);
            Ok::<_, futures::never::Never>(())
        });

        futures::pin_mut!(output);

        server.accept(input, output).await.unwrap();

        let result = result.lock().unwrap();

        // test response num
        assert_eq!(result.len(), 2);

        assert_eq!(
            result[0],
            json!({"result":"hello","id":1,"jsonrpc":"2.0"}).to_string()
        );

        assert_eq!(
            result[1],
            json!({"result":"world","id":2,"jsonrpc":"2.0"}).to_string()
        );
    }

    #[async_std::test]
    async fn test_server_sync_handlers() {
        _ = pretty_env_logger::try_init();

        let mut server = Server::default().handle("echo", |msg: String| Ok(Some(msg)));

        let input = stream::iter(vec![
            Ok(json!({
                "method":"echo",
                "params":"hello",
                "jsonrpc":"2.0",
                "id":1
            })
            .to_string()),
            Ok(json!({
                "method":"echo",
                "params":"world",
                "jsonrpc":"2.0",
                "id":2
            })
            .to_string()),
        ]);

        let result = Arc::new(Mutex::new(vec![]));

        let output = sink::unfold((), |_, msg: String| async {
            result.lock().unwrap().push(msg);
            Ok::<_, futures::never::Never>(())
        });

        futures::pin_mut!(output);

        server.accept(input, output).await.unwrap();

        let result = result.lock().unwrap();

        // test response num
        assert_eq!(result.len(), 2);

        assert_eq!(
            result[0],
            json!({"result":"hello","id":1,"jsonrpc":"2.0"}).to_string()
        );

        assert_eq!(
            result[1],
            json!({"result":"world","id":2,"jsonrpc":"2.0"}).to_string()
        );
    }
}
