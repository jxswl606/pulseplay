//! Key helpers. Keys use the standard base64 encoding of 32 bytes.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use boringtun::x25519::{PublicKey, StaticSecret};

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).expect("system random source unavailable");
    bytes
}

/// Generates a new keypair, returned as (private, public) base64 strings.
pub fn generate() -> (String, String) {
    let secret = StaticSecret::from(random_bytes::<32>());
    let public = PublicKey::from(&secret);
    (STANDARD.encode(secret.to_bytes()), STANDARD.encode(public.as_bytes()))
}

fn decode32(text: &str) -> Result<[u8; 32], String> {
    let bytes = STANDARD
        .decode(text.trim())
        .map_err(|_| "key is not valid base64".to_string())?;
    bytes.try_into().map_err(|_| "key must be 32 bytes".to_string())
}

pub fn parse_secret(text: &str) -> Result<StaticSecret, String> {
    decode32(text).map(StaticSecret::from)
}

pub fn parse_public(text: &str) -> Result<PublicKey, String> {
    decode32(text).map(PublicKey::from)
}

pub fn public_of(secret: &str) -> Result<String, String> {
    let secret = parse_secret(secret)?;
    Ok(STANDARD.encode(PublicKey::from(&secret).as_bytes()))
}

/// Short form of a key for logs and pages, such as `kq7M3x…`.
pub fn short(key: &str) -> String {
    format!("{}…", key.chars().take(6).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_round_trip() {
        let (private, public) = generate();
        assert_eq!(public_of(&private).unwrap(), public);
        assert!(parse_public(&public).is_ok());
        assert!(parse_secret("not a key").is_err());
        assert!(parse_secret("AAAA").is_err());
    }
}
