//! Indexed attribute set-index validation per glTF 2.0 §3.7.2.1:
//! TEXCOORD_n / COLOR_n / JOINTS_n / WEIGHTS_n set indices "MUST start
//! with 0 and be consecutive positive integers" and MUST NOT use leading
//! zeroes. A gap, a non-zero start, or a malformed/leading-zero suffix
//! surfaces as `AttributeSetIndex`.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a minimal single-triangle mesh whose `attributes` map is the
/// caller-supplied JSON. Three accessors are provided over a shared
/// buffer: accessor 0 = POSITION VEC3 (3 verts), accessors 1..=N are
/// VEC2 float TEXCOORD-shaped accessors the attributes map can point at.
fn build_doc(attributes_json: &str) -> Vec<u8> {
    // 3 VEC3 positions (36 bytes) + 3 VEC2 (24 bytes) shared by every
    // extra attribute accessor.
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    for uv in [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]] {
        for c in uv {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let uv_len = bin.len() - pos_len;
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);

    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {pos_len} }},
            {{ "buffer": 0, "byteOffset": {pos_len}, "byteLength": {uv_len} }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0, 0, 0], "max": [1, 1, 0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2" }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2" }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "mode": 4,
            "attributes": {attributes_json}
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

#[test]
fn consecutive_texcoord_sets_accepted() {
    let doc = build_doc(r#"{ "POSITION": 0, "TEXCOORD_0": 1, "TEXCOORD_1": 2 }"#);
    GltfDecoder::new()
        .decode(&doc)
        .expect("TEXCOORD_0 + TEXCOORD_1 must decode");
}

#[test]
fn single_texcoord_zero_accepted() {
    let doc = build_doc(r#"{ "POSITION": 0, "TEXCOORD_0": 1 }"#);
    GltfDecoder::new()
        .decode(&doc)
        .expect("a lone TEXCOORD_0 must decode");
}

#[test]
fn texcoord_gap_rejected() {
    // TEXCOORD_0 + TEXCOORD_2 with no TEXCOORD_1 is a gap.
    let doc = build_doc(r#"{ "POSITION": 0, "TEXCOORD_0": 1, "TEXCOORD_2": 2 }"#);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("a TEXCOORD set gap must be rejected");
    assert!(format!("{err}").contains("AttributeSetIndex"), "got: {err}");
}

#[test]
fn texcoord_nonzero_start_rejected() {
    // TEXCOORD_1 with no TEXCOORD_0 — set indices must start at 0.
    let doc = build_doc(r#"{ "POSITION": 0, "TEXCOORD_1": 1 }"#);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("a non-zero-start TEXCOORD set must be rejected");
    assert!(format!("{err}").contains("AttributeSetIndex"), "got: {err}");
}

#[test]
fn texcoord_leading_zero_rejected() {
    // TEXCOORD_01 uses a leading zero.
    let doc = build_doc(r#"{ "POSITION": 0, "TEXCOORD_0": 1, "TEXCOORD_01": 2 }"#);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("a leading-zero TEXCOORD set index must be rejected");
    assert!(format!("{err}").contains("AttributeSetIndex"), "got: {err}");
}
