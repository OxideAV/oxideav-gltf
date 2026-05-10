//! `.glb` binary container reader + writer (Khronos glTF 2.0 §4.4).
//!
//! Layout:
//!
//! ```text
//! Header (12 B): magic = b"glTF"  (0x46546C67 LE)
//!                version = 2u32 LE
//!                length = total file size in bytes (LE)
//!
//! Chunk N: chunkLength: u32 LE
//!          chunkType:   u32 LE  ("JSON" = 0x4E4F534A, "BIN\0" = 0x004E4942)
//!          chunkData:   [u8; chunkLength]      // padded to 4-byte multiple
//! ```
//!
//! Padding rule (§4.4): JSON chunk is padded with U+0020 spaces (0x20),
//! BIN chunk is padded with zeros, both up to a 4-byte multiple. Total
//! file `length` includes padding.

use crate::error::{invalid, Error, Result};

pub const GLB_MAGIC: u32 = 0x46546C67;
pub const GLB_VERSION: u32 = 2;
pub const CHUNK_TYPE_JSON: u32 = 0x4E4F534A;
pub const CHUNK_TYPE_BIN: u32 = 0x004E4942;

/// A parsed `.glb` payload — the raw JSON chunk bytes plus the
/// optional BIN chunk bytes.
#[derive(Debug)]
pub struct GlbPayload<'a> {
    pub json: &'a [u8],
    pub bin: Option<&'a [u8]>,
}

/// Parse the 12-byte header + chunked body of a `.glb` file. Returns
/// borrowed slices into `bytes`.
pub fn parse(bytes: &[u8]) -> Result<GlbPayload<'_>> {
    if bytes.len() < 12 {
        return Err(invalid("glb: too short for 12-byte header"));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if magic != GLB_MAGIC {
        return Err(invalid(format!(
            "glb: bad magic 0x{magic:08X}, expected 0x{GLB_MAGIC:08X}"
        )));
    }
    if version != GLB_VERSION {
        return Err(Error::Unsupported(format!(
            "glb: version {version}, only 2 supported"
        )));
    }
    if length > bytes.len() {
        return Err(invalid(format!(
            "glb: header length {length} > buffer {}",
            bytes.len()
        )));
    }

    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    let mut cursor = 12usize;
    while cursor < length {
        if length - cursor < 8 {
            return Err(invalid(format!(
                "glb: truncated chunk header at offset {cursor}"
            )));
        }
        let chunk_len = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let chunk_type = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap());
        let data_start = cursor + 8;
        let data_end = data_start
            .checked_add(chunk_len)
            .ok_or_else(|| invalid("glb: chunk length overflow"))?;
        if data_end > length {
            return Err(invalid(format!(
                "glb: chunk type 0x{chunk_type:08X} length {chunk_len} overruns container"
            )));
        }
        match chunk_type {
            CHUNK_TYPE_JSON => {
                if json.is_some() {
                    return Err(invalid("glb: more than one JSON chunk"));
                }
                json = Some(&bytes[data_start..data_end]);
            }
            CHUNK_TYPE_BIN => {
                if bin.is_some() {
                    return Err(invalid("glb: more than one BIN chunk"));
                }
                bin = Some(&bytes[data_start..data_end]);
            }
            _ => {
                // Spec §4.4.2 says clients MUST ignore unknown chunks.
            }
        }
        cursor = data_end;
    }

    let json = json.ok_or_else(|| invalid("glb: missing required JSON chunk"))?;
    Ok(GlbPayload { json, bin })
}

/// Build a `.glb` byte stream from a JSON payload + optional binary
/// buffer. JSON is padded with `0x20`, BIN with `0x00`, both to a
/// 4-byte multiple.
pub fn encode(json: &[u8], bin: Option<&[u8]>) -> Vec<u8> {
    let json_pad = (4 - (json.len() % 4)) % 4;
    let bin_padded = bin.map(|b| {
        let pad = (4 - (b.len() % 4)) % 4;
        (b, pad)
    });

    let mut total: usize = 12 + 8 + json.len() + json_pad;
    if let Some((b, pad)) = bin_padded {
        total += 8 + b.len() + pad;
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&GLB_VERSION.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());

    // JSON chunk
    out.extend_from_slice(&((json.len() + json_pad) as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_TYPE_JSON.to_le_bytes());
    out.extend_from_slice(json);
    if json_pad > 0 {
        out.resize(out.len() + json_pad, 0x20);
    }

    // BIN chunk (optional)
    if let Some((b, pad)) = bin_padded {
        out.extend_from_slice(&((b.len() + pad) as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_TYPE_BIN.to_le_bytes());
        out.extend_from_slice(b);
        if pad > 0 {
            out.resize(out.len() + pad, 0);
        }
    }
    debug_assert_eq!(out.len(), total);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_empty_bin() {
        let json = b"{\"asset\":{\"version\":\"2.0\"}}";
        let glb = encode(json, None);
        assert_eq!(&glb[0..4], b"glTF");
        let parsed = parse(&glb).unwrap();
        assert_eq!(&parsed.json[..json.len()], json.as_slice());
        assert!(parsed.bin.is_none());
    }

    #[test]
    fn roundtrip_with_bin() {
        let json = b"{\"asset\":{\"version\":\"2.0\"}}";
        let bin = vec![1u8, 2, 3, 4, 5];
        let glb = encode(json, Some(&bin));
        let parsed = parse(&glb).unwrap();
        // json is padded with spaces — original first bytes match.
        assert_eq!(&parsed.json[..json.len()], json.as_slice());
        // bin is padded with zeros — original first bytes match.
        let parsed_bin = parsed.bin.unwrap();
        assert_eq!(&parsed_bin[..bin.len()], &bin[..]);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = vec![0u8; 12];
        buf[0] = b'X';
        assert!(parse(&buf).is_err());
    }
}
