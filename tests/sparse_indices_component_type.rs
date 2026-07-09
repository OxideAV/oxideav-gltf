//! End-to-end validation of the glTF 2.0 §5.3.3
//! (accessor.sparse.indices.componentType) rule that the sparse index
//! component type MUST be one of the three unsigned integer types
//! 5121 / 5123 / 5125.
//!
//! The accessor is intentionally left unreferenced so the sparse block is
//! never materialised — the document-level `validate_sparse_indices_buffer_views`
//! pass is the thing under test (it fires on every declared sparse block,
//! not only the accessors a mesh reads).

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

// 64 zero bytes, base64-encoded.
const B64_64: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn doc_with_sparse_index_component(component_type: u32) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 64, "uri": "data:application/octet-stream;base64,{B64_64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0,  "byteLength": 24 }},
            {{ "buffer": 0, "byteOffset": 24, "byteLength": 4  }},
            {{ "buffer": 0, "byteOffset": 28, "byteLength": 12 }}
        ],
        "accessors": [ {{
            "bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
            "sparse": {{
                "count": 1,
                "indices": {{ "bufferView": 1, "componentType": {component_type} }},
                "values": {{ "bufferView": 2 }}
            }}
        }} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(component_type: u32) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_sparse_index_component(component_type))
        .expect_err("sparse indices componentType should have been rejected");
    format!("{err}")
}

fn decode_ok(component_type: u32) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_sparse_index_component(component_type))
        .unwrap_or_else(|e| {
            panic!("sparse indices componentType {component_type} should be accepted: {e}")
        });
}

#[test]
fn accepts_unsigned_byte() {
    decode_ok(5121);
}

#[test]
fn accepts_unsigned_short() {
    decode_ok(5123);
}

#[test]
fn accepts_unsigned_int() {
    decode_ok(5125);
}

#[test]
fn rejects_signed_short() {
    let msg = decode_err(5122);
    assert!(msg.contains("SparseIndicesComponentType"), "got: {msg}");
}

#[test]
fn rejects_float() {
    let msg = decode_err(5126);
    assert!(msg.contains("SparseIndicesComponentType"), "got: {msg}");
}
