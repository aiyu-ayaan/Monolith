//! Password hashing and signed tokens (doc 08).
//!
//! Phase 1 covers superusers: Argon2id password hashes and HMAC-SHA256 access tokens
//! verified without a DB lookup. Per-app EdDSA user tokens arrive in Phase 2.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("random source failed")]
    Random,
    #[error("password hashing failed: {0}")]
    Hash(String),
}

/// Fills a fresh random buffer from the OS.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], AuthError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| AuthError::Random)?;
    Ok(b)
}

// OWASP baseline from doc 08 §3: m=19 MiB, t=2, p=1.
fn argon() -> Argon2<'static> {
    let params = Params::new(19 * 1024, 2, 1, None).expect("static Argon2 params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// CPU-heavy (tens of ms): call from a blocking pool, never a Tokio worker.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()?).map_err(|e| AuthError::Hash(e.to_string()))?;
    argon()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Hash(e.to_string()))
}

/// CPU-heavy: call from a blocking pool. A malformed stored hash verifies as false.
pub fn verify_password(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|h| argon().verify_password(password.as_bytes(), &h).is_ok())
}

/// Claims of a superuser token: `v1.<sub hex>.<exp µs>.<hmac hex>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claims {
    pub sub: [u8; 16],
    pub exp_us: i64,
}

fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    b.iter().flat_map(|x| [H[(x >> 4) as usize] as char, H[(x & 15) as usize] as char]).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let b = s.as_bytes();
    if b.len() != N * 2 {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = nib(b[2 * i])? << 4 | nib(b[2 * i + 1])?;
    }
    Some(out)
}

fn mac(key: &[u8], body: &str) -> Hmac<Sha256> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(body.as_bytes());
    m
}

pub fn sign_token(key: &[u8], c: Claims) -> String {
    let body = format!("v1.{}.{}", hex(&c.sub), c.exp_us);
    let tag = mac(key, &body).finalize().into_bytes();
    format!("{body}.{}", hex(&tag))
}

/// Constant-time signature check, then expiry against `now_us`.
pub fn verify_token(key: &[u8], token: &str, now_us: i64) -> Option<Claims> {
    let (body, tag) = token.rsplit_once('.')?;
    let tag = unhex::<32>(tag)?;
    mac(key, body).verify_slice(&tag).ok()?;
    let mut parts = body.split('.');
    if parts.next()? != "v1" {
        return None;
    }
    let sub = unhex::<16>(parts.next()?)?;
    let exp_us: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || exp_us <= now_us {
        return None;
    }
    Some(Claims { sub, exp_us })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not a hash"));
    }

    #[test]
    fn token_round_trip_and_tamper() {
        let key = [9u8; 32];
        let c = Claims { sub: [1; 16], exp_us: 1_000 };
        let t = sign_token(&key, c);
        assert_eq!(verify_token(&key, &t, 999), Some(c));
        assert_eq!(verify_token(&key, &t, 1_000), None, "expired");
        assert_eq!(verify_token(&[8u8; 32], &t, 0), None, "other key");
        let forged = t.replace(".1000.", ".9999.");
        assert_eq!(verify_token(&key, &forged, 0), None, "edited claims");
        assert_eq!(verify_token(&key, "garbage", 0), None);
    }
}
