use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

pub fn verifier(random_bytes: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(random_bytes)
}

pub fn challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}
