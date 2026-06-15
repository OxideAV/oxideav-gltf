//! End-to-end core-accessor property validation per glTF 2.0 §3.6.2
//! (Accessor Data) + §5.1 (Accessor) — round r311.
//!
//! Each test drives a document carrying a single malformed `accessors[]`
//! entry through the public `GltfDecoder` API and asserts the
//! spec-prefixed `Error::InvalidData` surfaces. The per-rule logic also
//! has unit coverage in `src/validation.rs`; the tests here pin the
//! `validate_accessors` call wiring inside `convert()` so a future
//! refactor can't silently drop it.
//!
//! Rules under test:
//!   §5.1        count MUST be >= 1                  (AccessorCount)
//!   §5.1.6      normalized MUST NOT be true for      (AccessorNormalizedComponentType)
//!               FLOAT (5126) / UNSIGNED_INT (5125)
//!   §3.6.2.5    min/max length == component count    (AccessorMinMaxLength)

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Minimal document whose single `accessors[0]` entry is supplied as raw
/// JSON. A 64-byte buffer + a bufferView keep every reasonable accessor
/// layout in range so the earlier `validate_accessor_fits_bufferview`
/// pass succeeds and our `validate_accessors` pass is the one that fires.
/// The accessor need not be referenced by a mesh — `validate_accessors`
/// walks the whole `accessors[]` array (like `validate_cameras`).
fn doc_with_accessor(accessor_json: &str) -> Vec<u8> {
    // 64 zero bytes, base64-encoded ("AAAA..." → 64 bytes is 88 chars).
    let b64 = "data:application/octet-stream;base64,\
        AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "uri": "{b64}", "byteLength": 64 }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 64 }} ],
        "accessors": [ {accessor_json} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(accessor_json: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_accessor(accessor_json))
        .expect_err("accessor document should have been rejected");
    format!("{err}")
}

fn decode_ok(accessor_json: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_accessor(accessor_json))
        .unwrap_or_else(|e| panic!("accessor {accessor_json} should be accepted: {e}"));
}

#[test]
fn valid_accessors_pass_through_the_decoder() {
    // A spread of conformant accessors: FLOAT VEC3 with correct-length
    // min/max, a normalized signed-byte VEC3 (the one componentType that
    // MAY be normalized), an UNSIGNED_INT SCALAR left un-normalized, and
    // a MAT4 with 16-component bounds.
    for a in [
        r#"{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0] }"#,
        r#"{ "bufferView": 0, "componentType": 5120, "count": 1, "type": "VEC3",
             "normalized": true }"#,
        r#"{ "bufferView": 0, "componentType": 5125, "count": 1, "type": "SCALAR" }"#,
        r#"{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "MAT4",
             "min": [0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0],
             "max": [1,1,1,1, 1,1,1,1, 1,1,1,1, 1,1,1,1] }"#,
    ] {
        decode_ok(a);
    }
}

#[test]
fn rejects_zero_count() {
    // §5.1 — count Minimum: >= 1.
    let msg =
        decode_err(r#"{ "bufferView": 0, "componentType": 5126, "count": 0, "type": "VEC3" }"#);
    assert!(msg.contains("AccessorCount"), "got: {msg}");
}

#[test]
fn rejects_normalized_float() {
    // §5.1.6 — normalized MUST NOT be true for FLOAT.
    let msg = decode_err(
        r#"{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
             "normalized": true }"#,
    );
    assert!(
        msg.contains("AccessorNormalizedComponentType"),
        "got: {msg}"
    );
}

#[test]
fn rejects_normalized_unsigned_int() {
    // §5.1.6 — normalized MUST NOT be true for UNSIGNED_INT.
    let msg = decode_err(
        r#"{ "bufferView": 0, "componentType": 5125, "count": 1, "type": "SCALAR",
             "normalized": true }"#,
    );
    assert!(
        msg.contains("AccessorNormalizedComponentType"),
        "got: {msg}"
    );
}

#[test]
fn rejects_min_length_mismatch() {
    // §3.6.2.5 — VEC3 has 3 components but min carries 2.
    let msg = decode_err(
        r#"{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
             "min": [0.0, 0.0], "max": [1.0, 1.0, 1.0] }"#,
    );
    assert!(msg.contains("AccessorMinMaxLength"), "got: {msg}");
}

#[test]
fn rejects_max_length_mismatch() {
    // §3.6.2.5 — VEC3 has 3 components but max carries 4.
    let msg = decode_err(
        r#"{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0, 1.0] }"#,
    );
    assert!(msg.contains("AccessorMinMaxLength"), "got: {msg}");
}

#[test]
fn accepts_normalized_signed_short() {
    // §5.1.6 only bars FLOAT / UNSIGNED_INT; a normalized SHORT is fine.
    decode_ok(
        r#"{ "bufferView": 0, "componentType": 5122, "count": 1, "type": "VEC2",
             "normalized": true }"#,
    );
}
