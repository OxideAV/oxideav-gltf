//! KHR_texture_basisu extension — alternate KTX v2 image with Basis
//! Universal supercompression per
//! `docs/3d/gltf/extensions/KHR_texture_basisu.md`.
//!
//! Two spec-defined shapes (§glTF Schema Updates + §"Using Without a
//! Fallback") survive a round-trip through this crate via the
//! `scene.extras["KHR_texture_basisu"]` side-channel:
//!
//! * Fallback case — `texture.source` holds a PNG/JPEG (the only image
//!   carried in `Texture::image`); the alternate KTX2 image rides on
//!   `scene.extras["KHR_texture_basisu"].alternates[i].ktx2_image` so
//!   the encoder can re-emit it under `images[]` and wire up the
//!   extension reference.
//! * Required-only case — `texture.source` is absent; the texture's
//!   primary image IS the KTX2 one and the extras record carries the
//!   sentinel `"primary_is_ktx2": true` so the encoder reproduces the
//!   `texture.source = null` + `extensionsRequired` shape.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{ImageData, Mesh3DDecoder, Mesh3DEncoder, Scene3D, Texture};
use serde_json::{json, Value};

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}

fn png_texture(name: &str) -> Texture {
    let mut t = Texture::from_encoded("image/png".to_owned(), vec![0x89u8; 64]);
    t.name = Some(name.to_owned());
    t
}

fn ktx2_texture(name: &str) -> Texture {
    let mut t = Texture::from_encoded("image/ktx2".to_owned(), vec![0xABu8; 96]);
    t.name = Some(name.to_owned());
    t
}

#[test]
fn fallback_pair_round_trips_via_glb() {
    // Hand-build a fallback-pair Scene3D the way the decoder lifts a
    // KHR_texture_basisu document: one PNG texture as the primary image
    // plus a scene-extras side-channel describing the alternate KTX2.
    let mut scene = Scene3D::new();
    scene.add_texture(png_texture("base"));
    scene.extras.insert(
        "KHR_texture_basisu".to_owned(),
        json!({
            "alternates": [
                {
                    "texture": 0,
                    "primary_is_ktx2": false,
                    "ktx2_image": {
                        "kind": "embedded",
                        "bytes_base64": "qqqqqqqqqqo=",
                        "mimeType": "image/ktx2",
                        "name": "ktx2_alt"
                    }
                }
            ]
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"KHR_texture_basisu\""),
        "extension block on texture, got {raw}"
    );
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed declared, got {raw}"
    );
    // Fallback case — `extensionsRequired` must NOT carry the extension
    // (the fallback is still consumable by clients that ignore it).
    let doc: Value = serde_json::from_slice(&json_bytes).unwrap();
    let req = doc
        .get("extensionsRequired")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !req.iter().any(|v| v.as_str() == Some("KHR_texture_basisu")),
        "fallback case must NOT list extension in extensionsRequired, got {req:?}"
    );

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.textures.len(), 1);
    let record = decoded
        .extras
        .get("KHR_texture_basisu")
        .expect("side-channel survives round-trip");
    let alts = record.get("alternates").and_then(|v| v.as_array()).unwrap();
    assert_eq!(alts.len(), 1);
    assert_eq!(alts[0].get("texture"), Some(&json!(0u32)));
    assert_eq!(
        alts[0].get("primary_is_ktx2"),
        Some(&json!(false)),
        "fallback case survives the round-trip"
    );
    let ktx2 = alts[0].get("ktx2_image").unwrap();
    assert_eq!(ktx2.get("mimeType"), Some(&json!("image/ktx2")));
    // The alternate's bytes survive byte-for-byte through the base64
    // encode/decode legs in `json_to_scene.rs` + `scene_to_json.rs`.
    let raw_bytes = match ktx2.get("kind").and_then(|v| v.as_str()) {
        Some("embedded") => {
            let b64 = ktx2.get("bytes_base64").and_then(|v| v.as_str()).unwrap();
            base64_decode(b64)
        }
        Some("uri") => panic!("expected embedded kind on round-trip, got uri"),
        other => panic!("unexpected kind {other:?}"),
    };
    assert_eq!(raw_bytes.len(), 8, "8-byte sample reproduces");
    assert!(raw_bytes.iter().all(|&b| b == 0xAA));
}

#[test]
fn required_only_round_trips_via_glb() {
    // Required-only shape (§"Using Without a Fallback") — the texture's
    // own `source` is absent and the extension is listed in BOTH
    // `extensionsUsed` AND `extensionsRequired`.
    let mut scene = Scene3D::new();
    scene.add_texture(ktx2_texture("primary_ktx2"));
    scene.extras.insert(
        "KHR_texture_basisu".to_owned(),
        json!({
            "alternates": [
                {
                    "texture": 0,
                    "primary_is_ktx2": true
                }
            ]
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let doc: Value = serde_json::from_slice(&json_bytes).unwrap();
    let used = doc
        .get("extensionsUsed")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let req = doc
        .get("extensionsRequired")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        used.iter()
            .any(|v| v.as_str() == Some("KHR_texture_basisu")),
        "extensionsUsed must carry KHR_texture_basisu"
    );
    assert!(
        req.iter().any(|v| v.as_str() == Some("KHR_texture_basisu")),
        "extensionsRequired must carry KHR_texture_basisu in the no-fallback shape (spec §\"Using Without a Fallback\")"
    );
    // The texture's own `source` MUST be absent.
    let tex0 = doc
        .get("textures")
        .and_then(|v| v.as_array())
        .unwrap()
        .first()
        .unwrap();
    assert!(
        tex0.get("source").map(|v| v.is_null()).unwrap_or(true),
        "required-only texture must omit `source`, got {tex0:?}"
    );
    // …and the extension MUST point at the texture's actual image index.
    let basisu_src = tex0
        .get("extensions")
        .and_then(|e| e.get("KHR_texture_basisu"))
        .and_then(|b| b.get("source"))
        .and_then(|s| s.as_u64())
        .unwrap();
    assert_eq!(basisu_src, 0, "extension points at the sole image");

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.textures.len(), 1);
    // The primary mesh3d Texture carries the KTX2 image data (mime ==
    // "image/ktx2" per `ktx2_texture()` above).
    if let ImageData::Source(src) = &decoded.textures[0].image {
        assert_eq!(src.mime(), Some("image/ktx2"));
    } else {
        panic!(
            "expected ImageData::Source, got {:?}",
            decoded.textures[0].image
        );
    }
    let alts = decoded
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.get("alternates"))
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(alts.len(), 1);
    assert_eq!(alts[0].get("primary_is_ktx2"), Some(&json!(true)));
}

#[test]
fn scene_without_basisu_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_texture(png_texture("plain"));
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_texture_basisu"),
        "extension must NOT appear when no texture sets the alternate, got {raw}"
    );
}

#[test]
fn basisu_data_block_without_extensions_used_is_rejected() {
    // Hand-build a glTF JSON document with the extension on a texture
    // but no `extensionsUsed` declaration — spec §3.12 violation, the
    // validator must reject with `ExtensionStackUsedNotDeclared`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "textures": [
            {
                "source": 0,
                "extensions": { "KHR_texture_basisu": { "source": 1 } }
            }
        ],
        "images": [
            { "uri": "fallback.png" },
            { "uri": "ktx2.ktx2", "mimeType": "image/ktx2" }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_texture_basisu"),
        "expected ExtensionStackUsedNotDeclared for KHR_texture_basisu, got {msg}"
    );
}

#[test]
fn basisu_source_out_of_range_is_rejected() {
    // The extension's `source` MUST resolve to a real entry in
    // `images[]`. Image index 99 is well past the single-image roster,
    // so the validator must reject with
    // `ExtensionStackTextureBasisuSource`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "textures": [
            {
                "source": 0,
                "extensions": { "KHR_texture_basisu": { "source": 99 } }
            }
        ],
        "images": [
            { "uri": "fallback.png" }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackTextureBasisuSource"),
        "expected ExtensionStackTextureBasisuSource for out-of-range source, got {msg}"
    );
}

#[test]
fn fallback_decode_recognises_extension_block() {
    // Hand-build a fallback-shape JSON (`texture.source` + extension
    // both present); the decoder should surface the alternate on the
    // side-channel after round-trip.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "textures": [
            {
                "source": 0,
                "extensions": { "KHR_texture_basisu": { "source": 1 } }
            }
        ],
        "images": [
            { "uri": "fallback.png", "mimeType": "image/png" },
            { "uri": "alt.ktx2", "mimeType": "image/ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1);
    let alts = scene
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.get("alternates"))
        .and_then(|v| v.as_array())
        .expect("side-channel must materialise");
    assert_eq!(alts.len(), 1);
    let alt = &alts[0];
    assert_eq!(alt.get("texture"), Some(&json!(0u32)));
    assert_eq!(alt.get("primary_is_ktx2"), Some(&json!(false)));
    let ktx2 = alt.get("ktx2_image").unwrap();
    assert_eq!(ktx2.get("kind"), Some(&json!("uri")));
    assert_eq!(ktx2.get("uri"), Some(&json!("alt.ktx2")));
    assert_eq!(ktx2.get("mimeType"), Some(&json!("image/ktx2")));
    // Primary image stays the PNG fallback on the mesh3d Texture.
    if let ImageData::External { uri, .. } = &scene.textures[0].image {
        assert_eq!(uri, "fallback.png");
    } else {
        panic!("expected External image, got {:?}", scene.textures[0].image);
    }
}

#[test]
fn required_only_decode_recognises_extension_block() {
    // Hand-build a required-only JSON (`texture.source` absent), with
    // KHR_texture_basisu in both extensionsUsed AND extensionsRequired.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": { "KHR_texture_basisu": { "source": 0 } }
            }
        ],
        "images": [
            { "uri": "primary.ktx2", "mimeType": "image/ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1);
    // Primary image IS the KTX2 (no fallback to fall back on).
    if let ImageData::External { uri, mime } = &scene.textures[0].image {
        assert_eq!(uri, "primary.ktx2");
        assert_eq!(mime.as_deref(), Some("image/ktx2"));
    } else {
        panic!("expected External image, got {:?}", scene.textures[0].image);
    }
    let alts = scene
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.get("alternates"))
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(alts[0].get("primary_is_ktx2"), Some(&json!(true)));
}

#[test]
fn embedded_alternate_bufferview_decodes_to_inline_bytes() {
    // Build a small GLB whose alternate KTX2 image lives in the BIN
    // chunk via a `bufferView`. The decoder must inline the bytes
    // base64-encoded on the side-channel so a round-trip can recreate
    // the alternate.
    let alt_bytes = b"KTX 2 0";
    let mut bin = vec![0u8; 0];
    // Primary png image (8 bytes); KTX2 alternate after a 4-byte pad.
    let png_off = bin.len();
    bin.extend_from_slice(b"PNG_FAKE");
    let png_len = bin.len() - png_off;
    let pad = (4 - (bin.len() % 4)) % 4;
    bin.extend(std::iter::repeat(0u8).take(pad));
    let ktx_off = bin.len();
    bin.extend_from_slice(alt_bytes);
    let ktx_len = bin.len() - ktx_off;
    let pad = (4 - (bin.len() % 4)) % 4;
    bin.extend(std::iter::repeat(0u8).take(pad));
    let bin_len_padded = bin.len();

    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_texture_basisu"],
            "buffers": [{{ "byteLength": {bin_len} }}],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": {p_off}, "byteLength": {p_len} }},
                {{ "buffer": 0, "byteOffset": {k_off}, "byteLength": {k_len} }}
            ],
            "images": [
                {{ "bufferView": 0, "mimeType": "image/png" }},
                {{ "bufferView": 1, "mimeType": "image/ktx2" }}
            ],
            "textures": [
                {{
                    "source": 0,
                    "extensions": {{ "KHR_texture_basisu": {{ "source": 1 }} }}
                }}
            ]
        }}"#,
        bin_len = bin_len_padded,
        p_off = png_off,
        p_len = png_len,
        k_off = ktx_off,
        k_len = ktx_len,
    );

    // Build the GLB envelope: 12-byte header + JSON chunk + BIN chunk.
    let mut json_bytes = json.into_bytes();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes()); // version
    let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
    glb.extend_from_slice(&(total_len as u32).to_le_bytes());
    // JSON chunk.
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);
    // BIN chunk.
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);

    let scene = GltfDecoder::new().decode(&glb).unwrap();
    let alts = scene
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.get("alternates"))
        .and_then(|v| v.as_array())
        .unwrap();
    let ktx2 = alts[0].get("ktx2_image").unwrap();
    assert_eq!(ktx2.get("kind"), Some(&json!("embedded")));
    let b64 = ktx2.get("bytes_base64").and_then(|v| v.as_str()).unwrap();
    let decoded_alt = base64_decode(b64);
    assert_eq!(
        decoded_alt, alt_bytes,
        "alternate bufferView bytes survive base64 round-trip on the side channel"
    );
}

// --------------------------------------------------------------------
// Local RFC 4648 §4 base64 decoder so the test file doesn't pull in
// extra dev-deps. Mirrors the encoder used inside the crate.
fn base64_decode(input: &str) -> Vec<u8> {
    let lookup = |c: u8| -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 char {c:?}"),
        }
    };
    let raw = input.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len() / 4 * 3);
    let mut i = 0;
    while i < raw.len() {
        if raw[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let mut chunk = [0u8; 4];
        let mut pads = 0;
        for slot in chunk.iter_mut() {
            assert!(i < raw.len());
            let c = raw[i];
            i += 1;
            if c == b'=' {
                pads += 1;
            } else {
                *slot = lookup(c);
            }
        }
        let n = (u32::from(chunk[0]) << 18)
            | (u32::from(chunk[1]) << 12)
            | (u32::from(chunk[2]) << 6)
            | u32::from(chunk[3]);
        bytes.push(((n >> 16) & 0xFF) as u8);
        if pads <= 1 {
            bytes.push(((n >> 8) & 0xFF) as u8);
        }
        if pads == 0 {
            bytes.push((n & 0xFF) as u8);
        }
    }
    bytes
}
