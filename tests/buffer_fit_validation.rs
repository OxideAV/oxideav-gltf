//! End-to-end buffer / bufferView / accessor fit validation per
//! glTF 2.0 §3.6.2.4 line 3104 + §5.11 + §5.3.1 (round 8).
//!
//! Each test wires a malformed document through the `GltfDecoder`
//! and confirms the spec-prefixed `Error::InvalidData` surfaces. The
//! unit tests in `src/validation.rs` cover the per-function logic; the
//! tests here pin the wiring to the public decoder API so a future
//! refactor of `convert()` can't accidentally drop the calls.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Wrap a binary buffer as a `data:` URI inside an otherwise valid
/// glTF document; callers supply the accessors / bufferViews JSON
/// arrays plus the primitive attribute map.
fn build_doc(
    bin: &[u8],
    accessors_json: &str,
    buffer_views_json: &str,
    attributes_json: &str,
) -> Vec<u8> {
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(bin);
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [ {buffer_views_json} ],
        "accessors": [ {accessors_json} ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {attributes_json} }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

/// Override the buffer's `byteLength` independently of the actual
/// payload — used to test bufferView fit when the buffer claims it's
/// smaller than the payload pretends.
fn build_doc_with_buffer_length(
    bin: &[u8],
    declared_buffer_byte_length: usize,
    accessors_json: &str,
    buffer_views_json: &str,
    attributes_json: &str,
) -> Vec<u8> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bin);
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {declared_buffer_byte_length},
                        "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [ {buffer_views_json} ],
        "accessors": [ {accessors_json} ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {attributes_json} }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

/// 4 POSITION VEC3 floats = 48 bytes (12 bytes each), tightly packed
/// at offset 0 with declared bounds.
fn positions_48bytes() -> Vec<u8> {
    let positions: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    bin
}

// --- §3.6.2.4: accessor fit in bufferView -------------------------

#[test]
fn rejects_accessor_overrunning_bufferview_tight_pack() {
    // 4 VEC3 floats = 48 bytes; bufferView declares only 47.
    let bin = positions_48bytes();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 47}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("AccessorFitBufferView"),
        "expected AccessorFitBufferView, got: {msg}"
    );
}

#[test]
fn rejects_accessor_overrunning_bufferview_strided() {
    // stride=16, count=4, VEC3 float (element=12) → 16*3 + 12 = 60.
    // bufferView declares 56.
    let bin = vec![0u8; 60];
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 56, "byteStride": 16}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    assert!(
        format!("{err}").contains("AccessorFitBufferView"),
        "got: {err}"
    );
}

#[test]
fn accepts_accessor_exact_bufferview_fit() {
    // 4 VEC3 floats = 48 bytes; bufferView declares exactly 48.
    let bin = positions_48bytes();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 48}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    dec.decode(&doc).expect("exact-fit accessor must accept");
}

// --- §5.11: bufferView fit in buffer ------------------------------

#[test]
fn rejects_bufferview_overrunning_buffer() {
    // Payload is 48 bytes but we lie and declare buffer.byteLength=40.
    let bin = positions_48bytes();
    let doc = build_doc_with_buffer_length(
        &bin,
        40,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 48}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    assert!(
        format!("{err}").contains("BufferViewFitBuffer"),
        "got: {err}"
    );
}

#[test]
fn rejects_bytestride_above_spec_max() {
    // §5.11.4: byteStride MUST be ≤ 252. 256 is just outside.
    let bin = vec![0u8; 1024];
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 1024, "byteStride": 256}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    assert!(
        format!("{err}").contains("BufferViewStrideRange"),
        "got: {err}"
    );
}

// --- §5.3.1: sparse-indices bufferView must not have target / stride -

#[test]
fn rejects_sparse_indices_bufferview_with_target() {
    // VEC3 float sparse-base accessor; sparse.indices points at a
    // bufferView that wrongly has `target` set.
    let bin = vec![0u8; 64];
    let doc = build_doc(
        &bin,
        r#"{"componentType": 5126, "count": 4, "type": "VEC3",
            "sparse": {
                "count": 1,
                "indices": { "bufferView": 0, "componentType": 5121 },
                "values":  { "bufferView": 1 }
            }}"#,
        // bv[0] = indices buffer view (with bad target = 34962)
        // bv[1] = values buffer view (12 bytes for 1 VEC3 float)
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 4, "target": 34962},
           {"buffer": 0, "byteOffset": 4, "byteLength": 12}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    assert!(
        format!("{err}").contains("SparseIndicesBufferViewTarget"),
        "got: {err}"
    );
}

#[test]
fn rejects_sparse_indices_bufferview_with_stride() {
    let bin = vec![0u8; 64];
    let doc = build_doc(
        &bin,
        r#"{"componentType": 5126, "count": 4, "type": "VEC3",
            "sparse": {
                "count": 1,
                "indices": { "bufferView": 0, "componentType": 5121 },
                "values":  { "bufferView": 1 }
            }}"#,
        // bv[0] = indices buffer view (with bad byteStride = 4)
        // bv[1] = values buffer view
        r#"{"buffer": 0, "byteOffset": 0, "byteLength": 4, "byteStride": 4},
           {"buffer": 0, "byteOffset": 4, "byteLength": 12}"#,
        r#"{"POSITION": 0}"#,
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    assert!(
        format!("{err}").contains("SparseIndicesBufferViewStride"),
        "got: {err}"
    );
}
