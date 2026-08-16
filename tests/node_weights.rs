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
//! The decoder fills the typed `oxideav_mesh3d::Node::weights` field
//! (non-empty = override declared), so
//! `Scene3D::effective_morph_weights` resolves the static node > mesh
//! half of the §3.7.4 precedence chain directly on the decoded scene;
//! the encoder emits the typed vector back into the JSON
//! `node.weights` property. The pre-typed
//! `Node::extras["__node_weights"]` sidecar stays accepted as a
//! legacy encoder input (the typed field wins on a collision).

use base64::Engine as _;
use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{AnimationProperty, AnimationValues, Mesh3DDecoder, Mesh3DEncoder, NodeId};
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

    // Decoder fills the typed `Node::weights` field — no sidecar.
    let node = &scene.nodes[0];
    assert_eq!(node.weights, vec![0.75]);
    assert!(
        !node.extras.contains_key("__node_weights"),
        "the typed field replaces the extras sidecar on decode"
    );

    // The static node > mesh §3.7.4 precedence resolves on the typed
    // scene: the node override wins over the (absent) mesh defaults.
    assert_eq!(
        scene.effective_morph_weights(NodeId(0)),
        Some(&[0.75f32][..])
    );

    // Round-trip: the encoder emits the typed vector back into the
    // JSON `node.weights` property (not a surplus `extras` key).
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
    assert_eq!(scene2.nodes[0].weights, vec![0.75]);
}

#[test]
fn node_weights_and_mesh_weights_coexist() {
    // §3.7.2.2: node.weights overrides mesh.weights at runtime — both
    // are modelled and both round-trip.
    let bytes = build_doc("[0.25]", "[0.5]");
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).expect("decode");
    assert_eq!(scene.meshes[0].weights, vec![0.5]);
    assert_eq!(scene.nodes[0].weights, vec![0.25]);
    // §3.7.4 static precedence: the node override beats the mesh
    // default.
    assert_eq!(
        scene.effective_morph_weights(NodeId(0)),
        Some(&[0.25f32][..])
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
fn mesh_default_applies_when_no_node_override() {
    // §3.7.2.2: "When the latter is undefined, mesh.weights property
    // MUST be used instead" — the static chain's mesh rung.
    let bytes = build_doc("", "[0.5]");
    let scene = GltfDecoder::new().decode(&bytes).expect("decode");
    assert!(scene.nodes[0].weights.is_empty(), "no override declared");
    assert_eq!(
        scene.effective_morph_weights(NodeId(0)),
        Some(&[0.5f32][..]),
        "mesh default resolves when the node carries no override"
    );
}

#[test]
fn typed_node_weights_input_encodes() {
    // A hand-authored typed override (`Node::weights` set directly on
    // the decoded scene) must reach the JSON `node.weights` property.
    let bytes = build_doc("", "[0.5]");
    let mut scene = GltfDecoder::new().decode(&bytes).expect("decode");
    scene.nodes[0].weights = vec![0.875];

    let glb = GltfEncoder::new().encode(&scene).expect("encode");
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    assert_eq!(v["nodes"][0]["weights"], json!([0.875]));
}

#[test]
fn legacy_sidecar_input_still_lifts_and_typed_wins() {
    // Pre-typed hand-authored scenes park the vector on
    // `Node::extras["__node_weights"]` — still lifted when the typed
    // field is empty; when both are present the typed field wins and
    // the sidecar key is consumed either way.
    let bytes = build_doc("", "[0.5]");
    let mut scene = GltfDecoder::new().decode(&bytes).expect("decode");

    // Legacy only.
    scene.nodes[0]
        .extras
        .insert("__node_weights".to_owned(), json!([0.125]));
    let glb = GltfEncoder::new().encode(&scene).expect("encode legacy");
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    assert_eq!(v["nodes"][0]["weights"], json!([0.125]));
    assert!(
        v["nodes"][0].get("extras").is_none(),
        "sidecar key consumed, not leaked"
    );

    // Typed + legacy: typed wins.
    scene.nodes[0].weights = vec![0.625];
    let glb = GltfEncoder::new().encode(&scene).expect("encode both");
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    assert_eq!(v["nodes"][0]["weights"], json!([0.625]));
    assert!(v["nodes"][0].get("extras").is_none());
}

#[test]
fn two_nodes_sharing_one_mesh_hold_distinct_overrides() {
    // The typed field is per-instance: two nodes instantiating the
    // same mesh carry independent static blend states (the headline
    // capability of the typed `Node::weights` surface).
    let bytes = build_doc("[0.25]", "[0.5]");
    let mut doc: serde_json::Value = {
        // Decode-re-encode is unnecessary — just widen the JSON doc
        // with a second node referencing the same mesh.
        serde_json::from_slice(&bytes).unwrap()
    };
    doc["nodes"] = json!([
        { "mesh": 0, "weights": [0.25] },
        { "mesh": 0, "weights": [0.9] }
    ]);
    doc["scenes"] = json!([{ "nodes": [0, 1] }]);
    let bytes = serde_json::to_vec(&doc).unwrap();

    let scene = GltfDecoder::new().decode(&bytes).expect("decode");
    assert_eq!(
        scene.effective_morph_weights(NodeId(0)),
        Some(&[0.25f32][..])
    );
    assert_eq!(
        scene.effective_morph_weights(NodeId(1)),
        Some(&[0.9f32][..])
    );

    // Both instances survive the round trip independently.
    let glb = GltfEncoder::new().encode(&scene).expect("encode");
    let scene2 = GltfDecoder::new().decode(&glb).expect("re-decode");
    assert_eq!(scene2.nodes[0].weights, vec![0.25]);
    assert_eq!(scene2.nodes[1].weights, vec![0.9]);
    assert_eq!(scene2.meshes[0].weights, vec![0.5]);
}

#[test]
fn weight_animation_channel_coexists_with_node_override() {
    // §3.7.4 full chain: animation > node > mesh. The container's job
    // is carrying all three rungs intact — the animated MorphWeights
    // channel, the node override, and the mesh default must all
    // survive a round trip on one document.
    let mut doc: serde_json::Value = serde_json::from_slice(&build_doc("[0.25]", "[0.5]")).unwrap();
    // Append keyframe times [0, 1] + per-keyframe weights [0, 1] to
    // the existing BIN (1 morph target → 1 weight per keyframe).
    let b64 = doc["buffers"][0]["uri"]
        .as_str()
        .unwrap()
        .split(',')
        .nth(1)
        .unwrap()
        .to_owned();
    let mut bin = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .unwrap();
    let anim_off = bin.len();
    for v in [0.0f32, 1.0, 0.0, 1.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let total = bin.len();
    doc["buffers"][0] = json!({
        "byteLength": total,
        "uri": format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bin)
        )
    });
    let bvs = doc["bufferViews"].as_array_mut().unwrap();
    bvs.push(json!({ "buffer": 0, "byteOffset": anim_off, "byteLength": 8 }));
    bvs.push(json!({ "buffer": 0, "byteOffset": anim_off + 8, "byteLength": 8 }));
    let accs = doc["accessors"].as_array_mut().unwrap();
    accs.push(json!({
        "bufferView": 2, "componentType": 5126, "count": 2,
        "type": "SCALAR", "min": [0.0], "max": [1.0]
    }));
    accs.push(json!({
        "bufferView": 3, "componentType": 5126, "count": 2, "type": "SCALAR"
    }));
    doc["animations"] = json!([{
        "channels": [ { "sampler": 0, "target": { "node": 0, "path": "weights" } } ],
        "samplers": [ { "input": 2, "output": 3, "interpolation": "LINEAR" } ]
    }]);
    let bytes = serde_json::to_vec(&doc).unwrap();

    let scene = GltfDecoder::new().decode(&bytes).expect("decode");
    assert_eq!(scene.nodes[0].weights, vec![0.25]);
    assert_eq!(scene.meshes[0].weights, vec![0.5]);
    assert_eq!(scene.animations.len(), 1);
    let ch = &scene.animations[0].channels[0];
    assert_eq!(ch.target.property, AnimationProperty::MorphWeights);
    assert_eq!(ch.sampler.values, AnimationValues::Scalar(vec![0.0, 1.0]));

    let glb = GltfEncoder::new().encode(&scene).expect("encode");
    let scene2 = GltfDecoder::new().decode(&glb).expect("re-decode");
    assert_eq!(scene2.nodes[0].weights, vec![0.25]);
    assert_eq!(scene2.meshes[0].weights, vec![0.5]);
    let ch2 = &scene2.animations[0].channels[0];
    assert_eq!(ch2.target.property, AnimationProperty::MorphWeights);
    assert_eq!(ch2.sampler.values, AnimationValues::Scalar(vec![0.0, 1.0]));
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
