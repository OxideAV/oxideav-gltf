//! Morph targets per glTF 2.0 §3.7.2.2.
//!
//! Each `mesh.primitives[i].targets[t]` is an attribute → accessor map
//! whose elements are vertex deltas (POSITION / NORMAL / TANGENT only,
//! VEC3 FLOAT) added to the base attribute weighted by `mesh.weights`
//! (or `node.weights`) per the formula:
//!
//! ```text
//! mesh.primitives[i].attribute =
//!   primitives[i].attribute
//!     + sum_t weight[t] * primitives[i].targets[t].attribute
//! ```
//!
//! `oxideav_mesh3d::Primitive` doesn't carry a typed `targets` field
//! (cross-crate change deferred to r5), so this crate stashes them on
//! `primitive.extras["__morph_targets"]` (and `mesh.weights` on
//! `primitive[0].extras["__mesh_weights"]`) — same sentinel pattern as
//! `__mesh_extras`. Tests round-trip a hand-crafted JSON document and
//! verify the deltas come back through the encoder bit-equal.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};
use serde_json::json;

fn build_morph_doc(targets_json: &str, mesh_weights: Option<&str>) -> Vec<u8> {
    // Build a binary buffer with:
    //   bv0 = 3 base positions (9 floats = 36 bytes)
    //   then one VEC3 FLOAT array per accessor referenced by the
    //   targets JSON. We take the simpler route here and hard-code 3
    //   accessor blobs (POSITION_DELTA × however many targets) at
    //   known byte offsets — caller of this helper builds tests with
    //   the relevant target shapes.
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Three "delta" blobs, each 36 bytes = 3 vec3 floats.
    // Target 0 POSITION: (0.1, 0, 0) per vertex
    let off1 = bin.len();
    for _ in 0..3 {
        for &c in &[0.1f32, 0.0, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Target 1 POSITION: (0, 0.2, 0) per vertex
    let off2 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 0.2, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Target 2 POSITION: (0, 0, 0.3) per vertex
    let off3 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 0.0, 0.3] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Target 3 NORMAL_0 delta: (0, 1, 0) per vertex
    let off4 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 1.0, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let mw = mesh_weights.unwrap_or("");
    let mw_field = if mw.is_empty() {
        String::new()
    } else {
        format!(", \"weights\": {mw}")
    };
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off1}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off2}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off3}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off4}, "byteLength": 36 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.1, 0.0, 0.0], "max": [0.1, 0.0, 0.0] }},
            {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.2, 0.0], "max": [0.0, 0.2, 0.0] }},
            {{ "bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.3], "max": [0.0, 0.0, 0.3] }},
            {{ "bufferView": 4, "componentType": 5126, "count": 3, "type": "VEC3" }},
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{
                        "attributes": {{ "POSITION": 0, "NORMAL": 5 }},
                        "targets": {targets_json}
                    }}
                ]
                {mw_field}
            }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ],
        "scene": 0
    }}"#
    );
    json.into_bytes()
}

#[test]
fn one_target_position_round_trip() {
    // Single morph target: POSITION_0 -> accessor 1 (0.1,0,0 deltas).
    let bytes = build_morph_doc(r#"[ { "POSITION": 1 } ]"#, None);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).unwrap();

    let mt = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .expect("__morph_targets sentinel");
    let arr = mt.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    let pos = arr[0].as_object().unwrap().get("POSITION").unwrap();
    let elems = pos.as_array().unwrap();
    assert_eq!(elems.len(), 3);
    let first = elems[0].as_array().unwrap();
    assert!((first[0].as_f64().unwrap() - 0.1).abs() < 1e-6);

    // Re-encode → decode and verify the deltas survive the round trip.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let scene2 = dec.decode(&glb).unwrap();
    let mt2 = scene2.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .expect("__morph_targets sentinel after re-encode");
    assert_eq!(mt, mt2);
}

#[test]
fn four_targets_with_mesh_weights() {
    // 4 morph targets with default mesh.weights = [0.0, 0.5, 0.0, 0.25].
    let targets = r#"[
        { "POSITION": 1 },
        { "POSITION": 2 },
        { "POSITION": 3 },
        { "POSITION": 1 }
    ]"#;
    let bytes = build_morph_doc(targets, Some("[0.0, 0.5, 0.0, 0.25]"));
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).unwrap();

    let mw = scene.meshes[0].primitives[0]
        .extras
        .get("__mesh_weights")
        .expect("__mesh_weights sentinel");
    let weights: Vec<f64> = mw
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(weights, vec![0.0, 0.5, 0.0, 0.25]);

    let mt = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(mt.len(), 4);

    // Re-encode to .glb and pull the JSON chunk to verify
    // mesh.weights + primitive.targets are emitted at the right paths.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let mesh = &v["meshes"][0];
    assert_eq!(mesh["weights"], json!([0.0, 0.5, 0.0, 0.25]));
    let prim = &mesh["primitives"][0];
    let targets_out = prim["targets"].as_array().unwrap();
    assert_eq!(targets_out.len(), 4);
    for t in targets_out {
        assert!(t.as_object().unwrap().contains_key("POSITION"));
    }
}

#[test]
fn mixed_position_and_normal_target() {
    // One target with both POSITION_0 and NORMAL_0 attributes.
    let targets = r#"[ { "POSITION": 1, "NORMAL": 4 } ]"#;
    let bytes = build_morph_doc(targets, None);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).unwrap();
    let mt = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(mt.len(), 1);
    let obj = mt[0].as_object().unwrap();
    assert!(obj.contains_key("POSITION"));
    assert!(obj.contains_key("NORMAL"));

    // Round-trip through encoder.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let scene2 = dec.decode(&glb).unwrap();
    let mt2 = scene2.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(mt2.len(), 1);
    let obj2 = mt2[0].as_object().unwrap();
    // Both keys present after round-trip.
    assert!(obj2.contains_key("POSITION"));
    assert!(obj2.contains_key("NORMAL"));
    // POSITION delta first vertex was (0.1, 0, 0).
    let pos = obj2["POSITION"].as_array().unwrap();
    let p0 = pos[0].as_array().unwrap();
    assert!((p0[0].as_f64().unwrap() - 0.1).abs() < 1e-6);
    // NORMAL delta first vertex was (0, 1, 0).
    let nrm = obj2["NORMAL"].as_array().unwrap();
    let n0 = nrm[0].as_array().unwrap();
    assert!((n0[1].as_f64().unwrap() - 1.0).abs() < 1e-6);
}
