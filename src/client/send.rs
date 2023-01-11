use futures::{channel::mpsc::Receiver, SinkExt, StreamExt};

use crate::{
    channel::{RPCData, TransportChannel},
    map_error, RPCResult, Request,
};

use super::user_event::RPCCompletedQ;

pub async fn send_loop<C: TransportChannel, S: AsRef<str>>(
    client_id: S,
    mut output: C::Output,
    mut output_receiver: Receiver<RPCData>,
    completed_q: RPCCompletedQ,
) -> RPCResult<()> {
    while let Some(item) = output_receiver.next().await {
        match output.send(item.clone()).await {
            Err(err) => {
                let request: Request<String, serde_json::Value> =
                    serde_json::from_slice(&item).expect("Parse send json error");

                log::error!("RPC client send msg error, {}", err);

                if let Some(id) = request.id {
                    completed_q.complete_one(id, Err(map_error(err)));
                }
            }
            _ => {}
        }
    }

    log::info!("rpc client {} send_loop stop.", client_id.as_ref());

    Ok(())
}
