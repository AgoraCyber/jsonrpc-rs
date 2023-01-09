use async_timer::hashed::Timeout;
use completeq_rs::{
    oneshot::{CompleteQ, EventReceiver},
    user_event::RequestId,
    Timer,
};
use futures::{executor::ThreadPool, task::SpawnExt, Sink, SinkExt, Stream, TryStreamExt};

use once_cell::sync::OnceCell;

use serde::{Deserialize, Serialize};

use crate::{Error, RPCResult, Request, Response};
#[derive(Clone)]
struct EventArgument(RPCResult<serde_json::Value>);

type RPCEvent = RequestId<EventArgument>;

async fn response_loop<Input, E>(mut input: Input, completed_q: CompleteQ<RPCEvent>)
where
    Input: Stream<Item = Result<String, E>> + Unpin + Send + Sync + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
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

        let response = serde_json::from_str::<Response<String, serde_json::Value, ()>>(&data)
            .map_err(|e| Error::<String, ()>::from(e));

        match response {
            Ok(response) => {
                if let Some(result) = response.result {
                    completed_q.complete_one(response.id, EventArgument(Ok(result)));
                } else if let Some(err) = response.error {
                    completed_q.complete_one(response.id, EventArgument(Err(err)));
                }
            }
            Err(err) => {
                log::trace!("recv unexpect response, {}", err.message);
            }
        }
    }
}

#[derive(Clone)]
pub struct Client<Output> {
    /// Output stream
    output: Output,

    completed_q: CompleteQ<RPCEvent>,
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
    pub fn new<Input>(input: Input, output: Output, executor: Option<ThreadPool>) -> Self
    where
        Input: Stream<Item = Result<String, E>> + Unpin + Send + Sync + 'static,
    {
        let executor = executor.unwrap_or(global_executor().clone());

        let completed_q: CompleteQ<RPCEvent> = CompleteQ::new();

        _ = executor.spawn(response_loop(input, completed_q.clone()));

        Self {
            output,

            completed_q,
        }
    }

    pub async fn send<P>(&mut self, method: &str, params: P) -> RPCResult<Responser<Timeout>>
    where
        P: Default + Serialize,
    {
        let receiver = self.completed_q.wait_one();

        let request = Request {
            id: Some(receiver.event_id()),
            method,
            params,
            ..Default::default()
        };

        let data = serde_json::to_string(&request).expect("Inner error, assembly json request");

        self.output
            .send(data)
            .await
            .map_err(|e| Error::<String, ()>::from_std_error(e))?;

        Ok(Responser {
            receiver: Some(receiver),
        })
    }

    pub async fn call<P, R>(&mut self, method: &str, params: P) -> RPCResult<R>
    where
        P: Default + Serialize,
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        self.send(method, params).await?.recv().await
    }

    pub async fn send_with_timer<P, T>(
        &mut self,
        method: &str,
        params: P,
        timer: T,
    ) -> RPCResult<Responser<T>>
    where
        P: Default + Serialize,
        T: Timer + Unpin + 'static,
    {
        let receiver = self.completed_q.wait_one_with_timer(timer);

        let request = Request {
            id: Some(receiver.event_id()),
            method,
            params,
            ..Default::default()
        };

        let data = serde_json::to_string(&request).expect("Inner error, assembly json request");

        self.output
            .send(data)
            .await
            .map_err(|e| Error::<String, ()>::from_std_error(e))?;

        Ok(Responser {
            receiver: Some(receiver),
        })
    }

    pub async fn call_with_timer<P, T, R>(
        &mut self,
        method: &str,
        params: P,
        timer: T,
    ) -> RPCResult<R>
    where
        T: Timer + Unpin + 'static,
        P: Default + Serialize,
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        self.send_with_timer(method, params, timer)
            .await?
            .recv()
            .await
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

pub struct Responser<T: Timer> {
    receiver: Option<EventReceiver<RPCEvent, T>>,
}

impl<T: Timer> Responser<T>
where
    T: Unpin,
{
    pub async fn recv<R>(&mut self) -> RPCResult<R>
    where
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        let value = self
            .receiver
            .take()
            .unwrap()
            .await
            .success()
            .map_err(|err| Error::<String, ()>::from(err))?
            .0?;

        serde_json::from_value(value.clone()).map_err(|e| Error::<String, ()>::from(e))
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

    use crate::{Client, Error, ErrorCode, RPCResult};

    #[async_std::test]
    async fn test_client() -> RPCResult<()> {
        _ = pretty_env_logger::try_init();

        let input = stream::iter(vec![]);

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
                message: format!("RPC call channel broken: {}", CompleteQError::Timeout),
                data: None,
            })
        );

        Ok(())
    }
}
