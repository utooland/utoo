use base64::Engine;
use sha2::Digest;

/// Compute an SRI integrity string (`sha512-<base64>`) for the given data.
///
/// This is the same format used by npm registries and package-lock.json.
pub fn compute_integrity(data: &[u8]) -> String {
    let hash = sha2::Sha512::digest(data);
    let b64 = base64::engine::general_purpose::STANDARD.encode(hash);
    format!("sha512-{b64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_integrity_format() {
        let integrity = compute_integrity(b"hello");
        assert!(integrity.starts_with("sha512-"));
        // SHA-512 produces 64 bytes → 88 chars in base64 (with padding)
        let b64_part = integrity.strip_prefix("sha512-").unwrap();
        assert_eq!(b64_part.len(), 88);
    }

    #[test]
    fn test_compute_integrity_deterministic() {
        let a = compute_integrity(b"test data");
        let b = compute_integrity(b"test data");
        assert_eq!(a, b);
    }

    #[test]
    fn test_compute_integrity_different_input() {
        let a = compute_integrity(b"aaa");
        let b = compute_integrity(b"bbb");
        assert_ne!(a, b);
    }
}
