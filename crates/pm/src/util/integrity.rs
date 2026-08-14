use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use base64::Engine;
// sha1 and sha2 re-export the same `digest::Digest` trait; one anonymous
// import covers both hashers.
use sha2::Digest as _;

/// Compute an SRI integrity string (`sha512-<base64>`) for the given data.
///
/// This is the same format used by npm registries and package-lock.json.
pub fn compute_integrity(data: &[u8]) -> String {
    let hash = sha2::Sha512::digest(data);
    let b64 = base64::engine::general_purpose::STANDARD.encode(hash);
    format!("sha512-{b64}")
}

/// Compute a SHA-1 hex digest (the `shasum` field required by npm registries).
pub fn compute_shasum(data: &[u8]) -> String {
    let hash = sha1::Sha1::digest(data);
    hash.iter().fold(String::with_capacity(40), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Verify npm Subresource Integrity metadata against downloaded bytes.
///
/// Registries may provide several whitespace-separated digests. A match from
/// any supported algorithm is sufficient, following SRI semantics.
pub fn verify_integrity(data: &[u8], integrity: &str) -> Result<()> {
    let mut supported = false;
    for token in integrity.split_ascii_whitespace() {
        let Some((algorithm, encoded)) = token.split_once('-') else {
            continue;
        };
        let actual = match algorithm {
            "sha1" => sha1::Sha1::digest(data).to_vec(),
            "sha256" => sha2::Sha256::digest(data).to_vec(),
            "sha384" => sha2::Sha384::digest(data).to_vec(),
            "sha512" => sha2::Sha512::digest(data).to_vec(),
            _ => continue,
        };
        supported = true;
        let expected = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .with_context(|| format!("Invalid {algorithm} integrity digest"))?;
        if actual == expected {
            return Ok(());
        }
    }

    if !supported {
        bail!("No supported checksum in integrity metadata: {integrity}");
    }
    bail!("Downloaded archive failed integrity verification")
}

/// Verify npm's legacy hexadecimal SHA-1 `dist.shasum` field.
pub fn verify_shasum(data: &[u8], shasum: &str) -> Result<()> {
    if compute_shasum(data).eq_ignore_ascii_case(shasum) {
        Ok(())
    } else {
        bail!("Downloaded archive failed shasum verification")
    }
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

    #[test]
    fn test_compute_shasum_format() {
        let shasum = compute_shasum(b"hello");
        // SHA-1 produces 20 bytes → 40 hex chars
        assert_eq!(shasum.len(), 40);
        assert!(shasum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_compute_shasum_known_value() {
        // SHA-1("hello") = aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d
        assert_eq!(
            compute_shasum(b"hello"),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
    }

    #[test]
    fn test_compute_shasum_deterministic() {
        let a = compute_shasum(b"test data");
        let b = compute_shasum(b"test data");
        assert_eq!(a, b);
    }

    #[test]
    fn verifies_registry_sri_and_rejects_corruption() {
        let data = b"release archive";
        let sri = compute_integrity(data);

        assert!(verify_integrity(data, &sri).is_ok());
        assert!(verify_integrity(b"corrupted", &sri).is_err());
        assert!(verify_integrity(data, "md5-unsupported").is_err());
    }
}
