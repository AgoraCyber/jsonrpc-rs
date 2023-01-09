use jsonrpc_rs::{RPCResult, Server};

#[async_std::test]
async fn pingpong() -> RPCResult<()> {
    _ = pretty_env_logger::try_init();

    let mut _server = Server::default().async_handle("echo", |msg: String| async { Ok(Some(msg)) });

    Ok(())
}
