use crate::Error;

pub type RPCResult<T> = Result<T, Error<String, serde_json::Value>>;
pub type RPCError = Error<String, serde_json::Value>;
