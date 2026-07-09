//! End-to-end validation of the glTF 2.0 accessor.componentType (§5.1.5)
//! and accessor.type (§5.1.6) enum rules, exercised on a bufferView-less
//! fully-sparse accessor — the case the bufferView-fit pass never sees.
//!
//! A §3.6.2.3 accessor may omit `bufferView` (its base array is implicit
//! zeros), so only the document-level `validate_accessors` pass gates the
//! `componentType` / `type` enums for it.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

// 64 zero bytes, base64-encoded.
const B64_64: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// A minimal document with one unreferenced, bufferView-less sparse
/// accessor whose `componentType` / `type` are injected verbatim. The
/// sparse block sources its indices + values from real bufferViews so only
/// the enum rules are under test.
fn doc_with_accessor(kind: &str, component_type: u32) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 64, "uri": "data:application/octet-stream;base64,{B64_64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0,  "byteLength": 4  }},
            {{ "buffer": 0, "byteOffset": 4,  "byteLength": 12 }}
        ],
        "accessors": [ {{
            "componentType": {component_type}, "count": 2, "type": "{kind}",
            "sparse": {{
                "count": 1,
                "indices": {{ "bufferView": 0, "componentType": 5121 }},
                "values": {{ "bufferView": 1 }}
            }}
        }} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(kind: &str, component_type: u32) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_accessor(kind, component_type))
        .expect_err("accessor should have been rejected");
    format!("{err}")
}

fn decode_ok(kind: &str, component_type: u32) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_accessor(kind, component_type))
        .unwrap_or_else(|e| panic!("accessor {kind}/{component_type} should be accepted: {e}"));
}

#[test]
fn accepts_valid_vec3_float() {
    decode_ok("VEC3", 5126);
}

#[test]
fn rejects_unknown_component_type() {
    let msg = decode_err("VEC3", 5124);
    assert!(msg.contains("AccessorComponentType"), "got: {msg}");
}

#[test]
fn rejects_unknown_type_string() {
    let msg = decode_err("VEC5", 5126);
    assert!(msg.contains("AccessorType"), "got: {msg}");
}
