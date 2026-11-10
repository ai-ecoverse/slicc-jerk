use serde_json::Value;
#[cfg(target_os = "wasi")]
use std::io::{Read, Write};

pub trait Sealed {
    fn request(&self, body: &Value) -> Result<Value, SealedError>;
    fn poll(&self, id: &str) -> Result<PollResult, SealedError>;
    fn cancel(&self, id: &str) -> Result<(), SealedError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedError {
    pub message: String,
    pub code: &'static str,
}

impl SealedError {
    pub fn from_io(error: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let code = match error.kind() {
            ErrorKind::InvalidInput => "EINVAL",
            ErrorKind::Unsupported => "ENOTSUP",
            ErrorKind::TimedOut => "ETIMEDOUT",
            ErrorKind::PermissionDenied => "EACCES",
            ErrorKind::WouldBlock => "EAGAIN",
            ErrorKind::NotFound
            | ErrorKind::NotConnected
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe => "ENOTCONN",
            _ => "EIO",
        };
        Self {
            message: "sealed transport failed".into(),
            code,
        }
    }
}

impl std::fmt::Display for SealedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PollResult {
    Pending { progress: Vec<String> },
    Done(Value),
}

#[cfg(target_os = "wasi")]
pub struct DevSealed;

#[cfg(target_os = "wasi")]
impl DevSealed {
    fn roundtrip(body: &Value) -> Result<Value, SealedError> {
        let encoded =
            zeroize::Zeroizing::new(serde_json::to_vec(body).map_err(|_| SealedError {
                message: "cannot encode sealed request".into(),
                code: "EIO",
            })?);
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/sealed")
            .map_err(SealedError::from_io)?;
        file.write_all(&encoded).map_err(SealedError::from_io)?;
        let mut buf = zeroize::Zeroizing::new(Vec::new());
        file.read_to_end(&mut buf).map_err(SealedError::from_io)?;
        serde_json::from_slice(&buf).map_err(|_| SealedError {
            message: "invalid JSON from /dev/sealed".into(),
            code: "EIO",
        })
    }
}

#[cfg(target_os = "wasi")]
impl Sealed for DevSealed {
    fn request(&self, body: &Value) -> Result<Value, SealedError> {
        Self::roundtrip(body)
    }

    fn poll(&self, id: &str) -> Result<PollResult, SealedError> {
        let body = serde_json::json!({ "v": 1, "op": "status", "id": id });
        let reply = Self::roundtrip(&body)?;
        match reply.get("state").and_then(Value::as_str) {
            Some("done") => Ok(PollResult::Done(reply)),
            Some("pending") => {
                let progress = reply
                    .get("progress")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(PollResult::Pending { progress })
            }
            other => Err(SealedError {
                code: "EIO",
                message: format!(
                    "unexpected state from /dev/sealed: {}",
                    other.unwrap_or("(missing)")
                ),
            }),
        }
    }

    fn cancel(&self, id: &str) -> Result<(), SealedError> {
        let body = serde_json::json!({ "v": 1, "op": "cancel", "id": id });
        let _ = Self::roundtrip(&body)?;
        Ok(())
    }
}

pub fn is_placeholder(value: &str) -> bool {
    value
        .strip_prefix("fnox_proxy_")
        .is_some_and(|suffix| suffix.len() == 24 && suffix.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn seal_value(sealed: &dyn Sealed, name: &str, value: &str) -> Result<String, SealedError> {
    if is_placeholder(value) {
        return Ok(value.to_string());
    }
    let reply = sealed.request(&serde_json::json!({
        "v": 1, "op": "seal", "name": name, "value": value,
    }))?;
    if reply.get("error").is_some() {
        let code = reply["error"]["code"]
            .as_str()
            .filter(|code| code.bytes().all(|b| b.is_ascii_uppercase()))
            .unwrap_or("EIO");
        return Err(SealedError {
            code: "EIO",
            message: format!("seven refused sealing: {code}"),
        });
    }
    reply
        .get("placeholder")
        .and_then(Value::as_str)
        .filter(|v| is_placeholder(v))
        .map(String::from)
        .ok_or_else(|| SealedError {
            code: "EIO",
            message: "sealing returned no placeholder".into(),
        })
}

pub struct FakeSealed {
    handler: Box<dyn Fn(&Value) -> Value + Send + Sync>,
}

impl FakeSealed {
    pub fn new(handler: impl Fn(&Value) -> Value + Send + Sync + 'static) -> Self {
        Self {
            handler: Box::new(handler),
        }
    }
}

impl Sealed for FakeSealed {
    fn request(&self, body: &Value) -> Result<Value, SealedError> {
        Ok((self.handler)(body))
    }

    fn poll(&self, id: &str) -> Result<PollResult, SealedError> {
        let body = serde_json::json!({ "v": 1, "op": "status", "id": id });
        let reply = (self.handler)(&body);
        match reply.get("state").and_then(Value::as_str) {
            Some("done") => Ok(PollResult::Done(reply)),
            Some("pending") => {
                let progress = reply
                    .get("progress")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(PollResult::Pending { progress })
            }
            _ => Err(SealedError {
                code: "EIO",
                message: "unexpected state".into(),
            }),
        }
    }

    fn cancel(&self, id: &str) -> Result<(), SealedError> {
        let body = serde_json::json!({ "v": 1, "op": "cancel", "id": id });
        let _ = (self.handler)(&body);
        Ok(())
    }
}

pub struct Waiting<'a> {
    pub inner: &'a dyn Sealed,
    pub sleep: &'a dyn Fn(u64),
    pub limit: u32,
}

impl Waiting<'_> {
    pub fn request(&self, body: &Value) -> Result<Value, SealedError> {
        let reply = self.inner.request(body)?;
        if reply.get("state").and_then(Value::as_str) != Some("pending") {
            return Ok(reply);
        }
        let id = reply
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        for _ in 0..self.limit {
            (self.sleep)(1);
            match self.inner.poll(&id) {
                Ok(PollResult::Done(done)) => return Ok(done),
                Ok(PollResult::Pending { .. }) => {}
                Err(e) => {
                    let _ = self.inner.cancel(&id);
                    return Err(e);
                }
            }
        }
        let _ = self.inner.cancel(&id);
        Err(SealedError {
            code: "ETIMEDOUT",
            message: "seven did not answer in time".to_string(),
        })
    }
}

impl Sealed for Waiting<'_> {
    fn request(&self, body: &Value) -> Result<Value, SealedError> {
        Waiting::request(self, body)
    }

    fn poll(&self, id: &str) -> Result<PollResult, SealedError> {
        self.inner.poll(id)
    }

    fn cancel(&self, id: &str) -> Result<(), SealedError> {
        self.inner.cancel(id)
    }
}
