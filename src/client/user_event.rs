use completeq_rs::{oneshot::CompleteQ, user_event::RPCResponser};

use crate::RPCResult;

pub(crate) type ResponserArgument = RPCResult<serde_json::Value>;

pub(crate) type RPCEvent = RPCResponser<ResponserArgument>;

pub(crate) type RPCCompletedQ = CompleteQ<RPCEvent>;
