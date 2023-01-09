use std::time::Duration;

use async_std::{stream::StreamExt, task::spawn};
use async_timer::{hashed::Timeout, Timer};
use futures::channel::mpsc;
use jsonrpc_rs::{Client, RPCResult, Server};

#[async_std::test]
async fn pingpong() -> RPCResult<()> {
    _ = pretty_env_logger::try_init();

    let (server_output, client_input) = mpsc::channel(20);

    let (client_output, server_input) = mpsc::channel(20);

    let mut server = Server::default()
        .async_handle("echo", |msg: String| async { Ok(Some(msg)) })
        .handle("event", |msg: String| {
            log::debug!("{}", msg);
            Ok(None::<String>)
        });

    spawn(async move {
        server
            .accept(server_input.map(|c| Ok(c)), server_output)
            .await
            .unwrap();
    });

    let mut client = Client::new(client_input.map(|c| Ok(c)), client_output, None);

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

    Ok(())
}
