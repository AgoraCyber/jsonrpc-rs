use futures::{channel::mpsc::Receiver, SinkExt, StreamExt};

use crate::{channel::TransportChannel, Error, RPCResult};

pub async fn send_loop<C: TransportChannel, S: AsRef<str>>(
    client_id: S,
    mut output: C::Output,
    mut output_receiver: Receiver<String>,
) -> RPCResult<()> {
    while let Some(item) = output_receiver.next().await {
        match output.send(item).await {
            Err(err) => {
                log::error!("RPC client send msg error, {}", err);
                return Err(Error::<String, ()>::from_std_error(err));
            }
            _ => {}
        }
    }

    log::info!("rpc client {} send_loop stop.", client_id.as_ref());

    Ok(())
}
