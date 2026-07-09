//! End-to-end validation of the glTF 2.0 §5.11 bufferView data-kind
//! MUSTs:
//!
//!   * A bufferView MUST contain only one kind of data (image / vertex
//!     indices / vertex attributes / inverse bind matrices) —
//!     `BufferViewMixedData`.
//!   * When two or more vertex-attribute accessors use the same bufferView
//!     its byteStride MUST be defined — `BufferViewInterleavedStride`.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

fn b64(bin: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bin)
}

fn decode_err(doc: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(doc.as_bytes())
        .expect_err("document should have been rejected");
    format!("{err}")
}

fn decode_ok(doc: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(doc.as_bytes())
        .unwrap_or_else(|e| panic!("document should be accepted: {e}"));
}

/// Two VEC3 float accessors (POSITION + NORMAL) share a single 72-byte
/// bufferView carrying an explicit interleaved byteStride — legal per
/// §5.11.
#[test]
fn accepts_interleaved_attributes_with_byte_stride() {
    // 6 VEC3 floats interleaved (pos, nrm) × 3 = 72 bytes.
    let mut bin = vec![0u8; 72];
    // give POSITION accessor 0 a valid min/max window: all zeros.
    for b in bin.iter_mut() {
        *b = 0;
    }
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 72, "uri": "data:application/octet-stream;base64,{}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 72, "byteStride": 24 }} ],
        "accessors": [
            {{ "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0,0,0], "max": [0,0,0] }},
            {{ "bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": 3, "type": "VEC3" }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "mode": 0
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#,
        b64(&bin)
    );
    decode_ok(&doc);
}

/// The same interleaved layout WITHOUT byteStride — a §5.11 MUST NOT.
#[test]
fn rejects_shared_attribute_bufferview_without_byte_stride() {
    let bin = vec![0u8; 72];
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 72, "uri": "data:application/octet-stream;base64,{}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 72 }} ],
        "accessors": [
            {{ "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0,0,0], "max": [0,0,0] }},
            {{ "bufferView": 0, "byteOffset": 36, "componentType": 5126, "count": 3, "type": "VEC3" }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "mode": 0
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#,
        b64(&bin)
    );
    let msg = decode_err(&doc);
    assert!(msg.contains("BufferViewInterleavedStride"), "got: {msg}");
}

/// A bufferView used both as a vertex-attribute source AND as an image
/// source — two kinds of data on one bufferView, a §5.11 MUST NOT.
#[test]
fn rejects_bufferview_used_for_attribute_and_image() {
    // 36 bytes of VEC3 float data doubling as opaque "image" bytes. The
    // image carries a PNG magic prefix so the mimeType is well-formed.
    let mut bin = vec![0u8; 36];
    bin[..8].copy_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 36, "uri": "data:application/octet-stream;base64,{}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }} ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0,0,0], "max": [0,0,0] }}
        ],
        "images": [ {{ "bufferView": 0, "mimeType": "image/png" }} ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {{ "POSITION": 0 }}, "mode": 0
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#,
        b64(&bin)
    );
    let msg = decode_err(&doc);
    assert!(msg.contains("BufferViewMixedData"), "got: {msg}");
}

/// A bufferView used both for vertex indices AND vertex attributes — the
/// spec's explicit illustration of the MUST NOT.
#[test]
fn rejects_bufferview_used_for_indices_and_attribute() {
    // 12 bytes: 3 u32 indices doubling as the attribute source (VEC3 of a
    // single vertex would be 12 bytes too).
    let bin = vec![0u8; 12];
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": 12, "uri": "data:application/octet-stream;base64,{}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 12 }} ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
               "min": [0,0,0], "max": [0,0,0] }},
            {{ "bufferView": 0, "componentType": 5125, "count": 3, "type": "SCALAR" }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {{ "POSITION": 0 }}, "indices": 1, "mode": 0
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#,
        b64(&bin)
    );
    let msg = decode_err(&doc);
    assert!(msg.contains("BufferViewMixedData"), "got: {msg}");
}
