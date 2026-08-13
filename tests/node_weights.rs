//! `node.weights` per glTF 2.0 §5.25.9 — the per-instance
//! morph-weight override of the referenced mesh's default
//! `mesh.weights`:
//!
//! * "The weights of the instantiated morph target. The number of
//!   array elements MUST match the number of morph targets of the
//!   referenced mesh. When defined, `mesh` MUST also be defined."
//! * Schema A.29 pins `minItems: 1`.
//! * §3.7.2.2 runtime rule: "When an instantiated mesh has morph
//!   targets, it MUST use morph weights specified with the
//!   node.weights property. When the latter is undefined, mesh.weights
//!   property MUST be used instead."
//!
//! The published `oxideav_mesh3d::Node` does not yet carry a typed
//! `weights` field, so the decoder parks the vector on
//! `Node::extras["__node_weights"]` and the encoder lifts it back
//! into the JSON `node.weights` property, keeping the override intact
//! across a round trip.

use base64::Engine as _;
use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};
use serde_json::json;

/// One mesh, one morph target (POSITION deltas), instantiated by a
/// node carrying the given `weights` JSON fragment (empty string →
/// no `weights` property), with optional `mesh.weights` and an
/// optional meshless second node carrying weights.
fn build_doc(node_weights: &str, mesh_weights: &str) -> Vec<u8> {
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let off1 = bin.len();
    for _ in 0..3 {
        for &c in &[0.1f32, 0.0, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let nw = if node_weights.is_empty() {
        String::new()
    } else {
        format!(", \"weights\": {node_weights}")
    };
    let mw = if mesh_weights.is_empty() {
        String::new()
    } else {
        format!(", \"weights\": {mesh_weights}")
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off1}, "byteLength": 36 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.1, 0.0, 0.0], "max": [0.1, 0.0, 0.0] }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{ "attributes": {{ "POSITION": 0 }}, "targets": [ {{ "POSITION": 1 }} ] }}
                ]
                {mw}
            }}
        ],
        "nodes": [ {{ "mesh": 0 {nw} }} ],
        "scenes": [ {{ "nodes": [0] }} ],
        "scene": 0
    }}"#
    )
    .into_bytes()
}

#[test]
fn node_weights_round_trip() {
    let bytes = build_doc("[0.75]", "");
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).expect("decode node.weights");

    // Decoder parks the override on the node's extras sidecar.
    let node = &scene.nodes[0];
    let nw = node
        .extras
        .get("__node_weights")
        .expect("__node_weights sidecar");
    assert_eq!(nw, &json!([0.75]));

    // Round-trip: the encoder lifts the sidecar back into the JSON
    // `node.weights` property (not a surplus `extras` key).
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode");
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    assert_eq!(v["nodes"][0]["weights"], json!([0.75]));
    assert!(
        v["nodes"][0].get("extras").is_none(),
        "sidecar must not leak into JSON extras"
    );

    // And it survives a second decode.
    let scene2 = dec.decode(&glb).expect("re-decode");
    assert_eq!(
        scene2.nodes[0].extras.get("__node_weights"),
        Some(&json!([0.75]))
    );
}

#[test]
fn node_weights_and_mesh_weights_coexist() {
    // §3.7.2.2: node.weights overrides mesh.weights at runtime — both
    // are modelled and both round-trip.
    let bytes = build_doc("[0.25]", "[0.5]");
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).expect("decode");
    assert_eq!(scene.meshes[0].weights, vec![0.5]);
    assert_eq!(
        scene.nodes[0].extras.get("__node_weights"),
        Some(&json!([0.25]))
    );

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode");
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    assert_eq!(v["meshes"][0]["weights"], json!([0.5]));
    assert_eq!(v["nodes"][0]["weights"], json!([0.25]));
}

#[test]
fn node_weights_length_mismatch_rejected() {
    // 1 morph target but 2 node weights → §5.25.9 length MUST.
    let bytes = build_doc("[0.75, 0.25]", "");
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&bytes).expect_err("length mismatch must fail");
    assert!(
        format!("{err}").contains("NodeWeightsLength"),
        "expected NodeWeightsLength, got: {err}"
    );
}

#[test]
fn node_weights_empty_rejected() {
    // Schema A.29 `minItems: 1` — a declared-but-empty array is
    // invalid.
    let bytes = build_doc("[]", "");
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&bytes).expect_err("empty weights must fail");
    assert!(
        format!("{err}").contains("NodeWeightsEmpty"),
        "expected NodeWeightsEmpty, got: {err}"
    );
}

#[test]
fn node_weights_without_mesh_rejected() {
    // §5.25.9: "When defined, mesh MUST also be defined." Hand-build a
    // document whose weights-carrying node has no mesh.
    let doc = r#"{
        "asset": { "version": "2.0" },
        "nodes": [ { "weights": [0.5] } ],
        "scenes": [ { "nodes": [0] } ],
        "scene": 0
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(doc.as_bytes())
        .expect_err("weights without mesh must fail");
    assert!(
        format!("{err}").contains("NodeWeightsWithoutMesh"),
        "expected NodeWeightsWithoutMesh, got: {err}"
    );
}

#[test]
fn mesh_weights_empty_rejected() {
    // Schema A.26 `minItems: 1` on `mesh.weights` — the sibling rule.
    let bytes = build_doc("", "[]");
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&bytes)
        .expect_err("empty mesh.weights must fail");
    assert!(
        format!("{err}").contains("MeshWeightsEmpty"),
        "expected MeshWeightsEmpty, got: {err}"
    );
}
