//! UUIDv7 ids and their wire form: 26-char lowercase Crockford base32 (doc 05 §2).

use crate::Id;

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

const fn decode_table() -> [u8; 256] {
    let mut t = [0xff; 256];
    let mut i = 0;
    while i < 32 {
        t[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    t
}
static DECODE: [u8; 256] = decode_table();

pub fn new_id() -> Id {
    uuid::Uuid::now_v7().into_bytes()
}

/// 128 bits as 26 chars: the first char carries the top 3 bits, the rest 5 bits each.
pub fn encode(id: &Id) -> [u8; 26] {
    let mut n = u128::from_be_bytes(*id);
    let mut out = [0u8; 26];
    for c in out.iter_mut().rev() {
        *c = ALPHABET[(n & 31) as usize];
        n >>= 5;
    }
    out
}

pub fn encode_string(id: &Id) -> String {
    // ALPHABET is ASCII, so the bytes are valid UTF-8.
    encode(id).iter().map(|&b| b as char).collect()
}

/// Strict: lowercase only, exactly 26 chars, first char at most `7`.
pub fn decode(s: &str) -> Option<Id> {
    let b = s.as_bytes();
    if b.len() != 26 || b[0] > b'7' {
        return None;
    }
    let mut n = 0u128;
    for &c in b {
        let v = DECODE[c as usize];
        if v == 0xff {
            return None;
        }
        n = (n << 5) | v as u128;
    }
    Some(n.to_be_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_order() {
        let a = new_id();
        let b = new_id();
        assert_eq!(decode(&encode_string(&a)), Some(a));
        // Time-ordered ids keep their order in the wire form.
        assert!(encode(&a) < encode(&b));
        assert_eq!(encode_string(&[0; 16]), "0".repeat(26));
        assert_eq!(decode(&encode_string(&[0xff; 16])), Some([0xff; 16]));
    }

    #[test]
    fn rejects_bad_input() {
        let s = encode_string(&new_id());
        assert_eq!(decode(&s[..25]), None);
        assert_eq!(decode(&s.to_uppercase()), None);
        assert_eq!(decode(&format!("8{}", &s[1..])), None);
        assert_eq!(decode(&format!("{}u", &s[..25])), None); // 'u' is not in Crockford
    }
}
