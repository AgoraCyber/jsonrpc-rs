use serde::*;

/// A rpc call is represented by sending a Request object to a Server.  
///
/// visit [`here`](https://www.jsonrpc.org/specification) for details
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Request<S, P>
where
    S: AsRef<str>,
{
    /// An identifier established by the Client that MUST contain a String, Number,
    /// or NULL value if included. If it is not included it is assumed to be a notification.
    /// The value SHOULD normally not be Null and Numbers SHOULD NOT contain fractional parts
    pub id: Option<usize>,
    /// A String specifying the version of the JSON-RPC protocol. MUST be exactly "2.0".
    pub jsonrpc: Version,
    /// A String containing the name of the method to be invoked. Method names
    /// that begin with the word rpc followed by a period character (U+002E or ASCII 46)
    /// are reserved for rpc-internal methods and extensions and MUST NOT be used for anything else
    pub method: S,
    /// A Structured value that holds the parameter values to be used during the invocation of the method. This member MAY be omitted.
    pub params: P,
}

#[derive(Debug, Default)]
pub struct Version;

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(visitor::VersionVisitor)
    }
}

/// When a rpc call is made, the Server MUST reply with a Response,
/// except for in the case of Notifications.
///
/// visit [`here`](https://www.jsonrpc.org/specification) for details
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Response<S, R, D>
where
    S: AsRef<str>,
{
    /// This member is REQUIRED on error.
    /// This member MUST NOT exist if there was no error triggered during invocation.
    pub id: Option<usize>,
    /// A String specifying the version of the JSON-RPC protocol. MUST be exactly "2.0".
    pub jsonrpc: Option<S>,
    /// This member is REQUIRED on success.
    /// This member MUST NOT exist if there was an error invoking the method.
    /// The value of this member is determined by the method invoked on the Server.
    pub result: Option<R>,

    ///This member is REQUIRED on error.
    /// This member MUST NOT exist if there was no error triggered during invocation.
    pub error: Option<Error<S, D>>,
}

/// When a rpc call encounters an error,
/// the Response Object MUST contain the error member with a value that is a Object.
#[derive(Debug, Serialize, Deserialize)]
pub struct Error<S, D> {
    /// A Number that indicates the error type that occurred.
    pub code: ErrorCode,
    /// A String providing a short description of the error.
    /// The message SHOULD be limited to a concise single sentence
    pub message: S,
    ///A Primitive or Structured value that contains additional information about the error.
    /// This may be omitted.
    /// The value of this member is defined by the Server (e.g. detailed error information, nested errors etc.).
    ///
    pub data: Option<D>,
}

/// The error codes from and including -32768 to -32000 are reserved for pre-defined errors.
/// Any code within this range, but not defined explicitly below is reserved for future use.
/// The error codes are nearly the same as those suggested for XML-RPC at the following url:
/// <http://xmlrpc-epi.sourceforge.net/specs/rfc.fault_codes.php>
#[derive(thiserror::Error, Debug)]
pub enum ErrorCode {
    /// An error occurred on the server while parsing the JSON text.
    #[error("Invalid JSON was received by the server.")]
    ParseError,
    #[error("The JSON sent is not a valid Request object.")]
    InvalidRequest,
    #[error("The method does not exist / is not available.")]
    MethodNotFound,
    #[error("Invalid method parameter(s).")]
    InvalidParams,
    #[error("Internal JSON-RPC error.")]
    InternalError,
    /// Reserved for implementation-defined server-errors.
    #[error("Server error({0})")]
    ServerError(i64),
}

impl serde::Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::ParseError => serializer.serialize_i64(-32700),
            Self::InvalidRequest => serializer.serialize_i64(-32600),
            Self::MethodNotFound => serializer.serialize_i64(-32601),
            Self::InvalidParams => serializer.serialize_i64(-32602),
            Self::InternalError => serializer.serialize_i64(-32603),
            Self::ServerError(code) => serializer.serialize_i64(*code),
        }
    }
}

impl<'de> serde::Deserialize<'de> for ErrorCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = deserializer.deserialize_i64(visitor::ErrorCodeVisitor)?;

        match code {
            -32700 => Ok(ErrorCode::ParseError),
            -32600 => Ok(ErrorCode::InvalidRequest),
            -32601 => Ok(ErrorCode::MethodNotFound),
            -32602 => Ok(ErrorCode::InvalidParams),
            -32603 => Ok(ErrorCode::InternalError),
            _ => {
                // Check reserved implementation-defined server-errors range.
                if code > -32000 && code < -32099 {
                    Ok(ErrorCode::ServerError(code))
                } else {
                    Err(anyhow::format_err!("Invalid JSONRPC error code {}", code))
                        .map_err(serde::de::Error::custom)
                }
            }
        }
    }
}

mod visitor {
    use serde::de;
    use std::fmt;

    use crate::Version;

    pub struct ErrorCodeVisitor;

    impl<'de> de::Visitor<'de> for ErrorCodeVisitor {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("Version string MUST be exactly 2.0")
        }

        fn visit_i8<E>(self, value: i8) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(i64::from(value))
        }

        fn visit_i32<E>(self, value: i32) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(i64::from(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }
    }

    pub struct VersionVisitor;

    impl<'de> de::Visitor<'de> for VersionVisitor {
        type Value = Version;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer between -2^63 and 2^63")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v != "2.0" {
                return Err(anyhow::format_err!(
                    "Version string MUST be exactly 2.0, but got `{}`",
                    v
                ))
                .map_err(serde::de::Error::custom);
            }

            Ok(Version {})
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.as_str() != "2.0" {
                return Err(anyhow::format_err!(
                    "Version string MUST be exactly 2.0, but got `{}`",
                    v
                ))
                .map_err(serde::de::Error::custom);
            }

            Ok(Version {})
        }
    }
}

#[cfg(test)]
mod tests {

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use crate::Request;

    #[test]
    fn test_serialize() {
        _ = pretty_env_logger::try_init();
        #[derive(Default, Serialize, Deserialize)]
        struct Params<'a>(i32, &'a str);

        let request = Request {
            method: "hello",
            params: Params(10, "world"),
            ..Default::default()
        };

        let json = serde_json::to_string(&request).unwrap();

        assert_eq!(
            json!({ "id": null,"jsonrpc":"2.0", "method":"hello","params":[10, "world"]})
                .to_string(),
            json
        );
    }
}
