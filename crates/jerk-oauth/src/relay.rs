use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::config::Relay;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEnvelope {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn build_state(relay: &Relay, nonce_bytes: &[u8; 16]) -> String {
    let nonce = hex_encode(nonce_bytes);
    let envelope = match relay {
        Relay::Origin { origin } => StateEnvelope {
            source: "origin".into(),
            origin: Some(origin.clone()),
            port: None,
            nonce,
            path: Some("/auth/callback".into()),
        },
        Relay::Local { port } => StateEnvelope {
            source: "local".into(),
            origin: None,
            port: Some(*port),
            nonce,
            path: Some("/auth/callback".into()),
        },
    };
    use base64::engine::general_purpose::STANDARD;
    STANDARD.encode(serde_json::to_string(&envelope).unwrap_or_default())
}

pub fn nonce_from_state(state: &str) -> Result<String, RelayError> {
    use base64::engine::general_purpose::STANDARD;
    let bytes = STANDARD
        .decode(state)
        .map_err(|_| RelayError::InvalidState)?;
    let envelope: StateEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| RelayError::InvalidState)?;
    Ok(envelope.nonce)
}

pub fn verify_nonce(expected: &str, received: &str) -> Result<(), RelayError> {
    if !constant_time_eq(expected.as_bytes(), received.as_bytes()) {
        return Err(RelayError::NonceMismatch);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayError {
    InvalidState,
    NonceMismatch,
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::InvalidState => write!(f, "invalid relay state"),
            RelayError::NonceMismatch => write!(f, "relay nonce mismatch"),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize]);
        s.push(HEX[(b & 0xf) as usize]);
    }
    s
}

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

pub fn nonce_from_bytes(bytes: &[u8; 16]) -> String {
    hex_encode(bytes)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
