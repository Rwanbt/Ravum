//! SHA-256 identity helpers shared by snapshots, contexts and cache keys.

use sha2::{Digest, Sha256};

/// Returns the lowercase hexadecimal SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

/// Feeds a length-prefixed field into `hasher`, so concatenations of different
/// field sequences can never collide by moving bytes across boundaries.
pub fn push_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update((field.len() as u64).to_le_bytes());
    hasher.update(field);
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc") per FIPS 180-4 test vector.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn length_prefixing_separates_ambiguous_fields() {
        let mut a = Sha256::new();
        push_field(&mut a, b"ab");
        push_field(&mut a, b"c");
        let mut b = Sha256::new();
        push_field(&mut b, b"a");
        push_field(&mut b, b"bc");
        assert_ne!(hex(&a.finalize()), hex(&b.finalize()));
    }
}
