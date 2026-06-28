//! Skinned-mesh JOINTS_n / WEIGHTS_n attribute validation per glTF 2.0
//! §3.7.3.3:
//!
//! - both accessors MUST be VEC4 (`SkinningAttributeType`)
//! - JOINTS_n componentType MUST be unsigned byte / unsigned short
//!   (`SkinningJointsComponentType`)
//! - WEIGHTS_n componentType MUST be float, or normalized unsigned byte /
//!   short (`SkinningWeightsComponentType`)
//! - joint weights MUST NOT be negative (`SkinningWeightsNegative`),
//!   decided on the materialised f32 weights.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a one-vertex skinned primitive. POSITION is one VEC3; JOINTS_0
/// and WEIGHTS_0 each one VEC4. The caller controls the JOINTS / WEIGHTS
/// accessor `type` + `componentType` + `normalized` and the raw weight
/// bytes, so each MUST can be exercised in isolation.
#[allow(clippy::too_many_arguments)]
fn build_doc(
    joints_type: &str,
    joints_ct: u32,
    weights_type: &str,
    weights_ct: u32,
    weights_normalized: bool,
    weights_bytes: &[u8],
    joints_bytes: &[u8],
) -> Vec<u8> {
    // POSITION: 1 VEC3 float (12 bytes).
    let mut bin = Vec::new();
    bin.extend_from_slice(&[0u8; 12]);
    let pos_len = bin.len();
    bin.extend_from_slice(joints_bytes);
    let joints_off = pos_len;
    let joints_len = joints_bytes.len();
    let weights_off = bin.len();
    bin.extend_from_slice(weights_bytes);
    let weights_len = weights_bytes.len();
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);

    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {pos_len} }},
            {{ "buffer": 0, "byteOffset": {joints_off}, "byteLength": {joints_len} }},
            {{ "buffer": 0, "byteOffset": {weights_off}, "byteLength": {weights_len} }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
               "min": [0, 0, 0], "max": [0, 0, 0] }},
            {{ "bufferView": 1, "componentType": {joints_ct}, "count": 1, "type": "{joints_type}" }},
            {{ "bufferView": 2, "componentType": {weights_ct}, "count": 1, "type": "{weights_type}",
               "normalized": {weights_normalized} }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "mode": 0,
            "attributes": {{ "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2 }}
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

/// 4 u16 joints (VEC4 UNSIGNED_SHORT) = 8 bytes.
fn joints_u16(j: [u16; 4]) -> Vec<u8> {
    let mut v = Vec::new();
    for x in j {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

/// 4 f32 weights (VEC4 FLOAT) = 16 bytes.
fn weights_f32(w: [f32; 4]) -> Vec<u8> {
    let mut v = Vec::new();
    for x in w {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

#[test]
fn valid_float_weights_accepted() {
    let doc = build_doc(
        "VEC4",
        5123,
        "VEC4",
        5126,
        false,
        &weights_f32([0.5, 0.5, 0.0, 0.0]),
        &joints_u16([0, 1, 0, 0]),
    );
    GltfDecoder::new()
        .decode(&doc)
        .expect("VEC4 USHORT joints + VEC4 FLOAT weights must decode");
}

#[test]
fn joints_wrong_type_rejected() {
    // JOINTS as VEC3 (6 bytes of joint data) — the type MUST is VEC4.
    // Pad to 8 bytes so the following WEIGHTS bufferView stays 4-aligned
    // and the type rule (not the alignment rule) is what fires.
    let mut jb = Vec::new();
    for x in [0u16, 1, 0] {
        jb.extend_from_slice(&x.to_le_bytes());
    }
    jb.extend_from_slice(&[0u8, 0]);
    let doc = build_doc(
        "VEC3",
        5123,
        "VEC4",
        5126,
        false,
        &weights_f32([1.0, 0.0, 0.0, 0.0]),
        &jb,
    );
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("non-VEC4 JOINTS must be rejected");
    assert!(
        format!("{err}").contains("SkinningAttributeType"),
        "got: {err}"
    );
}

#[test]
fn joints_wrong_component_type_rejected() {
    // JOINTS as FLOAT (16 bytes) — componentType MUST be ubyte / ushort.
    let doc = build_doc(
        "VEC4",
        5126,
        "VEC4",
        5126,
        false,
        &weights_f32([1.0, 0.0, 0.0, 0.0]),
        &weights_f32([0.0, 1.0, 0.0, 0.0]),
    );
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("FLOAT JOINTS componentType must be rejected");
    assert!(
        format!("{err}").contains("SkinningJointsComponentType"),
        "got: {err}"
    );
}

#[test]
fn weights_non_normalized_integer_rejected() {
    // WEIGHTS as UNSIGNED_SHORT but normalized=false — MUST be float or
    // NORMALIZED unsigned byte/short.
    let doc = build_doc(
        "VEC4",
        5123,
        "VEC4",
        5123,
        false,
        &joints_u16([1, 0, 0, 0]),
        &joints_u16([0, 1, 0, 0]),
    );
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("non-normalized integer WEIGHTS must be rejected");
    assert!(
        format!("{err}").contains("SkinningWeightsComponentType"),
        "got: {err}"
    );
}

#[test]
fn negative_weight_rejected() {
    // A negative weight component.
    let doc = build_doc(
        "VEC4",
        5123,
        "VEC4",
        5126,
        false,
        &weights_f32([1.5, -0.5, 0.0, 0.0]),
        &joints_u16([0, 1, 0, 0]),
    );
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("a negative joint weight must be rejected");
    assert!(
        format!("{err}").contains("SkinningWeightsNegative"),
        "got: {err}"
    );
}

#[test]
fn normalized_ushort_weights_accepted() {
    // WEIGHTS as normalized UNSIGNED_SHORT — [65535, 0, 0, 0] = [1,0,0,0].
    let mut wb = Vec::new();
    for x in [65535u16, 0, 0, 0] {
        wb.extend_from_slice(&x.to_le_bytes());
    }
    let doc = build_doc(
        "VEC4",
        5123,
        "VEC4",
        5123,
        true,
        &wb,
        &joints_u16([0, 1, 0, 0]),
    );
    GltfDecoder::new()
        .decode(&doc)
        .expect("normalized USHORT weights must decode");
}
