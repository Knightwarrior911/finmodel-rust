//! Pure UUID-v4 formatting. The reducer never generates identifiers; the driver
//! supplies 16 random bytes and formats them here so id shape stays consistent
//! with the web `crypto.randomUUID()` used elsewhere.

/// Format 16 bytes as a canonical RFC-4122 v4 UUID string (`8-4-4-4-12` hex).
/// Version and variant bits are forced so the output is always a well-formed v4.
pub fn format_uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx
    let h = |b: u8| -> [u8; 2] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        [HEX[(b >> 4) as usize], HEX[(b & 0x0f) as usize]]
    };
    let mut out = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let [hi, lo] = h(*b);
        out.push(hi as char);
        out.push(lo as char);
    }
    out
}

/// Whether `s` is a UUID-shaped id (`8-4-4-4-12` lowercase/uppercase hex).
pub fn is_uuid_shaped(s: &str) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != groups.len() {
        return false;
    }
    parts
        .iter()
        .zip(groups)
        .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_v4_shape_and_bits() {
        let id = format_uuid_v4([0xff; 16]);
        assert!(is_uuid_shaped(&id), "{id}");
        // version nibble
        assert_eq!(id.as_bytes()[14], b'4');
        // variant nibble in {8,9,a,b}
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    }

    #[test]
    fn rejects_malformed() {
        assert!(!is_uuid_shaped("not-a-uuid"));
        assert!(!is_uuid_shaped("12345678-1234-1234-1234-1234567890"));
        assert!(!is_uuid_shaped(""));
    }

    #[test]
    fn distinct_bytes_distinct_ids() {
        let a = format_uuid_v4([1; 16]);
        let b = format_uuid_v4([2; 16]);
        assert_ne!(a, b);
    }
}
