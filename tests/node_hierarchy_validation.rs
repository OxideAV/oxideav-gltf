//! End-to-end node-hierarchy + transform validation per glTF 2.0
//! §3.5.2 (node hierarchy) + §3.5.3 (transformations) (round r300).
//!
//! Each test wires a malformed `nodes[]` graph or transform through the
//! public `GltfDecoder` API and confirms the spec-prefixed
//! `Error::InvalidData` surfaces. The unit tests in `src/validation.rs`
//! exercise the per-rule logic directly; the tests here pin the wiring
//! inside `convert()` so a future refactor cannot drop the call, and
//! confirm well-formed graphs round-trip unchanged.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a minimal document with the supplied `nodes[]` / `animations[]`
/// JSON fragments. The scene references node 0 so conversion runs.
fn doc(nodes_json: &str, animations_json: &str) -> Vec<u8> {
    let anim = if animations_json.is_empty() {
        String::new()
    } else {
        format!(r#", "animations": [ {animations_json} ]"#)
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "nodes": [ {nodes_json} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0{anim}
    }}"#
    )
    .into_bytes()
}

fn decode_ok(nodes_json: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc(nodes_json, ""))
        .unwrap_or_else(|e| panic!("expected valid node graph, got: {e}"));
}

fn decode_err(nodes_json: &str, animations_json: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc(nodes_json, animations_json))
        .expect_err("malformed node document should have been rejected");
    format!("{err}")
}

// ----------------------------------------------------------------------
// §3.5.2 hierarchy
// ----------------------------------------------------------------------

#[test]
fn valid_strict_tree_passes() {
    // 0 -> [1, 2], 1 -> [3]; a disjoint strict tree.
    decode_ok(r#"{ "children": [1, 2] }, { "children": [3] }, {}, {}"#);
}

#[test]
fn valid_disjoint_forest_passes() {
    // Two separate roots, each with a child — still "a set of disjoint
    // strict trees".
    decode_ok(r#"{ "children": [1] }, {}, { "children": [3] }, {}"#);
}

#[test]
fn rejects_child_index_out_of_range() {
    // §3.5.2 — children must resolve into nodes[].
    let msg = decode_err(r#"{ "children": [5] }, {}"#, "");
    assert!(msg.contains("NodeChildIndex"), "{msg}");
}

#[test]
fn rejects_node_with_two_parents() {
    // §3.5.2 — node 2 claimed by both node 0 and node 1.
    let msg = decode_err(r#"{ "children": [2] }, { "children": [2] }, {}"#, "");
    assert!(msg.contains("NodeMultipleParents"), "{msg}");
}

#[test]
fn rejects_self_child_cycle() {
    // §3.5.2 — a node listing itself as a child makes it its own parent.
    let msg = decode_err(r#"{ "children": [0] }"#, "");
    assert!(msg.contains("NodeHierarchyCycle"), "{msg}");
}

#[test]
fn rejects_two_node_cycle() {
    // §3.5.2 — 0 -> 1 -> 0.
    let msg = decode_err(r#"{ "children": [1] }, { "children": [0] }"#, "");
    // The second back-edge is caught as a multiple-parent before the
    // cycle walk on most layouts; either spec-prefix is a valid reject.
    assert!(
        msg.contains("NodeHierarchyCycle") || msg.contains("NodeMultipleParents"),
        "{msg}"
    );
}

#[test]
fn rejects_three_node_cycle() {
    // §3.5.2 — 0 -> 1 -> 2 -> 0; the closing edge gives node 0 a second
    // parent claim, surfaced as a hierarchy violation.
    let msg = decode_err(
        r#"{ "children": [1] }, { "children": [2] }, { "children": [0] }"#,
        "",
    );
    assert!(
        msg.contains("NodeHierarchyCycle") || msg.contains("NodeMultipleParents"),
        "{msg}"
    );
}

// ----------------------------------------------------------------------
// §3.5.3 transforms
// ----------------------------------------------------------------------

#[test]
fn valid_trs_node_passes() {
    decode_ok(r#"{ "translation": [1, 2, 3], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] }"#);
}

#[test]
fn valid_identity_matrix_node_passes() {
    decode_ok(r#"{ "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1] }"#);
}

#[test]
fn rejects_matrix_and_trs_combined() {
    // §3.5.3 — matrix is mutually exclusive with TRS.
    let msg = decode_err(
        r#"{ "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1], "translation": [1,0,0] }"#,
        "",
    );
    assert!(msg.contains("NodeMatrixTRSExclusive"), "{msg}");
}

#[test]
fn rejects_animated_node_with_matrix() {
    // §3.5.3 — a node referenced by an animation channel MUST NOT carry
    // a matrix. The animation channel targets node 0's translation; the
    // sampler indices need not resolve because the node check runs
    // first only after validate_animation_channels — so we point the
    // sampler at a real (degenerate) sampler to clear that gate.
    let nodes = r#"{ "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1] }"#;
    // animation channel/sampler need to validate first; give it a
    // resolvable sampler whose accessors resolve (none here → but
    // validate_animation_channels only resolves indices when present).
    let anim = r#"{
        "channels": [ { "sampler": 0, "target": { "node": 0, "path": "translation" } } ],
        "samplers": [ { "input": 0, "output": 1 } ]
    }"#;
    // Provide the two accessors the sampler references so the animation
    // validator passes and the node check is reached.
    let mut dec = GltfDecoder::new();
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "nodes": [ {nodes} ],
        "accessors": [
            {{ "componentType": 5126, "count": 1, "type": "SCALAR" }},
            {{ "componentType": 5126, "count": 1, "type": "VEC3" }}
        ],
        "animations": [ {anim} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    );
    let err = dec
        .decode(doc.as_bytes())
        .expect_err("animated matrix node should be rejected");
    assert!(format!("{err}").contains("NodeAnimatedMatrix"), "{err}");
}

#[test]
fn rejects_non_unit_quaternion() {
    // §3.5.3 — rotation MUST be a unit quaternion.
    let msg = decode_err(r#"{ "rotation": [0, 0, 0, 2] }"#, "");
    assert!(msg.contains("NodeRotationUnitQuaternion"), "{msg}");
}

#[test]
fn accepts_near_unit_quaternion() {
    // A quaternion slightly off unit length (within the round-trip
    // tolerance) is accepted.
    decode_ok(r#"{ "rotation": [0.0, 0.7071, 0.0, 0.7071] }"#);
}

#[test]
fn rejects_singular_matrix() {
    // §3.5.3 — a matrix MUST be decomposable to TRS; a zero-scale
    // column gives a zero determinant, so it is not decomposable.
    let msg = decode_err(r#"{ "matrix": [0,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1] }"#, "");
    assert!(msg.contains("NodeMatrixDecompose"), "{msg}");
}

#[test]
fn round_trip_after_node_validation() {
    // A valid mixed graph survives decode → its node count is intact.
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(&doc(
            r#"{ "children": [1], "translation": [0, 1, 0] },
               { "rotation": [0, 0, 0, 1], "scale": [2, 2, 2] }"#,
            "",
        ))
        .unwrap();
    assert_eq!(scene.nodes.len(), 2);
}
