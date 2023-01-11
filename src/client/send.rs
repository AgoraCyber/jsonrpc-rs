use futures::{channel::mpsc::Receiver, SinkExt, StreamExt};

use crate::{channel::TransportChannel, Error, RPCResult, Request};

use super::user_event::RPCCompletedQ;

pub async fn send_loop<C: TransportChannel, S: AsRef<str>>(
    client_id: S,
    mut output: C::Output,
    mut output_receiver: Receiver<Vec<u8>>,
    completed_q: RPCCompletedQ,
) -> RPCResult<()> {
    while let Some(item) = output_receiver.next().await {
        match output.send(item.clone()).await {
            Err(err) => {
                let request: Request<String, serde_json::Value> =
                    serde_json::from_slice(&item).expect("Parse send json error");

                log::error!("RPC client send msg error, {}", err);

                if let Some(id) = request.id {
                    completed_q.complete_one(id, Err(Error::<String, ()>::from_std_error(err)));
                }
            }
            _ => {}
        }
    }

    log::info!("rpc client {} send_loop stop.", client_id.as_ref());

    Ok(())
}
