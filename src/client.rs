use completeq_rs::{
    oneshot::{CompleteQ, EventReceiver},
    user_event::RequestId,
    Timer, TimerWithContext,
};
use futures::{executor::ThreadPool, task::SpawnExt, Future, Sink, SinkExt, Stream, TryStreamExt};

use once_cell::sync::OnceCell;

use serde::{Deserialize, Serialize};

use crate::{Error, ErrorCode, RPCResult, Request, Response};
#[derive(Clone)]
struct EventArgument(RPCResult<serde_json::Value>);

#[derive(Clone)]
pub struct Client<Output> {
    /// Output stream
    output: Output,

    completed_q: CompleteQ<RequestId<EventArgument>>,
}

impl<Output, E> Client<Output>
where
    Output: Sink<String, Error = E> + Unpin,
    E: std::error::Error + Send + Sync + 'static,
{
    /// Create new JSONRPC client
    ///
    /// # Arguments
    ///
    /// * `input` - A [`futures::Stream`] instance channel on that recv JSONRPC message from server
    /// * `output` - A [`futures::Sink`] instance channel on that send JSONRPC message to server
    pub fn new<Input>(mut input: Input, output: Output, executor: Option<ThreadPool>) -> Self
    where
        Input: Stream<Item = Result<String, E>> + Unpin + Send + Sync + 'static,
    {
        let executor = executor.unwrap_or(global_executor().clone());

        let completed_q: CompleteQ<RequestId<EventArgument>> = CompleteQ::new();

        let recv_loop_callbacks = completed_q.clone();

        let recv_loop = async move {
            loop {
                let data = match input.try_next().await {
                    Ok(Some(data)) => data,
                    Err(err) => {
                        log::error!("Error raise from input stream {}", err);
                        break;
                    }
                    _ => {
                        log::info!("Input stream closed");
                        break;
                    }
                };

                let response = serde_json::from_str::<Response<String, serde_json::Value, ()>>(
                    &data,
                )
                .map_err(|e| Error::<String, ()> {
                    code: ErrorCode::ParseError,
                    message: format!("Parse response error, {}\r\t{}", e, data),
                    data: None,
                });

                match response {
                    Ok(response) => {
                        if let Some(result) = response.result {
                            recv_loop_callbacks
                                .complete_one(response.id, EventArgument(Ok(result)));
                        } else if let Some(err) = response.error {
                            recv_loop_callbacks.complete_one(response.id, EventArgument(Err(err)));
                        }
                    }
                    Err(err) => {
                        log::trace!("recv unexpect response, {}", err.message);
                    }
                }
            }
        };

        _ = executor.spawn(recv_loop);

        Self {
            output,

            completed_q,
        }
    }

    pub fn call<'a, P, R>(
        &'a mut self,
        method: &str,
        params: P,
    ) -> impl Future<Output = Result<R, Error<String, ()>>> + 'a
    where
        P: Default + Serialize,
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        let receiver = self.completed_q.wait_one();

        let request = Request {
            id: Some(receiver.event_id()),
            method,
            params,
            ..Default::default()
        };

        let data = serde_json::to_string(&request).expect("Inner error, assembly json request");

        self.send_request(receiver, data)
    }

    pub fn call_with_timer<'a, P, R, T>(
        &'a mut self,
        method: &str,
        params: P,
        timer: T,
    ) -> impl Future<Output = RPCResult<R>> + 'a
    where
        P: Default + Serialize,
        for<'b> R: Deserialize<'b> + Send + 'static,
        T: TimerWithContext + Unpin + 'static,
    {
        let receiver = self.completed_q.wait_one_with_timer(timer);

        let request = Request {
            id: Some(receiver.event_id()),
            method,
            params,
            ..Default::default()
        };

        let data = serde_json::to_string(&request).expect("Inner error, assembly json request");

        self.send_request(receiver, data)
    }

    async fn send_request<R, T: Timer + Unpin>(
        &mut self,
        receiver: EventReceiver<RequestId<EventArgument>, T>,
        data: String,
    ) -> RPCResult<R>
    where
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        self.output
            .send(data)
            .await
            .map_err(|e| Error::<String, ()> {
                code: ErrorCode::InternalError,
                message: format!("Send message failed, {}", e),
                data: None,
            })?;

        let value = receiver
            .await
            .success()
            .map_err(|err| Error::<String, ()> {
                code: ErrorCode::InternalError,
                message: format!("Send message failed, {}", err),
                data: None,
            })?
            .0?;

        serde_json::from_value(value.clone()).map_err(|e| Error::<String, ()> {
            code: ErrorCode::ParseError,
            message: format!("Parse response error, {}\r\t{}", e, value),
            data: None,
        })
    }

    pub async fn notification<P>(&mut self, method: &str, params: P) -> anyhow::Result<()>
    where
        P: Default + Serialize,
    {
        let request = Request {
            method,
            params,
            ..Default::default()
        };

        let data = serde_json::to_string(&request)?;

        self.output.send(data).await?;

        Ok(())
    }
}

/// Accesss global static rpc executor instance
pub fn global_executor() -> &'static ThreadPool {
    static INSTANCE: OnceCell<ThreadPool> = OnceCell::new();

    INSTANCE.get_or_init(|| ThreadPool::new().unwrap())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use async_timer::{hashed::Timeout, Timer};
    use completeq_rs::error::CompleteQError;
    use futures::{sink, stream};
    use serde_json::json;

    use crate::{Client, Error, ErrorCode};

    #[async_std::test]
    async fn test_client() -> Result<(), Error<String, ()>> {
        _ = pretty_env_logger::try_init();

        let input = stream::iter(vec![
            Ok(json!({
                "result":"hello",
                "jsonrpc":"2.0",
                "id":1
            })
            .to_string()),
            Ok(json!({
                "result":"world",
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

        let mut client = Client::new(input, output, None);

        let result: Result<String, Error<String, ()>> = client
            .call_with_timer("echo", "timeout", Timeout::new(Duration::from_secs(2)))
            .await;

        assert_eq!(
            result,
            Err(Error::<String, ()> {
                code: ErrorCode::InternalError,
                message: format!("Send message failed, {}", CompleteQError::Timeout),
                data: None,
            })
        );

        Ok(())
    }
}
