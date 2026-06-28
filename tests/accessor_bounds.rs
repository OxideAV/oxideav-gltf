//! Accessor `min` / `max` bounds per glTF 2.0 §3.6.2.1.5.
//!
//! Spec rules surfaced here:
//!
//! - POSITION accessors **MUST** declare `min` and `max` (§3.7.2.1).
//! - When ANY accessor declares `min` / `max`, the values MUST equal
//!   the component-wise extrema of the stored data (§3.6.2.1.5).
//! - Animation-input accessors MUST declare `min` / `max` (§3.11) —
//!   already enforced by the encoder for the keyframe times path.
//!
//! The encoder fills POSITION min/max from the data unconditionally
//! (`with_minmax = true` on the POSITION attribute path); the decoder
//! validates declared bounds when present and surfaces a mismatch via
//! an `AccessorBoundsMismatch`-prefixed `Error::InvalidData`. (The
//! shared `oxideav_core::Error` enum can't gain a new variant from a
//! sibling crate; the prefix lets callers grep for the condition.)

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology};

fn scene_with_known_position_extents() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    // Known extents: x ∈ [-2, 5], y ∈ [-1, 4], z ∈ [0, 3].
    // Six vertices (two triangles) so the indexless TRIANGLES primitive
    // satisfies the §3.7.2.1 "divisible by 3" vertex-count rule; the two
    // extra interior points don't change the component-wise extrema.
    prim.positions = vec![
        [-2.0, 4.0, 0.0],
        [5.0, -1.0, 3.0],
        [0.0, 0.0, 1.5],
        [3.0, 2.0, 0.5],
        [1.0, 1.0, 1.0],
        [2.0, 3.0, 2.0],
    ];
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    scene
}

#[test]
fn encoder_fills_position_min_max() {
    let scene = scene_with_known_position_extents();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    // Pull JSON chunk and inspect the POSITION accessor.
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let acc0 = &v["accessors"][0];
    assert_eq!(acc0["type"], "VEC3");
    let mn = acc0["min"].as_array().unwrap();
    let mx = acc0["max"].as_array().unwrap();
    assert!((mn[0].as_f64().unwrap() - -2.0).abs() < 1e-6);
    assert!((mn[1].as_f64().unwrap() - -1.0).abs() < 1e-6);
    assert!((mn[2].as_f64().unwrap() - 0.0).abs() < 1e-6);
    assert!((mx[0].as_f64().unwrap() - 5.0).abs() < 1e-6);
    assert!((mx[1].as_f64().unwrap() - 4.0).abs() < 1e-6);
    assert!((mx[2].as_f64().unwrap() - 3.0).abs() < 1e-6);
}

#[test]
fn decoder_accepts_correct_bounds() {
    // Standard round-trip — bounds the encoder writes are correct, so
    // the decoder must not reject them.
    let scene = scene_with_known_position_extents();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let scene2 = dec
        .decode(&glb)
        .expect("decoder must accept correct bounds");
    assert_eq!(scene2.meshes[0].primitives[0].positions.len(), 6);
}

#[test]
fn decoder_rejects_mismatched_min() {
    // Hand-craft a JSON document with a POSITION accessor whose
    // declared `min` is wrong (claims [0,0,0] but actual data has
    // negative components).
    use base64::Engine as _;
    let mut bin = Vec::new();
    for v in [[-1.0f32, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, -1.0, 0.5]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    // declared min = [0, 0, 0] but actual min[0] = -1 → mismatch.
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": {total} }} ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0] }}
        ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    );
    let mut dec = GltfDecoder::new();
    let res = dec.decode(json.as_bytes());
    assert!(res.is_err(), "expected mismatch error, got Ok: {res:?}");
    let msg = format!("{}", res.unwrap_err());
    assert!(
        msg.contains("AccessorBoundsMismatch"),
        "expected AccessorBoundsMismatch in error message, got: {msg}"
    );
}

#[test]
fn decoder_rejects_mismatched_max() {
    // declared max claims [10,10,10] but actual is way smaller.
    use base64::Engine as _;
    let mut bin = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 1.0, 1.0], [0.5, 0.5, 0.5]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": {total} }} ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0.0, 0.0, 0.0], "max": [10.0, 10.0, 10.0] }}
        ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    );
    let mut dec = GltfDecoder::new();
    let res = dec.decode(json.as_bytes());
    assert!(res.is_err());
    let msg = format!("{}", res.unwrap_err());
    assert!(msg.contains("AccessorBoundsMismatch"), "got: {msg}");
}

#[test]
fn decoder_accepts_when_bounds_omitted_on_normal() {
    // NORMAL accessor doesn't have min/max declared (spec says
    // optional for non-POSITION attributes); decoder must not error.
    use base64::Engine as _;
    let mut bin = Vec::new();
    // POSITIONs (with declared bounds)
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    // NORMALs (no bounds)
    for v in [[0.0f32, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let nrm_len = bin.len() - pos_len;
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {pos_len} }},
            {{ "buffer": 0, "byteOffset": {pos_len}, "byteLength": {nrm_len} }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }}
        ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }} }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).expect("optional bounds OK");
    assert_eq!(scene.meshes[0].primitives[0].positions.len(), 3);
    assert!(scene.meshes[0].primitives[0].normals.is_some());
}

/// Build a single-triangle mesh whose POSITION (VEC3, correct bounds)
/// is paired with a second attribute — `attr_name` / `attr_type` with
/// `attr_data` floats and caller-supplied (possibly wrong) `min`/`max`
/// JSON literals. Exercises the §3.6.2.1.5 bounds rule on the
/// non-VEC3 arities the check was generalised to cover.
fn doc_with_attr(
    attr_name: &str,
    attr_type: &str,
    attr_data: &[f32],
    attr_min: &str,
    attr_max: &str,
) -> Vec<u8> {
    use base64::Engine as _;
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    for &v in attr_data {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let attr_len = bin.len() - pos_len;
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let arity = match attr_type {
        "VEC2" => 2,
        "VEC4" => 4,
        _ => 3,
    };
    let count = attr_data.len() / arity;
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {pos_len} }},
            {{ "buffer": 0, "byteOffset": {pos_len}, "byteLength": {attr_len} }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
               "min": [0, 0, 0], "max": [1, 1, 0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": {count}, "type": "{attr_type}",
               "min": {attr_min}, "max": {attr_max} }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "mode": 4,
            "attributes": {{ "POSITION": 0, "{attr_name}": 1 }}
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

#[test]
fn texcoord_vec2_correct_bounds_accepted() {
    let data = [0.0, 0.0, 1.0, 0.0, 0.5, 1.0];
    let doc = doc_with_attr("TEXCOORD_0", "VEC2", &data, "[0, 0]", "[1, 1]");
    GltfDecoder::new()
        .decode(&doc)
        .expect("matching VEC2 bounds must decode");
}

#[test]
fn texcoord_vec2_wrong_max_rejected() {
    let data = [0.0, 0.0, 1.0, 0.0, 0.5, 1.0];
    // Declared max [2, 2] disagrees with actual [1, 1].
    let doc = doc_with_attr("TEXCOORD_0", "VEC2", &data, "[0, 0]", "[2, 2]");
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("wrong VEC2 max must be rejected");
    assert!(
        format!("{err}").contains("AccessorBoundsMismatch"),
        "got: {err}"
    );
}

#[test]
fn tangent_vec4_correct_bounds_accepted() {
    let data = [
        1.0, 0.0, 0.0, 1.0, //
        0.0, 1.0, 0.0, 1.0, //
        0.0, 0.0, 1.0, 1.0, //
    ];
    let doc = doc_with_attr("TANGENT", "VEC4", &data, "[0, 0, 0, 1]", "[1, 1, 1, 1]");
    GltfDecoder::new()
        .decode(&doc)
        .expect("matching VEC4 bounds must decode");
}

#[test]
fn tangent_vec4_wrong_min_rejected() {
    let data = [
        1.0, 0.0, 0.0, 1.0, //
        0.0, 1.0, 0.0, 1.0, //
        0.0, 0.0, 1.0, 1.0, //
    ];
    // Declared w-min -1 disagrees with the actual w-min of 1.
    let doc = doc_with_attr("TANGENT", "VEC4", &data, "[0, 0, 0, -1]", "[1, 1, 1, 1]");
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("wrong VEC4 min must be rejected");
    assert!(
        format!("{err}").contains("AccessorBoundsMismatch"),
        "got: {err}"
    );
}
