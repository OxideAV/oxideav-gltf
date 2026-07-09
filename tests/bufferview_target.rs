//! End-to-end validation of the glTF 2.0 §5.11.5 (bufferView.target) rule
//! that the OPTIONAL `target` hint, when present, MUST hold one of the two
//! WebGL buffer-binding enum constants: 34962 (ARRAY_BUFFER) or 34963
//! (ELEMENT_ARRAY_BUFFER).
//!
//! Each test drives a document through the public `GltfDecoder` API and
//! asserts either the `BufferViewTarget`-prefixed `Error::InvalidData` or a
//! clean decode.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A minimal document whose single bufferView optionally carries a
/// `target` value. The 64-byte buffer keeps the view in range so the
/// target rule is the pass under test. `target` is a raw JSON token so a
/// non-enum value can be injected.
fn doc_with_bufferview_target(target: &str) -> Vec<u8> {
    let target_field = if target.is_empty() {
        String::new()
    } else {
        format!(", \"target\": {target}")
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 64, "uri": "data:application/octet-stream;base64,{B64_64}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 64{target_field} }} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

// 64 zero bytes, base64-encoded.
const B64_64: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn decode_err(target: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_bufferview_target(target))
        .expect_err("bufferView document should have been rejected");
    format!("{err}")
}

fn decode_ok(target: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_bufferview_target(target))
        .unwrap_or_else(|e| panic!("bufferView target {target} should be accepted: {e}"));
}

#[test]
fn accepts_array_buffer_target() {
    decode_ok("34962");
}

#[test]
fn accepts_element_array_buffer_target() {
    decode_ok("34963");
}

#[test]
fn accepts_absent_target() {
    decode_ok("");
}

#[test]
fn rejects_arbitrary_target() {
    let msg = decode_err("34960");
    assert!(msg.contains("BufferViewTarget"), "got: {msg}");
}

#[test]
fn rejects_zero_target() {
    let msg = decode_err("0");
    assert!(msg.contains("BufferViewTarget"), "got: {msg}");
}
