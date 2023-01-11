mod recv;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_timer_rs::{hashed::Timeout, Timer};
use completeq_rs::oneshot::EventReceiver;
use futures::{
    channel::mpsc::{self, Sender},
    SinkExt,
};
use recv::*;
mod send;
use send::*;
mod user_event;
use serde::{Deserialize, Serialize};
use user_event::*;

use crate::{
    channel::{RPCData, TransportChannel},
    map_error, RPCResult, Request,
};

#[derive(Clone)]
pub struct Client {
    output_sender: Sender<RPCData>,
    completed_q: RPCCompletedQ,
}

impl Client {
    pub fn new<C, S>(tag: S, channel: C) -> Self
    where
        C: TransportChannel,
        S: AsRef<str>,
    {
        static ID: AtomicUsize = AtomicUsize::new(1);

        let client_id = format!("{}_{}", tag.as_ref(), ID.fetch_add(1, Ordering::SeqCst));

        let (output_sender, output_receiver) = mpsc::channel(100);

        let completed_q = RPCCompletedQ::new();

        let (input, output) = channel.framed();

        C::spawn(send_loop::<C, String>(
            client_id.clone(),
            output,
            output_receiver,
            completed_q.clone(),
        ));

        C::spawn(recv_loop::<C, String>(
            client_id,
            input,
            completed_q.clone(),
        ));

        Self {
            output_sender,
            completed_q,
        }
    }

    pub async fn send<P>(&mut self, method: &str, params: P) -> RPCResult<Responser<Timeout>>
    where
        P: Serialize,
    {
        let receiver = self.completed_q.wait_one();

        let request = Request {
            id: Some(receiver.event_id()),
            method,
            params,
            jsonrpc: crate::Version::default(),
        };

        let data = serde_json::to_vec(&request).expect("Inner error, assembly json request");

        self.output_sender
            .send(data.into())
            .await
            .map_err(map_error)?;

        Ok(Responser {
            receiver: Some(receiver),
        })
    }

    pub async fn call<P, R>(&mut self, method: &str, params: P) -> RPCResult<R>
    where
        P: Serialize,
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        self.send(method, params).await?.recv().await
    }

    pub async fn send_with_timer<P, T>(
        &mut self,
        method: &str,
        params: P,
        timer: T,
    ) -> RPCResult<Responser<T>>
    where
        P: Serialize,
        T: Timer + Unpin + 'static,
    {
        let receiver = self.completed_q.wait_one_with_timer(timer);

        let request = Request {
            id: Some(receiver.event_id()),
            method,
            params,
            jsonrpc: crate::Version::default(),
        };

        let data = serde_json::to_vec(&request).expect("Inner error, assembly json request");

        self.output_sender
            .send(data.into())
            .await
            .map_err(map_error)?;

        Ok(Responser {
            receiver: Some(receiver),
        })
    }

    pub async fn call_with_timer<P, T, R>(
        &mut self,
        method: &str,
        params: P,
        timer: T,
    ) -> RPCResult<R>
    where
        T: Timer + Unpin + 'static,
        P: Serialize,
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        self.send_with_timer(method, params, timer)
            .await?
            .recv()
            .await
    }

    pub async fn notification<P>(&mut self, method: &str, params: P) -> RPCResult<()>
    where
        P: Serialize,
    {
        let request = Request {
            method,
            params,
            id: None,
            jsonrpc: crate::Version::default(),
        };

        let data = serde_json::to_vec(&request)?;

        self.output_sender
            .send(data.into())
            .await
            .map_err(map_error)?;

        Ok(())
    }
}

pub struct Responser<T: Timer> {
    receiver: Option<EventReceiver<RPCEvent, T>>,
}

impl<T: Timer> Responser<T>
where
    T: Unpin,
{
    pub async fn recv<R>(&mut self) -> RPCResult<R>
    where
        for<'b> R: Deserialize<'b> + Send + 'static,
    {
        let value = self
            .receiver
            .take()
            .unwrap()
            .await
            .success()
            .map_err(map_error)??;

        serde_json::from_value(value.clone()).map_err(map_error)
    }
}
