use deno_core::{op2, OpState, Resource, ResourceId};
use deno_error::JsErrorBox;
use ring::aead;
use ring::digest;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::borrow::Cow;
use std::cell::RefCell;

// --- Hash ---

struct HashResource {
    ctx: RefCell<Option<digest::Context>>,
}

impl Resource for HashResource {
    fn name(&self) -> Cow<'_, str> {
        "cryptoHash".into()
    }
}

fn algo_from_name(name: &str) -> Result<&'static digest::Algorithm, JsErrorBox> {
    match name {
        "sha1" | "SHA1" | "SHA-1" => Ok(&digest::SHA1_FOR_LEGACY_USE_ONLY),
        "sha256" | "SHA256" | "SHA-256" => Ok(&digest::SHA256),
        "sha384" | "SHA384" | "SHA-384" => Ok(&digest::SHA384),
        "sha512" | "SHA512" | "SHA-512" => Ok(&digest::SHA512),
        _ => Err(JsErrorBox::generic(format!(
            "Unsupported hash algorithm: {}",
            name
        ))),
    }
}

#[op2(fast)]
#[smi]
pub fn op_crypto_hash_create(
    state: &mut OpState,
    #[string] algorithm: String,
) -> Result<ResourceId, JsErrorBox> {
    let algo = algo_from_name(&algorithm)?;
    let ctx = digest::Context::new(algo);
    Ok(state.resource_table.add(HashResource {
        ctx: RefCell::new(Some(ctx)),
    }))
}

#[op2(fast)]
pub fn op_crypto_hash_update(
    state: &mut OpState,
    #[smi] rid: ResourceId,
    #[buffer] data: &[u8],
) -> Result<(), JsErrorBox> {
    let resource = state
        .resource_table
        .get::<HashResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let mut ctx_ref = resource.ctx.borrow_mut();
    if let Some(ctx) = ctx_ref.as_mut() {
        ctx.update(data);
    }
    Ok(())
}

#[op2]
#[serde]
pub fn op_crypto_hash_digest(
    state: &mut OpState,
    #[smi] rid: ResourceId,
) -> Result<Vec<u8>, JsErrorBox> {
    let resource = state
        .resource_table
        .take::<HashResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let ctx = resource
        .ctx
        .borrow_mut()
        .take()
        .ok_or_else(|| JsErrorBox::generic("Hash context already consumed"))?;
    let result = ctx.finish();
    Ok(result.as_ref().to_vec())
}

// --- HMAC ---

struct HmacResource {
    ctx: RefCell<Option<hmac::Context>>,
}

impl Resource for HmacResource {
    fn name(&self) -> Cow<'_, str> {
        "cryptoHmac".into()
    }
}

fn hmac_algo_from_name(name: &str) -> Result<hmac::Algorithm, JsErrorBox> {
    match name {
        "sha1" | "SHA1" | "SHA-1" => Ok(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
        "sha256" | "SHA256" | "SHA-256" => Ok(hmac::HMAC_SHA256),
        "sha384" | "SHA384" | "SHA-384" => Ok(hmac::HMAC_SHA384),
        "sha512" | "SHA512" | "SHA-512" => Ok(hmac::HMAC_SHA512),
        _ => Err(JsErrorBox::generic(format!(
            "Unsupported HMAC algorithm: {}",
            name
        ))),
    }
}

#[op2(fast)]
#[smi]
pub fn op_crypto_hmac_create(
    state: &mut OpState,
    #[string] algorithm: String,
    #[buffer] key: &[u8],
) -> Result<ResourceId, JsErrorBox> {
    let algo = hmac_algo_from_name(&algorithm)?;
    let hmac_key = hmac::Key::new(algo, key);
    let ctx = hmac::Context::with_key(&hmac_key);
    Ok(state.resource_table.add(HmacResource {
        ctx: RefCell::new(Some(ctx)),
    }))
}

#[op2(fast)]
pub fn op_crypto_hmac_update(
    state: &mut OpState,
    #[smi] rid: ResourceId,
    #[buffer] data: &[u8],
) -> Result<(), JsErrorBox> {
    let resource = state
        .resource_table
        .get::<HmacResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let mut ctx_ref = resource.ctx.borrow_mut();
    if let Some(ctx) = ctx_ref.as_mut() {
        ctx.update(data);
    }
    Ok(())
}

#[op2]
#[serde]
pub fn op_crypto_hmac_digest(
    state: &mut OpState,
    #[smi] rid: ResourceId,
) -> Result<Vec<u8>, JsErrorBox> {
    let resource = state
        .resource_table
        .take::<HmacResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let ctx = resource
        .ctx
        .borrow_mut()
        .take()
        .ok_or_else(|| JsErrorBox::generic("HMAC context already consumed"))?;
    let tag = ctx.sign();
    Ok(tag.as_ref().to_vec())
}

// --- Random ---

#[op2]
#[serde]
pub fn op_crypto_random_bytes(#[smi] size: u32) -> Result<Vec<u8>, JsErrorBox> {
    let rng = SystemRandom::new();
    let mut buf = vec![0u8; size as usize];
    rng.fill(&mut buf)
        .map_err(|_| JsErrorBox::generic("Failed to generate random bytes"))?;
    Ok(buf)
}

// --- AES-GCM ---

#[op2]
#[serde]
pub fn op_crypto_aes_gcm_encrypt(
    #[buffer] key: &[u8],
    #[buffer] iv: &[u8],
    #[buffer] data: &[u8],
    #[buffer] additional_data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    let algorithm = match key.len() {
        16 => &aead::AES_128_GCM,
        32 => &aead::AES_256_GCM,
        _ => return Err(JsErrorBox::type_error("AES-GCM key must be 128 or 256 bits")),
    };
    let unbound_key = aead::UnboundKey::new(algorithm, key)
        .map_err(|_| JsErrorBox::generic("Failed to create AES-GCM key"))?;
    let key = aead::LessSafeKey::new(unbound_key);
    let nonce = aead::Nonce::try_assume_unique_for_key(iv)
        .map_err(|_| JsErrorBox::type_error("AES-GCM IV must be 12 bytes"))?;
    let mut in_out = data.to_vec();
    key.seal_in_place_append_tag(nonce, aead::Aad::from(additional_data), &mut in_out)
        .map_err(|_| JsErrorBox::generic("AES-GCM encryption failed"))?;
    Ok(in_out)
}

#[op2]
#[serde]
pub fn op_crypto_aes_gcm_decrypt(
    #[buffer] key: &[u8],
    #[buffer] iv: &[u8],
    #[buffer] data: &[u8],
    #[buffer] additional_data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    let algorithm = match key.len() {
        16 => &aead::AES_128_GCM,
        32 => &aead::AES_256_GCM,
        _ => return Err(JsErrorBox::type_error("AES-GCM key must be 128 or 256 bits")),
    };
    let unbound_key = aead::UnboundKey::new(algorithm, key)
        .map_err(|_| JsErrorBox::generic("Failed to create AES-GCM key"))?;
    let key = aead::LessSafeKey::new(unbound_key);
    let nonce = aead::Nonce::try_assume_unique_for_key(iv)
        .map_err(|_| JsErrorBox::type_error("AES-GCM IV must be 12 bytes"))?;
    let mut in_out = data.to_vec();
    let plaintext = key.open_in_place(nonce, aead::Aad::from(additional_data), &mut in_out)
        .map_err(|_| JsErrorBox::generic("AES-GCM decryption failed"))?;
    Ok(plaintext.to_vec())
}
