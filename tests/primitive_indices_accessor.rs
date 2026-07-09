//! End-to-end validation of the glTF 2.0 §5.24.2 (mesh.primitive.indices)
//! rule that, when defined, the indices accessor MUST have SCALAR type and
//! an unsigned integer component type (5121 / 5123 / 5125).
//!
//! Each test drives a document through the public `GltfDecoder` API and
//! asserts either a `PrimitiveIndices…`-prefixed `Error::InvalidData` or a
//! clean decode.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

// 64 zero bytes, base64-encoded. Positions read as (0,0,0); indices read
// as zeros (all in range for a 3-vertex primitive).
const B64_64: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// A single-triangle mesh whose `indices` accessor `type` / `componentType`
/// are injected so a spec-non-conformant index accessor can be exercised.
/// The POSITION accessor is a well-formed SCALAR-free VEC3 with bounds so
/// the indices rule is the pass under test.
fn doc_with_indices(kind: &str, component_type: u32) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 64, "uri": "data:application/octet-stream;base64,{B64_64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 }},
            {{ "buffer": 0, "byteOffset": 36, "byteLength": 24, "target": 34963 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0,0,0], "max": [0,0,0] }},
            {{ "bufferView": 1, "componentType": {component_type}, "count": 3, "type": "{kind}" }}
        ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }}, "indices": 1 }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(kind: &str, component_type: u32) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_indices(kind, component_type))
        .expect_err("indices accessor should have been rejected");
    format!("{err}")
}

fn decode_ok(kind: &str, component_type: u32) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_indices(kind, component_type))
        .unwrap_or_else(|e| panic!("indices {kind}/{component_type} should be accepted: {e}"));
}

#[test]
fn accepts_scalar_unsigned_short() {
    decode_ok("SCALAR", 5123);
}

#[test]
fn accepts_scalar_unsigned_byte() {
    decode_ok("SCALAR", 5121);
}

#[test]
fn accepts_scalar_unsigned_int() {
    decode_ok("SCALAR", 5125);
}

#[test]
fn rejects_float_component_type() {
    let msg = decode_err("SCALAR", 5126);
    assert!(msg.contains("PrimitiveIndicesComponentType"), "got: {msg}");
}

#[test]
fn rejects_signed_short_component_type() {
    let msg = decode_err("SCALAR", 5122);
    assert!(msg.contains("PrimitiveIndicesComponentType"), "got: {msg}");
}

#[test]
fn rejects_vec2_type() {
    let msg = decode_err("VEC2", 5123);
    assert!(msg.contains("PrimitiveIndicesType"), "got: {msg}");
}
