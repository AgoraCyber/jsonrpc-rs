// use std::time::Duration;

use std::time::Duration;

use async_timer_rs::hashed::Timeout;
use async_timer_rs::Timer;
use criterion::Criterion;
use criterion::{criterion_group, criterion_main};

// This is a struct that tells Criterion.rs to use the "futures" crate's current-thread executor
use criterion::async_executor::FuturesExecutor;

use futures::{
    channel::mpsc::{self, SendError, Sender},
    executor::ThreadPool,
    stream::BoxStream,
    task::SpawnExt,
    StreamExt,
};
use jsonrpc_rs::channel::RPCData;
use jsonrpc_rs::RPCError;
use jsonrpc_rs::{channel::TransportChannel, Client, RPCResult, Server};
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

async fn prepare_bench() -> RPCResult<(Server, Client)> {
    let (server_output, client_input) = mpsc::channel(20);

    let (client_output, server_input) = mpsc::channel(20);

    let server_transport = MPSCTransportChannel(server_input.map(|c| Ok(c)).boxed(), server_output);

    let client_transport = MPSCTransportChannel(client_input.map(|c| Ok(c)).boxed(), client_output);

    let mut server = Server::default()
        .async_handle("echo", |msg: String| async { Ok(Some(msg)) })
        .handle("event", |msg: String| {
            log::debug!("{}", msg);
            Ok(None::<String>)
        });

    server.accept(server_transport);

    Ok((server, Client::new("Test", client_transport)))
}

async fn blocking_pingpong(mut client: Client) -> RPCResult<()> {
    let echo: String = client.call("echo", "world").await?;

    assert_eq!(echo, "world");

    Ok(())
}

async fn timeout_pingpong(mut client: Client) -> RPCResult<()> {
    let echo: String = client
        .call_with_timer("echo", "world", Timeout::new(Duration::from_secs(5)))
        .await?;

    assert_eq!(echo, "world");

    Ok(())
}

fn call_benchmark(c: &mut Criterion) {
    let (_server, client) = async_std::task::block_on(async { prepare_bench().await.unwrap() });

    // let mut group = c.benchmark_group("jsonrpc");

    // group.measurement_time(Duration::from_secs(10));

    c.bench_function("blocking pingpong", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| blocking_pingpong(client.clone()));
    });

    c.bench_function("timeout pingpong", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| timeout_pingpong(client.clone()));
    });

    // group.finish();
}

criterion_group!(benches, call_benchmark);

criterion_main!(benches);
