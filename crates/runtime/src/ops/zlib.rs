use deno_core::{op2, JsBuffer};
use deno_error::JsErrorBox;
use flate2::read::{DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder};
use flate2::Compression;
use std::io::Read;

fn compression_level(level: i32) -> Compression {
    if level < 0 {
        Compression::default()
    } else {
        Compression::new(level.min(9) as u32)
    }
}

#[op2]
#[serde]
pub fn op_zlib_gzip(
    #[buffer] data: JsBuffer,
    #[smi] level: i32,
) -> Result<Vec<u8>, JsErrorBox> {
    let mut encoder = GzEncoder::new(data.as_ref(), compression_level(level));
    let mut out = Vec::new();
    encoder
        .read_to_end(&mut out)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(out)
}

#[op2]
#[serde]
pub fn op_zlib_gunzip(#[buffer] data: JsBuffer) -> Result<Vec<u8>, JsErrorBox> {
    let mut decoder = GzDecoder::new(data.as_ref());
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(out)
}

#[op2]
#[serde]
pub fn op_zlib_deflate(
    #[buffer] data: JsBuffer,
    #[smi] level: i32,
) -> Result<Vec<u8>, JsErrorBox> {
    let mut encoder = DeflateEncoder::new(data.as_ref(), compression_level(level));
    let mut out = Vec::new();
    encoder
        .read_to_end(&mut out)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(out)
}

#[op2]
#[serde]
pub fn op_zlib_inflate(#[buffer] data: JsBuffer) -> Result<Vec<u8>, JsErrorBox> {
    let mut decoder = DeflateDecoder::new(data.as_ref());
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(out)
}
