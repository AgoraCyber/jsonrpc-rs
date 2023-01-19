use futures::TryStreamExt;

use crate::{channel::TransportChannel, map_error, RPCResult, Response};

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

        let response =
            serde_json::from_slice::<Response<String, serde_json::Value, serde_json::Value>>(&data)
                .map_err(map_error);

        match response {
            Ok(response) => {
                log::trace!("parsed response: {:?}", response);
                if let Some(result) = response.result {
                    log::trace!("response {} with result: {}", response.id, result);
                    completed_q.complete_one(response.id, Ok(result));
                } else if let Some(err) = response.error {
                    log::trace!("response {} with error: {}", response.id, err);
                    completed_q.complete_one(response.id, Err(err));
                } else {
                    completed_q.complete_one(response.id, Ok(serde_json::Value::Null));
                    log::trace!("response {} with null result", response.id);
                }
            }
            Err(err) => {
                log::error!("parse response error,{}", err);
                log::error!("response {}", String::from_utf8_lossy(&data));
                completed_q.cancel_all();
                return Err(err);
            }
        }
    }

    completed_q.cancel_all();

    log::info!("rpc client {} recv_loop stop.", client_id.as_ref());

    Ok(())
}
