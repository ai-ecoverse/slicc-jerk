use serde_json::Value;

pub trait Sealed: Send + Sync {
    fn request(&self, body: &Value) -> Result<Value, SealedError>;
    fn poll(&self, id: &str) -> Result<PollResult, SealedError>;
    fn cancel(&self, id: &str) -> Result<(), SealedError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedError {
    pub message: String,
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
        let encoded = serde_json::to_vec(body).map_err(|e| SealedError {
            message: format!("cannot encode request: {e}"),
        })?;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/sealed")
            .map_err(|e| SealedError {
                message: format!("cannot open /dev/sealed: {e}"),
            })?;
        file.write_all(&encoded).map_err(|e| SealedError {
            message: format!("write to /dev/sealed failed: {e}"),
        })?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| SealedError {
            message: format!("read from /dev/sealed failed: {e}"),
        })?;
        serde_json::from_slice(&buf).map_err(|e| SealedError {
            message: format!("invalid JSON from /dev/sealed: {e}"),
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
