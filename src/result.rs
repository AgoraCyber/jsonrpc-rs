use crate::Error;

pub type RPCResult<T> = Result<T, Error<String, ()>>;
