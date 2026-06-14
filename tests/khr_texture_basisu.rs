//! KHR_texture_basisu extension — per-texture indirection to a KTX
//! v2 image with Basis Universal supercompression per
//! `docs/3d/gltf/extensions/KHR_texture_basisu.md` §glTF Schema
//! Updates.
//!
//! The crate is a pass-through engine (it doesn't transcode KTX2);
//! the extension is therefore handled at the JSON sidecar level:
//!
//! * **Decode**: if the base `texture.source` is present (the
//!   PNG / JPEG fallback path), that source is loaded as today and
//!   the extension's KTX2 source is ignored on the live texture but
//!   acknowledged as "extensionsUsed is OK". If the base
//!   `texture.source` is absent (the spec's "Using Without a
//!   Fallback" shape), the extension's source is loaded instead and
//!   the scene-texture index is recorded under
//!   `Scene3D::extras["KHR_texture_basisu"].textures` so the encoder
//!   re-emits the "without fallback" shape.
//!
//! * **Encode**: every scene-texture-index listed in the sidecar is
//!   emitted with `extensions.KHR_texture_basisu.source = <emitted
//!   image idx>` and the base `texture.source` omitted, and the
//!   extension is declared in BOTH `extensionsUsed` AND
//!   `extensionsRequired` per the spec.
//!
//! * **Validation** (§3.12): a `KHR_texture_basisu` data block on any
//!   texture without the extension declared in `extensionsUsed` is
//!   rejected with `ExtensionStackUsedNotDeclared`; an out-of-range
//!   `KHR_texture_basisu.source` is rejected with
//!   `ExtensionStackTextureBasisuSource`; the "without fallback"
//!   shape (no base `texture.source`) without
//!   `KHR_texture_basisu` in `extensionsRequired` is rejected with
//!   `ExtensionStackTextureBasisuRequired`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{ImageData, Mesh3DDecoder, Mesh3DEncoder, Scene3D, Texture};
use serde_json::Value;

/// Walk the `.glb` container and return its JSON chunk's payload bytes.
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}

fn ktx2_texture() -> Texture {
    // The KTX2 bytes are opaque to us — we never transcode. A
    // 32-byte placeholder with the `image/ktx2` MIME is enough to
    // exercise the buffer-view-backed BIN path.
    Texture::from_encoded("image/ktx2".to_owned(), vec![0xABu8; 32])
}

fn png_texture() -> Texture {
    Texture::from_encoded("image/png".to_owned(), vec![0xCDu8; 16])
}

/// Decode a hand-built JSON document with a "without fallback"
/// KHR_texture_basisu texture and verify the extension sidecar
/// surfaces on `Scene3D::extras`.
#[test]
fn without_fallback_decodes_into_scene_extras() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1, "one texture loaded");
    let basisu = scene
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.as_object())
        .expect("KHR_texture_basisu sidecar present on scene.extras");
    let arr = basisu
        .get("textures")
        .and_then(|v| v.as_array())
        .expect("`textures` sidecar key holds an array of indices");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_u64(), Some(0));
    // The loaded image's URI comes from the extension indirection.
    // The JSON above doesn't carry a `mimeType` for the image, so
    // the loaded asset's MIME is left absent (the file-extension on
    // the URI is the only KTX2 hint).
    match &scene.textures[0].image {
        ImageData::External { uri, mime } => {
            assert_eq!(uri, "image.ktx2");
            assert!(mime.is_none(), "no mimeType in JSON → mime stays None");
        }
        other => panic!("expected External image data, got {other:?}"),
    }
}

#[test]
fn without_fallback_with_mimetype_decodes_with_ktx2_mime() {
    // Same as the previous test but the image carries
    // `mimeType: "image/ktx2"` per spec §"glTF Schema Updates"
    // ("When used in the glTF Binary (GLB) format the image that
    // points to the KTX v2 resource uses the mimeType value of
    // image/ktx2").
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2", "mimeType": "image/ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    match &scene.textures[0].image {
        ImageData::External { uri, mime } => {
            assert_eq!(uri, "image.ktx2");
            assert_eq!(mime.as_deref(), Some("image/ktx2"));
        }
        other => panic!("expected External image data, got {other:?}"),
    }
}

/// Decode a document with both base `texture.source` (PNG) AND the
/// KHR_texture_basisu extension (KTX2 alternate). Our engine picks
/// the PNG fallback as the live image and does NOT record the
/// texture in the sidecar (since the fallback path was taken).
#[test]
fn with_fallback_picks_base_source_no_sidecar() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "textures": [
            {
                "source": 0,
                "extensions": {
                    "KHR_texture_basisu": { "source": 1 }
                }
            }
        ],
        "images": [
            { "uri": "image.png" },
            { "uri": "image.ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1);
    match &scene.textures[0].image {
        ImageData::External { uri, .. } => assert_eq!(uri, "image.png"),
        other => panic!("expected PNG fallback path, got {other:?}"),
    }
    // No sidecar entry because the fallback path was active.
    assert!(
        !scene.extras.contains_key("KHR_texture_basisu"),
        "no sidecar when the fallback PNG/JPEG path was loaded"
    );
}

/// A scene whose texture's loaded image came from the extension
/// (sidecar present) must re-emit the "without fallback" shape:
/// `texture.source` omitted, `KHR_texture_basisu.source` set, and
/// the extension declared in both `extensionsUsed` AND
/// `extensionsRequired`.
#[test]
fn sidecar_round_trips_to_without_fallback_glb() {
    let mut scene = Scene3D::new();
    scene.add_texture(ktx2_texture());
    let mut top = serde_json::Map::new();
    top.insert("textures".to_owned(), Value::Array(vec![Value::from(0u32)]));
    scene
        .extras
        .insert("KHR_texture_basisu".to_owned(), Value::Object(top));

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = String::from_utf8(extract_json_chunk(&glb)).unwrap();

    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted: {raw}"
    );
    assert!(
        raw.contains("\"extensionsRequired\""),
        "extensionsRequired must be emitted: {raw}"
    );
    assert!(
        raw.contains("\"KHR_texture_basisu\""),
        "extension name must appear: {raw}"
    );
    // No `source` on the texture object itself — only inside the
    // extension. We assert structurally by parsing.
    let root: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let tex = &root["textures"][0];
    assert!(tex.get("source").is_none(), "base source must be omitted");
    let ext_src = tex["extensions"]["KHR_texture_basisu"]["source"]
        .as_u64()
        .expect("extension.source must be set");
    assert_eq!(ext_src, 0, "image[0] is the only emitted image");

    // Round-trip back through the decoder — the sidecar survives.
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let sidecar = decoded
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.as_object())
        .expect("sidecar survives round-trip");
    let arr = sidecar.get("textures").and_then(|v| v.as_array()).unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_u64(), Some(0));
}

/// A texture without the sidecar marker must NOT trigger the
/// extension emission. This is the regression guard: every existing
/// test that encodes a plain PNG texture must still produce a
/// `texture.source = N` shape with no extensions block.
#[test]
fn plain_texture_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_texture(png_texture());
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = String::from_utf8(extract_json_chunk(&glb)).unwrap();
    assert!(
        !raw.contains("KHR_texture_basisu"),
        "extension must NOT appear when no sidecar marks the texture: {raw}"
    );
    let root: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let tex = &root["textures"][0];
    assert!(tex.get("source").is_some(), "base source must be set");
    assert!(
        tex.get("extensions").is_none(),
        "no extensions block on a plain texture: {tex}"
    );
}

/// Validation: a `KHR_texture_basisu` data block on any texture
/// without `KHR_texture_basisu` in `extensionsUsed` is rejected per
/// spec §3.12.
#[test]
fn data_block_without_extensions_used_is_rejected() {
    // No extensionsUsed declaration but the extension is materialised.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2" }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    // The §3.12 subset check (extensionsRequired ⊆ extensionsUsed)
    // fires first; both checks defend the wall.
    assert!(
        msg.contains("ExtensionStackRequiredNotListed")
            || msg.contains("ExtensionStackUsedNotDeclared"),
        "expected ExtensionStack… rejection, got: {msg}"
    );
}

#[test]
fn data_block_with_only_used_no_required_is_rejected_when_no_fallback() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2" }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackTextureBasisuRequired"),
        "expected ExtensionStackTextureBasisuRequired (no fallback \
         source means the extension MUST be in extensionsRequired): \
         got {msg}"
    );
}

#[test]
fn out_of_range_source_is_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 7 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2" }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackTextureBasisuSource"),
        "expected ExtensionStackTextureBasisuSource rejection, got: {msg}"
    );
}

#[test]
fn texture_with_neither_source_nor_basisu_is_rejected() {
    // Spec: every texture must have a source (either base
    // `source` or the `KHR_texture_basisu.source` indirection).
    let json = br#"{
        "asset": { "version": "2.0" },
        "textures": [
            { "sampler": 0 }
        ],
        "samplers": [
            { "magFilter": 9729 }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("missing source"),
        "expected 'missing source' rejection, got: {msg}"
    );
}

#[test]
fn with_fallback_does_not_force_required_on_encode() {
    // A scene constructed without the sidecar (i.e. a plain PNG
    // texture) must NOT emit the extension at all on encode. This
    // mirrors the "with fallback" original shape where capable
    // engines pick the KTX2 — we only carry through the sidecar
    // when our decoder loaded the KTX2 path (because the base was
    // missing).
    let mut scene = Scene3D::new();
    scene.add_texture(png_texture());
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = String::from_utf8(extract_json_chunk(&glb)).unwrap();
    assert!(!raw.contains("KHR_texture_basisu"), "no extension: {raw}");
    assert!(
        !raw.contains("extensionsRequired"),
        "no extensionsRequired when no extension is emitted: {raw}"
    );
}

#[test]
fn sidecar_with_unknown_index_is_ignored() {
    // The sidecar carries a texture index past the end of
    // `scene.textures`. The encoder silently skips it — it can't
    // synthesise a texture out of thin air, and the index is
    // documented as referring to scene-texture indices. This is a
    // robustness guard against a hand-crafted scene.
    let mut scene = Scene3D::new();
    scene.add_texture(png_texture());
    let mut top = serde_json::Map::new();
    top.insert(
        "textures".to_owned(),
        Value::Array(vec![Value::from(99u32)]),
    );
    scene
        .extras
        .insert("KHR_texture_basisu".to_owned(), Value::Object(top));

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = String::from_utf8(extract_json_chunk(&glb)).unwrap();
    let root: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // The lone scene-texture-0 was NOT in the sidecar's set
    // (which only contained 99), so it must encode as a plain
    // texture, and no extension is declared in `extensionsUsed`.
    let tex = &root["textures"][0];
    assert!(
        tex.get("extensions").is_none(),
        "no per-texture extensions block on a non-marked texture: {tex}"
    );
    assert!(
        tex.get("source").is_some(),
        "base source must be present on a non-marked texture"
    );
    let used = root
        .get("extensionsUsed")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|v| v.as_str() == Some("KHR_texture_basisu")))
        .unwrap_or(false);
    assert!(
        !used,
        "stale sidecar index must not promote the extension into \
         extensionsUsed: {raw}"
    );
    let required = root
        .get("extensionsRequired")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|v| v.as_str() == Some("KHR_texture_basisu")))
        .unwrap_or(false);
    assert!(
        !required,
        "stale sidecar index must not promote the extension into \
         extensionsRequired: {raw}"
    );
}

#[test]
fn embedded_ktx2_data_uri_decodes_into_scene_extras() {
    // base64("KTX 20\xBB\r\n\x1A\n") — a 12-byte placeholder; only
    // the indirection is exercised here.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "data:image/ktx2;base64,S1RYIDIwuw0KGgo=" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1);
    let basisu = scene
        .extras
        .get("KHR_texture_basisu")
        .and_then(|v| v.as_object())
        .expect("sidecar present on extras");
    let arr = basisu.get("textures").and_then(|v| v.as_array()).unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_u64(), Some(0));
}

/// Validation rule 4: when the image referenced by
/// `KHR_texture_basisu.source` declares a `mimeType`, that value MUST
/// be `image/ktx2` (spec §Overview + §"glTF Schema Updates": "the
/// image that points to the KTX v2 resource uses the mimeType value
/// of image/ktx2"). A target image carrying `image/png` is rejected
/// with `ExtensionStackTextureBasisuMimeType`.
#[test]
fn basisu_target_image_with_wrong_mimetype_is_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2", "mimeType": "image/png" }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackTextureBasisuMimeType"),
        "expected ExtensionStackTextureBasisuMimeType rejection for a \
         non-image/ktx2 target image, got: {msg}"
    );
}

/// The explicit `image/ktx2` mimeType on the target image is the
/// canonical, accepted shape (spec GLB example). It must validate.
#[test]
fn basisu_target_image_with_ktx2_mimetype_is_accepted() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2", "mimeType": "image/ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1, "the canonical KTX2 shape decodes");
}

/// A target image that omits `mimeType` entirely is permitted — the
/// spec only constrains the value when a mimeType is present (the
/// uri-only "fallback" example carries no mimeType). Guards against
/// the validator over-firing on the bare-uri case.
#[test]
fn basisu_target_image_without_mimetype_is_accepted() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_basisu"],
        "extensionsRequired": ["KHR_texture_basisu"],
        "textures": [
            {
                "extensions": {
                    "KHR_texture_basisu": { "source": 0 }
                }
            }
        ],
        "images": [
            { "uri": "image.ktx2" }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.textures.len(), 1, "uri-only target image decodes");
}
