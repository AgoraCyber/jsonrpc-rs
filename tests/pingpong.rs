use std::time::Duration;

use async_std::task::spawn;
use async_timer_rs::{hashed::Timeout, Timer};
use futures::{
    channel::mpsc::{self, SendError, Sender},
    executor::ThreadPool,
    stream::BoxStream,
    task::SpawnExt,
    StreamExt,
};
use jsonrpc_rs::{
    channel::{RPCData, TransportChannel},
    Client, RPCError, RPCResult, Server,
};
use once_cell::sync::OnceCell;

struct MPSCTransportChannel(BoxStream<'static, RPCResult<RPCData>>, Sender<RPCData>);

impl TransportChannel for MPSCTransportChannel {
    type StreamError = RPCError;

    type SinkError = SendError;

    type Input = BoxStream<'static, RPCResult<RPCData>>;

    type Output = Sender<RPCData>;

    fn spawn<Fut>(future: Fut)
    where
        Fut: futures::Future<Output = RPCResult<()>> + Send + 'static,
    {
        static INSTANCE: OnceCell<ThreadPool> = OnceCell::new();

        let executor = INSTANCE.get_or_init(|| ThreadPool::new().unwrap());

        _ = executor.spawn(async move {
            _ = future.await;
        });
    }

    fn framed(self) -> (Self::Input, Self::Output) {
        (self.0, self.1)
    }
}

#[async_std::test]
async fn pingpong() -> RPCResult<()> {
    _ = pretty_env_logger::try_init();

    let (server_output, client_input) = mpsc::channel(20);

    let (client_output, server_input) = mpsc::channel(20);

    let server_transport = MPSCTransportChannel(server_input.map(|c| Ok(c)).boxed(), server_output);

    let client_transport = MPSCTransportChannel(client_input.map(|c| Ok(c)).boxed(), client_output);

    let mut server = Server::default();

    server
        .async_handle("echo", |msg: String| async { Ok(Some(msg)) })
        .handle("event", |msg: String| {
            log::debug!("{}", msg);
            Ok(None::<String>)
        });

    server.accept(server_transport);

    // spawn(async move {
    //     server.accept(server_input.map(|c| Ok(c)), server_output);

    //     Ok(())
    // });

    let mut client = Client::new("Test", client_transport);

    let echo: String = client.call("echo", "hello").await?;

    assert_eq!(echo, "hello");

    let echo: String = client.call("echo", "world").await?;

    assert_eq!(echo, "world");

    let echo: String = client
        .call_with_timer("echo", "hello", Timeout::new(Duration::from_secs(10)))
        .await?;

    assert_eq!(echo, "hello");

    let echo: String = client
        .call_with_timer("echo", "world", Timeout::new(Duration::from_secs(10)))
        .await?;

    assert_eq!(echo, "world");

    let mut client2 = client.clone();

    spawn(async move {
        let echo: String = client2.call("echo", "clone_instance").await?;

        assert_eq!(echo, "clone_instance");

        Ok::<(), RPCError>(())
    })
    .await?;

    Ok(())
}
