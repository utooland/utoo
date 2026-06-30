//! Sigstore keyless signing for provenance.
//!
//! Signs an in-toto Statement with an ephemeral ECDSA P-256 key, obtains a
//! short-lived signing certificate from Fulcio (bound to the CI OIDC identity),
//! records the signature in the Rekor transparency log, and assembles a
//! Sigstore **bundle v0.3** — the format npm-compatible registries consume.
//!
//! ## Verification status
//!
//! The deterministic building blocks (DSSE PAE, SPKI/PEM encoding, JWT subject
//! extraction, bundle field mapping) are unit-tested. The live Fulcio/Rekor
//! exchange and the registry's acceptance of the resulting bundle require a
//! real OIDC-capable CI run against the public-good Sigstore instances and an
//! attestation-aware registry, which cannot be exercised offline.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::util::http::client;

/// Public-good Fulcio signing-cert endpoint. Override with `SIGSTORE_FULCIO_URL`
/// to target staging (`fulcio.sigstage.dev`) or a private instance.
const DEFAULT_FULCIO_URL: &str = "https://fulcio.sigstore.dev/api/v2/signingCert";
/// Public-good Rekor log-entry endpoint. Override with `SIGSTORE_REKOR_URL`.
const DEFAULT_REKOR_URL: &str = "https://rekor.sigstore.dev/api/v1/log/entries";
const DSSE_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";
const BUNDLE_MEDIA_TYPE: &str = "application/vnd.dev.sigstore.bundle.v0.3+json";

/// Resolve a Sigstore endpoint from `env_key`, falling back to `default` when
/// the override is unset or empty.
fn sigstore_url(env_key: &str, default: &str) -> String {
    std::env::var(env_key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// DER SubjectPublicKeyInfo prefix for an uncompressed NIST P-256 public key.
/// The 65-byte point (`0x04 ‖ X ‖ Y`) is appended to form the full SPKI.
const P256_SPKI_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

/// Sign `statement` and return a serialized Sigstore bundle (v0.3) plus its
/// media type, ready to attach to the publish request.
pub async fn attest(statement: &Value, id_token: &str) -> Result<(String, String)> {
    let payload =
        serde_json::to_vec(statement).context("failed to serialize provenance statement")?;

    let key = EphemeralKey::new()?;

    // DSSE: sign the pre-authentication encoding of (payloadType, payload).
    let signature = key.sign(&pae(DSSE_PAYLOAD_TYPE, &payload))?;

    // Fulcio binds the ephemeral key to the OIDC identity; proof-of-possession
    // is a signature over the token's `sub` claim.
    let subject = jwt_subject(id_token)?;
    let proof = key.sign(subject.as_bytes())?;
    let cert_chain = fulcio_signing_cert(id_token, &key.public_key_pem(), &BASE64.encode(&proof))
        .await
        .context("Fulcio signing-certificate request failed")?;
    let leaf_pem = cert_chain
        .first()
        .context("Fulcio returned an empty certificate chain")?;
    let leaf_der = pem_to_der(leaf_pem).context("invalid Fulcio leaf certificate")?;

    let envelope = json!({
        "payload": BASE64.encode(&payload),
        "payloadType": DSSE_PAYLOAD_TYPE,
        "signatures": [{ "sig": BASE64.encode(&signature), "keyid": "" }],
    });

    let tlog_entry = rekor_create_entry(&envelope, leaf_pem)
        .await
        .context("Rekor transparency-log submission failed")?;

    let bundle = json!({
        "mediaType": BUNDLE_MEDIA_TYPE,
        "verificationMaterial": {
            "certificate": { "rawBytes": BASE64.encode(&leaf_der) },
            "tlogEntries": [tlog_entry],
        },
        "dsseEnvelope": envelope,
    });

    let serialized = serde_json::to_string(&bundle).context("failed to serialize bundle")?;
    Ok((serialized, BUNDLE_MEDIA_TYPE.to_string()))
}

/// An ephemeral ECDSA P-256 signing key, discarded after the publish.
struct EphemeralKey {
    key_pair: EcdsaKeyPair,
    rng: SystemRandom,
}

impl EphemeralKey {
    fn new() -> Result<Self> {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .map_err(|e| anyhow!("failed to generate ephemeral key: {e}"))?;
        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                .map_err(|e| anyhow!("failed to load ephemeral key: {e}"))?;
        Ok(Self { key_pair, rng })
    }

    /// Sign `message` (ASN.1 DER ECDSA signature over SHA-256).
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        self.key_pair
            .sign(&self.rng, message)
            .map(|sig| sig.as_ref().to_vec())
            .map_err(|e| anyhow!("signing failed: {e}"))
    }

    /// The public key as a PEM-encoded SPKI (what Fulcio expects).
    fn public_key_pem(&self) -> String {
        let point = self.key_pair.public_key().as_ref();
        let mut der = P256_SPKI_PREFIX.to_vec();
        der.extend_from_slice(point);
        pem_encode("PUBLIC KEY", &der)
    }
}

/// DSSE pre-authentication encoding:
/// `DSSEv1 SP len(type) SP type SP len(body) SP body`.
fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(payload.len() + payload_type.len() + 32);
    buf.extend_from_slice(b"DSSEv1 ");
    buf.extend_from_slice(payload_type.len().to_string().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(payload_type.as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(payload.len().to_string().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(payload);
    buf
}

/// Request a signing certificate from Fulcio, returning the PEM certificate
/// chain (leaf first).
async fn fulcio_signing_cert(
    id_token: &str,
    public_key_pem: &str,
    proof_b64: &str,
) -> Result<Vec<String>> {
    let body = json!({
        "credentials": { "oidcIdentityToken": id_token },
        "publicKeyRequest": {
            "publicKey": { "algorithm": "ECDSA", "content": public_key_pem },
            "proofOfPossession": proof_b64,
        },
    });

    let resp = client()?
        .post(sigstore_url("SIGSTORE_FULCIO_URL", DEFAULT_FULCIO_URL))
        .json(&body)
        .send()
        .await
        .context("Fulcio request failed")?;
    let status = resp.status();
    let value: Value = if status.is_success() {
        resp.json().await.context("invalid Fulcio response")?
    } else {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Fulcio returned HTTP {status}: {text}");
    };

    // Fulcio v2 returns the chain under either an embedded- or detached-SCT key.
    let chain = value
        .get("signedCertificateEmbeddedSct")
        .or_else(|| value.get("signedCertificateDetachedSct"))
        .and_then(|c| c.get("chain"))
        .and_then(|c| c.get("certificates"))
        .and_then(|c| c.as_array())
        .context("Fulcio response missing certificate chain")?;

    let certs: Vec<String> = chain
        .iter()
        .filter_map(|c| c.as_str().map(str::to_owned))
        .collect();
    if certs.is_empty() {
        anyhow::bail!("Fulcio certificate chain was empty");
    }
    Ok(certs)
}

/// Submit a `dsse` v0.0.1 entry to Rekor and map the response into the
/// bundle's `tlogEntry` shape (protobuf-JSON: bytes→base64, int64→string).
async fn rekor_create_entry(envelope: &Value, leaf_pem: &str) -> Result<Value> {
    let proposed = json!({
        "apiVersion": "0.0.1",
        "kind": "dsse",
        "spec": {
            "proposedContent": {
                "envelope": serde_json::to_string(envelope)?,
                "verifiers": [BASE64.encode(leaf_pem.as_bytes())],
            },
        },
    });

    let resp = client()?
        .post(sigstore_url("SIGSTORE_REKOR_URL", DEFAULT_REKOR_URL))
        .json(&proposed)
        .send()
        .await
        .context("Rekor request failed")?;
    let status = resp.status();
    let value: Value = if status.is_success() {
        resp.json().await.context("invalid Rekor response")?
    } else {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Rekor returned HTTP {status}: {text}");
    };

    // The response is `{ "<uuid>": <entry> }`.
    let entry = value
        .as_object()
        .and_then(|m| m.values().next())
        .context("Rekor response was empty")?;

    map_rekor_entry(entry)
}

/// Convert a Rekor REST log entry into the Sigstore bundle `tlogEntry` shape.
fn map_rekor_entry(entry: &Value) -> Result<Value> {
    let log_index = entry["logIndex"]
        .as_i64()
        .context("Rekor entry missing logIndex")?;
    let integrated_time = entry["integratedTime"]
        .as_i64()
        .context("Rekor entry missing integratedTime")?;
    let log_id_hex = entry["logID"]
        .as_str()
        .context("Rekor entry missing logID")?;
    let body = entry["body"].as_str().context("Rekor entry missing body")?;

    let verification = &entry["verification"];
    let set = verification["signedEntryTimestamp"]
        .as_str()
        .context("Rekor entry missing signedEntryTimestamp")?;

    let mut tlog = json!({
        "logIndex": log_index.to_string(),
        "logId": { "keyId": BASE64.encode(hex_decode(log_id_hex)?) },
        "kindVersion": { "kind": "dsse", "version": "0.0.1" },
        "integratedTime": integrated_time.to_string(),
        "inclusionPromise": { "signedEntryTimestamp": set },
        "canonicalizedBody": body,
    });

    if let Some(proof) = verification.get("inclusionProof").filter(|p| p.is_object()) {
        tlog["inclusionProof"] = map_inclusion_proof(proof)?;
    }

    Ok(tlog)
}

/// Convert a Rekor inclusion proof (hex hashes) into the bundle shape
/// (base64 bytes, stringified counters).
fn map_inclusion_proof(proof: &Value) -> Result<Value> {
    let log_index = proof["logIndex"]
        .as_i64()
        .context("proof missing logIndex")?;
    let tree_size = proof["treeSize"]
        .as_i64()
        .context("proof missing treeSize")?;
    let root_hash = proof["rootHash"]
        .as_str()
        .context("proof missing rootHash")?;
    let hashes = proof["hashes"]
        .as_array()
        .context("proof missing hashes")?
        .iter()
        .filter_map(|h| h.as_str())
        .map(|h| Ok(BASE64.encode(hex_decode(h)?)))
        .collect::<Result<Vec<String>>>()?;

    let mut out = json!({
        "logIndex": log_index.to_string(),
        "rootHash": BASE64.encode(hex_decode(root_hash)?),
        "treeSize": tree_size.to_string(),
        "hashes": hashes,
    });
    if let Some(cp) = proof["checkpoint"].as_str() {
        out["checkpoint"] = json!({ "envelope": cp });
    }
    Ok(out)
}

/// Extract the `sub` (subject) claim from a JWT without verifying it — only the
/// payload segment is needed to bind the Fulcio proof-of-possession.
fn jwt_subject(token: &str) -> Result<String> {
    let payload_b64 = token.split('.').nth(1).context("malformed OIDC token")?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .context("OIDC token payload is not valid base64url")?;
    let claims: Value =
        serde_json::from_slice(&payload).context("OIDC token payload is not JSON")?;
    claims["sub"]
        .as_str()
        .map(str::to_owned)
        .context("OIDC token missing `sub` claim")
}

/// PEM-encode DER bytes under `label` with 64-char lines.
fn pem_encode(label: &str, der: &[u8]) -> String {
    let b64 = BASE64.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// Decode the DER body of a single PEM block (ignoring the armor lines).
fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let b64: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    BASE64.decode(b64).context("PEM body is not valid base64")
}

/// Decode a lowercase/uppercase hex string into bytes.
fn hex_decode(s: &str) -> Result<Vec<u8>> {
    hex::decode(s).with_context(|| format!("invalid hex value: {s}"))
}

/// Lowercase hex SHA-256 of `data` (used elsewhere for digests).
#[allow(dead_code)]
fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pae_matches_dsse_spec() {
        // DSSEv1 SP 4 SP type SP 5 SP hello
        let out = pae("type", b"hello");
        assert_eq!(out, b"DSSEv1 4 type 5 hello");
    }

    #[test]
    fn pae_uses_byte_lengths() {
        // `application/vnd.in-toto+json` is 28 bytes.
        let out = pae("application/vnd.in-toto+json", b"{}");
        assert_eq!(out, b"DSSEv1 28 application/vnd.in-toto+json 2 {}");
    }

    #[test]
    fn pem_roundtrips_to_der() {
        let der = [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03];
        let pem = pem_encode("PUBLIC KEY", &der);
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(pem.trim_end().ends_with("-----END PUBLIC KEY-----"));
        assert_eq!(pem_to_der(&pem).unwrap(), der);
    }

    #[test]
    fn spki_prefix_plus_point_is_well_formed() {
        // 26-byte prefix + 65-byte uncompressed point = 91-byte SPKI; the
        // DER SEQUENCE length byte (0x59 = 89) covers the remaining 89 bytes.
        let point = [0x04u8; 65];
        let mut der = P256_SPKI_PREFIX.to_vec();
        der.extend_from_slice(&point);
        assert_eq!(der.len(), 91);
        assert_eq!(der[0], 0x30);
        assert_eq!(der[1], 0x59);
    }

    #[test]
    fn jwt_subject_extracts_sub() {
        // header.payload.signature with payload {"sub":"repo:o/r:ref:..."}
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"repo:o/r","aud":"sigstore"}"#);
        let token = format!("h.{payload}.s");
        assert_eq!(jwt_subject(&token).unwrap(), "repo:o/r");
    }

    #[test]
    fn ephemeral_key_signs_and_exports_pem() {
        let key = EphemeralKey::new().unwrap();
        let sig = key.sign(b"message").unwrap();
        assert!(!sig.is_empty());
        let pem = key.public_key_pem();
        let der = pem_to_der(&pem).unwrap();
        // 26-byte SPKI prefix + 65-byte P-256 point.
        assert_eq!(der.len(), 91);
        assert!(der.starts_with(&P256_SPKI_PREFIX));
    }

    #[test]
    fn map_inclusion_proof_encodes_bytes_and_strings() {
        let proof = json!({
            "logIndex": 42,
            "treeSize": 100,
            "rootHash": "00ff",
            "hashes": ["aa", "bb"],
            "checkpoint": "ckpt-body",
        });
        let out = map_inclusion_proof(&proof).unwrap();
        assert_eq!(out["logIndex"], "42");
        assert_eq!(out["treeSize"], "100");
        assert_eq!(out["rootHash"], BASE64.encode([0x00, 0xff]));
        assert_eq!(out["hashes"][0], BASE64.encode([0xaa]));
        assert_eq!(out["checkpoint"]["envelope"], "ckpt-body");
    }

    #[test]
    fn map_rekor_entry_builds_tlog_shape() {
        let entry = json!({
            "logIndex": 7,
            "integratedTime": 1700000000_i64,
            "logID": "0a0b",
            "body": "Y2Fub25pY2Fs",
            "verification": { "signedEntryTimestamp": "c2V0" },
        });
        let tlog = map_rekor_entry(&entry).unwrap();
        assert_eq!(tlog["logIndex"], "7");
        assert_eq!(tlog["integratedTime"], "1700000000");
        assert_eq!(tlog["kindVersion"]["kind"], "dsse");
        assert_eq!(tlog["logId"]["keyId"], BASE64.encode([0x0a, 0x0b]));
        assert_eq!(tlog["inclusionPromise"]["signedEntryTimestamp"], "c2V0");
        assert_eq!(tlog["canonicalizedBody"], "Y2Fub25pY2Fs");
    }
}
