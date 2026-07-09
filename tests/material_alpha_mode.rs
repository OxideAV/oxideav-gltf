//! End-to-end validation of the glTF 2.0 §5.19.3 (material.alphaMode)
//! rule that `alphaMode`, when present, MUST be one of the enumerated
//! strings "OPAQUE", "MASK", or "BLEND".
//!
//! Each test drives a document through the public `GltfDecoder` API and
//! asserts either the `MaterialAlphaMode`-prefixed `Error::InvalidData` or
//! a clean decode.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A minimal document carrying a single material with the given
/// `alphaMode` token injected verbatim (so a non-enum string can be
/// exercised). The material is otherwise well-formed.
fn doc_with_alpha_mode(alpha_mode: &str) -> Vec<u8> {
    let field = if alpha_mode.is_empty() {
        String::new()
    } else {
        format!("\"alphaMode\": \"{alpha_mode}\"")
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "materials": [ {{ {field} }} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(alpha_mode: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_alpha_mode(alpha_mode))
        .expect_err("material alphaMode should have been rejected");
    format!("{err}")
}

fn decode_ok(alpha_mode: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_alpha_mode(alpha_mode))
        .unwrap_or_else(|e| panic!("alphaMode {alpha_mode} should be accepted: {e}"));
}

#[test]
fn accepts_opaque() {
    decode_ok("OPAQUE");
}

#[test]
fn accepts_mask() {
    decode_ok("MASK");
}

#[test]
fn accepts_blend() {
    decode_ok("BLEND");
}

#[test]
fn accepts_absent_alpha_mode() {
    decode_ok("");
}

#[test]
fn rejects_lowercase_opaque() {
    let msg = decode_err("opaque");
    assert!(msg.contains("MaterialAlphaMode"), "got: {msg}");
}

#[test]
fn rejects_unknown_alpha_mode() {
    let msg = decode_err("DITHER");
    assert!(msg.contains("MaterialAlphaMode"), "got: {msg}");
}
