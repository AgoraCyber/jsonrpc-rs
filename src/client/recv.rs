use futures::TryStreamExt;

use crate::{channel::TransportChannel, Error, RPCResult, Response};

use super::user_event::RPCCompletedQ;

pub async fn recv_loop<C: TransportChannel, S: AsRef<str>>(
    client_id: S,
    mut input: C::Input,
    completed_q: RPCCompletedQ,
) -> RPCResult<()> {
    loop {
        let data = match input.try_next().await {
            Ok(Some(data)) => data,
            Err(err) => {
                log::error!("Error raise from input stream {}", err);
                completed_q.cancel_all();
                break;
            }
            _ => {
                break;
            }
        };

        let response = serde_json::from_slice::<Response<String, serde_json::Value, ()>>(&data)
            .map_err(|e| Error::<String, ()>::from(e))?;

        if let Some(result) = response.result {
            completed_q.complete_one(response.id, Ok(result));
        } else if let Some(err) = response.error {
            completed_q.complete_one(response.id, Err(err));
        }
    }

    log::info!("rpc client {} recv_loop stop.", client_id.as_ref());

    Ok(())
}
