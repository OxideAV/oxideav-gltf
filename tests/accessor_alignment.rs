//! Per-accessor `byteOffset` / `byteStride` component-size alignment
//! validation per glTF 2.0 §3.6.2.4 (spec line 3091):
//!
//! - `accessor.byteOffset` MUST be a multiple of the component size.
//! - `accessor.byteOffset + bufferView.byteOffset` MUST be a multiple of
//!   the component size.
//! - when `bufferView.byteStride` is defined it MUST be a multiple of the
//!   component size.
//!
//! These hold on EVERY accessor with a bufferView — including the
//! NON-vertex accessors (animation sampler input/output, indices,
//! inverseBindMatrices, sparse) that the per-primitive vertex-attribute
//! alignment pass never sees. The accessor-fit pass enforces them with
//! `AccessorByteOffsetAlignment` / `AccessorStrideAlignment`.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A single-triangle indexed mesh whose `indices` accessor is given a
/// caller-controlled `byteOffset`. UNSIGNED_SHORT (component size 2), so
/// an odd byteOffset is misaligned. The buffer is padded generously so
/// the accessor-fit byteLength check still passes and only the alignment
/// rule can fire.
fn indexed_doc(indices_byte_offset: u32) -> Vec<u8> {
    // POSITION: 3 VEC3 floats (36 bytes).
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len() as u32;
    // Indices region: 3 valid u16 indices at offset 0 (so a byteOffset of
    // 0 is 2-aligned AND decodes to [0, 1, 2]); a misaligned byteOffset of
    // 1 is rejected by the alignment rule before any bytes are read, so
    // the trailing pad only has to keep the buffer large enough.
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    // Trailing pad so a byteOffset of 1 still leaves room for 3 u16.
    bin.extend_from_slice(&[0u8; 8]);
    let total = bin.len();
    let idx_bv_len = total as u32 - pos_len;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);

    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {pos_len} }},
            {{ "buffer": 0, "byteOffset": {pos_len}, "byteLength": {idx_bv_len} }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0, 0, 0], "max": [1, 1, 0] }},
            {{ "bufferView": 1, "byteOffset": {indices_byte_offset},
               "componentType": 5123, "count": 3, "type": "SCALAR" }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "mode": 4,
            "attributes": {{ "POSITION": 0 }},
            "indices": 1
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

#[test]
fn aligned_indices_offset_accepted() {
    // byteOffset 0 into the indices bufferView is 2-aligned.
    let doc = indexed_doc(0);
    GltfDecoder::new()
        .decode(&doc)
        .expect("a 2-aligned UNSIGNED_SHORT indices accessor must decode");
}

#[test]
fn misaligned_indices_offset_rejected() {
    // byteOffset 1 into the indices bufferView is odd → not a multiple of
    // the UNSIGNED_SHORT component size (2). The indices accessor is not a
    // vertex attribute, so only the general §3.6.2.4 rule catches it.
    let doc = indexed_doc(1);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("an odd UNSIGNED_SHORT indices byteOffset must be rejected");
    assert!(
        format!("{err}").contains("AccessorByteOffsetAlignment"),
        "got: {err}"
    );
}
