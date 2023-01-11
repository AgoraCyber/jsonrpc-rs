use futures::{SinkExt, TryStreamExt};

use crate::{
    channel::{RPCData, TransportChannel},
    Error, ErrorCode, RPCResult, Request, Response,
};

use super::handler::*;

pub struct ServiceSession<C: TransportChannel> {
    id: String,
    input: C::Input,
    output: C::Output,
    methods: HandlerClonerRegister<ServerHandler>,
    async_methods: HandlerClonerRegister<AsyncServerHandler>,
}

impl<C: TransportChannel> ServiceSession<C> {
    pub(crate) fn new(
        id: String,
        input: C::Input,
        output: C::Output,
        methods: HandlerClonerRegister<ServerHandler>,
        async_methods: HandlerClonerRegister<AsyncServerHandler>,
    ) -> Self {
        Self {
            id,
            input,
            output,
            methods,
            async_methods,
        }
    }
    pub async fn run(&mut self) -> RPCResult<()> {
        while let Some(next) = self
            .input
            .try_next()
            .await
            .map_err(|err| Error::<String, ()>::from_std_error(err))?
        {
            let request = serde_json::from_slice::<Request<&str, serde_json::Value>>(&next)?;

            if let Some(mut handler) = self.methods.clone_from(request.method) {
                self.handle_resp(
                    request.id,
                    request.method,
                    handler(request.id, request.params),
                )
                .await?;
            } else if let Some(mut handler) = self.async_methods.clone_from(request.method) {
                self.handle_resp(
                    request.id,
                    request.method,
                    handler(request.id, request.params).await,
                )
                .await?;
            }
        }

        log::info!("Server session {} stop.", self.id);

        Ok(())
    }

    async fn handle_resp(
        &mut self,
        id: Option<usize>,
        method: &str,
        result: Result<Option<RPCData>, ErrorCode>,
    ) -> RPCResult<()> {
        match result {
            Ok(Some(response)) => {
                self.output
                    .send(response)
                    .await
                    .map_err(|err| Error::<String, ()>::from_std_error(err))?;
            }
            Err(code) => {
                if let Some(id) = id {
                    let resp = Self::new_error_resp(id, code, None);
                    self.output
                        .send(resp)
                        .await
                        .map_err(|err| Error::<String, ()>::from_std_error(err))?;
                } else {
                    log::trace!("Method {} call return error, {}", method, code);
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn new_error_resp(id: usize, code: ErrorCode, message: Option<String>) -> RPCData {
        let response = Response::<String, (), ()> {
            id,
            error: Some(Error {
                code: code.clone(),
                message: message.unwrap_or(code.to_string()),
                data: None,
            }),
            ..Default::default()
        };

        serde_json::to_vec(&response)
            .expect("Inner error, serialize jsonrpc response")
            .into()
    }
}
