//! `mesh.extras.targetNames` — the de-facto morph-target naming
//! convention recorded in the glTF 2.0 §3.7.2.2 implementation note:
//!
//! > "While the glTF 2.0 specification currently does not provide a
//! > way to specify names, most tools use an array of strings,
//! > `mesh.extras.targetNames`, for this purpose. The `targetNames`
//! > array and all primitive `targets` arrays must have the same
//! > length."
//!
//! The decoder lifts a convention-shaped array (non-empty, all
//! strings) into the typed `oxideav_mesh3d::Mesh::target_names` field
//! and leaves every other `extras` key opaque; the encoder writes the
//! typed names back under the same key. The length rule is policed on
//! both sides (`MeshTargetNamesLength`), and the decoded scene passes
//! mesh3d's own `Scene3D::validate` length rule.

use base64::Engine as _;
use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Mesh, Mesh3DDecoder, Mesh3DEncoder, MorphTarget, Primitive, Scene3D, Topology,
};

/// One mesh with two POSITION-delta morph targets and the given
/// `extras` JSON fragment (empty string → no `extras` property).
fn build_doc(mesh_extras: &str) -> Vec<u8> {
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
    let off2 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 0.2, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let ex = if mesh_extras.is_empty() {
        String::new()
    } else {
        format!(", \"extras\": {mesh_extras}")
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off1}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off2}, "byteLength": 36 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.1, 0.0, 0.0], "max": [0.1, 0.0, 0.0] }},
            {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.2, 0.0], "max": [0.0, 0.2, 0.0] }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{ "attributes": {{ "POSITION": 0 }}, "targets": [ {{ "POSITION": 1 }}, {{ "POSITION": 2 }} ] }}
                ]
                {ex}
            }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ],
        "scene": 0
    }}"#
    )
    .into_bytes()
}

fn glb_json(glb: &[u8]) -> serde_json::Value {
    let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    serde_json::from_slice(&glb[20..20 + n]).unwrap()
}

#[test]
fn target_names_lift_into_typed_field_and_round_trip() {
    let bytes = build_doc(r#"{ "targetNames": ["smile", "frown"] }"#);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).expect("decode targetNames");

    let mesh = &scene.meshes[0];
    assert_eq!(
        mesh.target_names,
        vec!["smile".to_owned(), "frown".to_owned()]
    );
    assert_eq!(mesh.target_name(1), Some("frown"));
    assert_eq!(mesh.find_target("smile"), Some(0));
    // The lifted key does not linger in the opaque extras stash, and
    // the stash itself is gone once it is empty.
    assert!(
        !mesh.primitives[0].extras.contains_key("__mesh_extras"),
        "targetNames was the only extras key — nothing left to stash"
    );
    // mesh3d's own length rule is satisfied on the decoded scene.
    scene.validate().expect("decoded scene validates");

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode");
    let v = glb_json(&glb);
    assert_eq!(
        v["meshes"][0]["extras"]["targetNames"],
        serde_json::json!(["smile", "frown"])
    );
    // Fixed point: the second decode sees the same typed names, and
    // the second encode emits the identical JSON document.
    let scene2 = dec.decode(&glb).expect("re-decode");
    assert_eq!(scene2.meshes[0].target_names, mesh.target_names);
    let glb2 = enc.encode(&scene2).expect("re-encode");
    assert_eq!(glb_json(&glb2), v);
}

#[test]
fn target_names_coexist_with_other_extras_keys() {
    let bytes = build_doc(r#"{ "targetNames": ["a", "b"], "author": "me" }"#);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).expect("decode");
    let mesh = &scene.meshes[0];
    assert_eq!(mesh.target_names, vec!["a".to_owned(), "b".to_owned()]);
    // The sibling key stays in the stash, minus the lifted one.
    let stash = mesh.primitives[0]
        .extras
        .get("__mesh_extras")
        .expect("remaining extras stashed");
    assert_eq!(stash, &serde_json::json!({ "author": "me" }));

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode");
    let v = glb_json(&glb);
    assert_eq!(
        v["meshes"][0]["extras"],
        serde_json::json!({ "author": "me", "targetNames": ["a", "b"] })
    );
}

#[test]
fn target_names_non_convention_shape_stays_opaque() {
    // An array of numbers is not the implementation-note convention:
    // it stays in the opaque extras and the typed field is empty.
    let bytes = build_doc(r#"{ "targetNames": [1, 2] }"#);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).expect("decode");
    let mesh = &scene.meshes[0];
    assert!(mesh.target_names.is_empty());
    assert_eq!(
        mesh.primitives[0].extras.get("__mesh_extras"),
        Some(&serde_json::json!({ "targetNames": [1, 2] }))
    );
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode");
    assert_eq!(
        glb_json(&glb)["meshes"][0]["extras"]["targetNames"],
        serde_json::json!([1, 2])
    );
}

#[test]
fn target_names_length_mismatch_rejected_on_decode() {
    let bytes = build_doc(r#"{ "targetNames": ["only-one"] }"#);
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&bytes).unwrap_err();
    assert!(
        format!("{err}").contains("MeshTargetNamesLength"),
        "unexpected error: {err}"
    );
}

#[test]
fn target_names_length_mismatch_rejected_on_encode() {
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut t = MorphTarget::new();
    t.position = Some(vec![[0.1, 0.0, 0.0]; 3]);
    prim.targets.push(t);
    let mesh = Mesh::new(None)
        .with_primitive(prim)
        .with_target_names(["a", "b"]);
    let mut scene = Scene3D::new();
    scene.meshes.push(mesh);
    // mesh3d's validate flags the same authoring error the encoder
    // refuses to write.
    assert!(scene.validate().is_err());
    let mut enc = GltfEncoder::new();
    let err = enc.encode(&scene).unwrap_err();
    assert!(
        format!("{err}").contains("MeshTargetNamesLength"),
        "unexpected error: {err}"
    );
}

#[test]
fn typed_names_win_over_legacy_stash_key() {
    // A hand-authored scene that still carries `targetNames` inside the
    // `__mesh_extras` sidecar AND the typed field: the typed field is
    // authoritative on encode.
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut t = MorphTarget::new();
    t.position = Some(vec![[0.1, 0.0, 0.0]; 3]);
    prim.targets.push(t);
    prim.extras.insert(
        "__mesh_extras".to_owned(),
        serde_json::json!({ "targetNames": ["stale"] }),
    );
    let mesh = Mesh::new(None)
        .with_primitive(prim)
        .with_target_names(["fresh"]);
    let mut scene = Scene3D::new();
    scene.meshes.push(mesh);
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode");
    assert_eq!(
        glb_json(&glb)["meshes"][0]["extras"]["targetNames"],
        serde_json::json!(["fresh"])
    );
}
