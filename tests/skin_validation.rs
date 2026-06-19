//! End-to-end skin-roster validation per glTF 2.0 §5.28 (Skin),
//! §3.7.3 (Skins) and §5.25.3 (node.skin) (round r346).
//!
//! Each test wires a malformed (or well-formed) `skins[]` / `nodes[]`
//! roster through the public `GltfDecoder` API and confirms the
//! spec-prefixed `Error::InvalidData` surfaces (or the document
//! round-trips). The unit tests in `src/validation.rs::tests` exercise
//! the per-rule logic directly; the tests here pin the wiring inside
//! `convert()` so a future refactor cannot drop the `validate_skins`
//! call.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// 2 identity MAT4 float matrices (128 bytes) as a base64 data URI
/// buffer, suitable as the backing store for an inverseBindMatrices
/// accessor of count 2.
const IBM_BUFFER_B64: &str = "AACAPwAAAAAAAAAAAAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAAAAAAAAAgD8AAAAAAAAAAAAAAAAAAAAAAACAPwAAgD8AAAAAAAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAAAAAAAAAgD8=";

/// Build a document from explicit `skins`, `nodes`, optional
/// `accessors`/`bufferViews`/`buffers`, and a scene listing `scene_roots`.
fn doc(skins: &str, nodes: &str, extra: &str, scene_roots: &str) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "skins": [ {skins} ],
        "nodes": [ {nodes} ],
        "scenes": [ {{ "nodes": [{scene_roots}] }} ], "scene": 0{extra}
    }}"#
    )
    .into_bytes()
}

fn decode_err(skins: &str, nodes: &str, extra: &str, scene_roots: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc(skins, nodes, extra, scene_roots))
        .expect_err("malformed skin document should have been rejected");
    format!("{err}")
}

fn decode_ok(skins: &str, nodes: &str, extra: &str, scene_roots: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc(skins, nodes, extra, scene_roots))
        .unwrap_or_else(|e| panic!("expected valid skin document, got: {e}"));
}

// ----------------------------------------------------------------------
// §5.28.3 — joints
// ----------------------------------------------------------------------

#[test]
fn joints_empty_rejected() {
    // A skin with an empty joints array. `joints` is `integer [1-*]`.
    let err = decode_err(r#"{ "joints": [] }"#, r#"{}"#, "", "0");
    assert!(err.contains("SkinJointsEmpty"), "got: {err}");
}

#[test]
fn joint_index_out_of_range_rejected() {
    // Joint 5 does not exist (only nodes 0,1 present).
    let err = decode_err(r#"{ "joints": [0, 5] }"#, r#"{}, {}"#, "", "0");
    assert!(err.contains("SkinJointIndex"), "got: {err}");
}

#[test]
fn duplicate_joint_rejected() {
    let err = decode_err(
        r#"{ "joints": [0, 1, 0] }"#,
        r#"{ "children": [1] }, {}"#,
        "",
        "0",
    );
    assert!(err.contains("SkinJointDuplicate"), "got: {err}");
}

#[test]
fn unique_joints_in_range_pass() {
    decode_ok(
        r#"{ "joints": [0, 1] }"#,
        r#"{ "children": [1] }, {}"#,
        "",
        "0",
    );
}

// ----------------------------------------------------------------------
// §5.28.2 — skeleton
// ----------------------------------------------------------------------

#[test]
fn skeleton_index_out_of_range_rejected() {
    let err = decode_err(r#"{ "joints": [0], "skeleton": 9 }"#, r#"{}"#, "", "0");
    assert!(err.contains("SkinSkeletonIndex"), "got: {err}");
}

#[test]
fn skeleton_in_range_passes() {
    // node 0 -> [1, 2]; joints [1, 2]; skeleton 0 is a valid node index.
    decode_ok(
        r#"{ "joints": [1, 2], "skeleton": 0 }"#,
        r#"{ "children": [1, 2] }, {}, {}"#,
        "",
        "0",
    );
}

// ----------------------------------------------------------------------
// §5.28.1 / §3.7.3.1 — inverseBindMatrices accessor
// ----------------------------------------------------------------------

/// Document fragment defining one MAT4/FLOAT accessor of `count` over the
/// 128-byte IBM data buffer, plus a bufferView covering the whole buffer.
fn ibm_extra(kind: &str, component_type: u32, count: u32, normalized: bool) -> String {
    format!(
        r#", "accessors": [ {{ "bufferView": 0, "componentType": {component_type}, "count": {count}, "type": "{kind}", "normalized": {normalized} }} ],
        "bufferViews": [ {{ "buffer": 0, "byteLength": 128 }} ],
        "buffers": [ {{ "byteLength": 128, "uri": "data:application/octet-stream;base64,{IBM_BUFFER_B64}" }} ]"#
    )
}

#[test]
fn ibm_index_out_of_range_rejected() {
    // No accessors declared; inverseBindMatrices points at accessor 0.
    let err = decode_err(
        r#"{ "joints": [0], "inverseBindMatrices": 0 }"#,
        r#"{}"#,
        "",
        "0",
    );
    assert!(err.contains("SkinIbmIndex"), "got: {err}");
}

#[test]
fn ibm_wrong_type_rejected() {
    // VEC4 instead of MAT4.
    let extra = ibm_extra("VEC4", 5126, 2, false);
    let err = decode_err(
        r#"{ "joints": [0, 1], "inverseBindMatrices": 0 }"#,
        r#"{ "children": [1] }, {}"#,
        &extra,
        "0",
    );
    assert!(err.contains("SkinIbmAccessorType"), "got: {err}");
}

#[test]
fn ibm_wrong_component_type_rejected() {
    // MAT4 but UNSIGNED_SHORT components instead of FLOAT.
    let extra = ibm_extra("MAT4", 5123, 2, false);
    let err = decode_err(
        r#"{ "joints": [0, 1], "inverseBindMatrices": 0 }"#,
        r#"{ "children": [1] }, {}"#,
        &extra,
        "0",
    );
    assert!(err.contains("SkinIbmAccessorComponentType"), "got: {err}");
}

// Note: a `normalized` IBM accessor is unreachable through this
// end-to-end path because IBM accessors MUST be FLOAT and a
// FLOAT+normalized accessor is already rejected by `validate_accessors`
// (§5.1.6) before `validate_skins` runs. The defence-in-depth
// `SkinIbmAccessorNormalized` branch is exercised by the unit test in
// `src/validation.rs::tests::skin_ibm_normalized_rejected`.

#[test]
fn ibm_count_too_small_rejected() {
    // 3 joints but the accessor only has 2 IBM matrices.
    let extra = ibm_extra("MAT4", 5126, 2, false);
    let err = decode_err(
        r#"{ "joints": [0, 1, 2], "inverseBindMatrices": 0 }"#,
        r#"{ "children": [1] }, { "children": [2] }, {}"#,
        &extra,
        "0",
    );
    assert!(err.contains("SkinIbmCount"), "got: {err}");
}

#[test]
fn ibm_well_formed_passes() {
    let extra = ibm_extra("MAT4", 5126, 2, false);
    decode_ok(
        r#"{ "joints": [0, 1], "inverseBindMatrices": 0 }"#,
        r#"{ "children": [1] }, {}"#,
        &extra,
        "0",
    );
}

// ----------------------------------------------------------------------
// §3.7.3.2 — joints as disjoint scene roots are accepted
// ----------------------------------------------------------------------

#[test]
fn joints_as_disjoint_scene_roots_pass() {
    // node 0 and node 1 are two disjoint roots, both in scenes[0]. The
    // spec allows the common root to be a node that "may or may not be a
    // joint node itself", and this crate's encoder emits joints that are
    // distinct scene roots — the scene is their implicit common root. No
    // document-node common ancestor is required.
    decode_ok(r#"{ "joints": [0, 1] }"#, r#"{}, {}"#, "", "0, 1");
}

// ----------------------------------------------------------------------
// §5.25.3 — node.skin coupling
// ----------------------------------------------------------------------

#[test]
fn node_skin_index_out_of_range_rejected() {
    // node 0 references skin 3 but only skin 0 exists. node 0 must have
    // a mesh for the coupling rule, but the index check fires first.
    let err = decode_err(
        r#"{ "joints": [1] }"#,
        r#"{ "skin": 3, "mesh": 0 }, {}"#,
        r#", "meshes": [ { "primitives": [ { "attributes": {} } ] } ]"#,
        "0, 1",
    );
    assert!(err.contains("NodeSkinIndex"), "got: {err}");
}

#[test]
fn node_skin_without_mesh_rejected() {
    // node 0 has skin but no mesh.
    let err = decode_err(r#"{ "joints": [1] }"#, r#"{ "skin": 0 }, {}"#, "", "0, 1");
    assert!(err.contains("NodeSkinWithoutMesh"), "got: {err}");
}

// ----------------------------------------------------------------------
// §3.7.3.2 — joints must belong to the same scene
// ----------------------------------------------------------------------

#[test]
fn skin_joint_in_other_scene_rejected() {
    // Two scenes. scenes[0] roots node 0 (skinned mesh node). The skin's
    // joint is node 2, which only lives under scenes[1]'s root. The
    // skinned node is in scene 0 but its joint is not → violation.
    let mut dec = GltfDecoder::new();
    let body = r#"{
        "asset": { "version": "2.0" },
        "skins": [ { "joints": [2] } ],
        "nodes": [
            { "skin": 0, "mesh": 0 },
            { "children": [2] },
            {}
        ],
        "meshes": [ { "primitives": [ { "attributes": {} } ] } ],
        "scenes": [ { "nodes": [0] }, { "nodes": [1] } ], "scene": 0
    }"#;
    let err = dec
        .decode(body.as_bytes())
        .expect_err("joint in a different scene should be rejected");
    let s = format!("{err}");
    assert!(s.contains("SkinJointWrongScene"), "got: {s}");
}

/// A buffer of 3 VEC3 float POSITION vertices (36 bytes) for a minimal
/// renderable triangle, base64-encoded.
const POSITION_BUFFER_B64: &str = "AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA";

#[test]
fn skin_joints_same_scene_passes() {
    // Skinned node 0 (bound to a real one-triangle mesh) and its joint
    // node 1 both live under scenes[0].
    let extra = format!(
        r#", "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
        "accessors": [ {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }} ],
        "bufferViews": [ {{ "buffer": 0, "byteLength": 36 }} ],
        "buffers": [ {{ "byteLength": 36, "uri": "data:application/octet-stream;base64,{POSITION_BUFFER_B64}" }} ]"#
    );
    decode_ok(
        r#"{ "joints": [1] }"#,
        r#"{ "skin": 0, "mesh": 0 }, {}"#,
        &extra,
        "0, 1",
    );
}
