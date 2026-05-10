//! Sparse-encoding heuristic for `skin.inverseBindMatrices` (MAT4)
//! per glTF 2.0 §3.6.2.3 — extends r3's animation-output sparse
//! heuristic to skin IBM accessors. A skeleton with many all-zero
//! IBM matrices (typical for symmetric rigs that carry placeholder
//! joints) re-emits as a sparse accessor when the configured
//! threshold is met.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Skeleton, Skin, Topology,
};

/// Build a scene with a 6-joint skeleton whose IBM matrix list has 2
/// non-zero matrices and 4 all-zero matrices (zero fraction = 4/6
/// ≈ 0.667). Threshold of 0.5 should trigger sparse encoding.
fn skeleton_with_mostly_zero_ibms() -> Scene3D {
    let mut scene = Scene3D::new();

    // Minimal mesh + node so the document is well-formed.
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);

    // Six joint nodes.
    let joint_ids: Vec<_> = (0..6).map(|_| scene.add_node(Node::new())).collect();
    let mesh_node = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(mesh_node);
    for &j in &joint_ids {
        scene.add_root(j);
    }

    // Skeleton: 2 non-zero matrices (translation 1, then 2), 4 all-zero.
    let mut skel = Skeleton::new();
    skel.name = Some("rig".to_owned());
    skel.joints = joint_ids.clone();
    let mut tr1 = identity_mat4();
    tr1[0][3] = 1.0;
    let mut tr2 = identity_mat4();
    tr2[1][3] = 2.0;
    skel.inverse_bind_matrices = vec![tr1, zero_mat4(), zero_mat4(), tr2, zero_mat4(), zero_mat4()];
    let skel_id = scene.add_skeleton(skel);
    scene.add_skin(Skin::new(skel_id));
    scene
}

#[test]
fn ibm_sparse_threshold_emits_sparse_accessor() {
    let scene = skeleton_with_mostly_zero_ibms();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();

    let json_chunk = extract_json_chunk(&glb);
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let accessors = v["accessors"].as_array().unwrap();

    // The MAT4 IBM accessor should now carry a sparse block with
    // count == 2 (the two non-zero matrices) and no base bufferView.
    let mut found = false;
    for acc in accessors {
        if acc["type"] == "MAT4" && acc["componentType"] == 5126 {
            if let Some(s) = acc.get("sparse") {
                assert_eq!(s["count"], 2, "expected 2 sparse override slots");
                assert!(
                    acc.get("bufferView").is_none(),
                    "sparse zero-base must drop bufferView"
                );
                found = true;
            }
        }
    }
    assert!(found, "expected a MAT4 FLOAT accessor with sparse storage");

    // Round-trip through the decoder: the all-zero matrices come back
    // as all-zero, the two non-zeros recover bit-for-bit.
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let skel = decoded
        .skeletons
        .first()
        .expect("decoded skeleton should round-trip");
    assert_eq!(skel.inverse_bind_matrices.len(), 6);

    let mut tr1 = identity_mat4();
    tr1[0][3] = 1.0;
    let mut tr2 = identity_mat4();
    tr2[1][3] = 2.0;
    assert_eq!(skel.inverse_bind_matrices[0], tr1);
    assert_eq!(skel.inverse_bind_matrices[1], zero_mat4());
    assert_eq!(skel.inverse_bind_matrices[2], zero_mat4());
    assert_eq!(skel.inverse_bind_matrices[3], tr2);
    assert_eq!(skel.inverse_bind_matrices[4], zero_mat4());
    assert_eq!(skel.inverse_bind_matrices[5], zero_mat4());
}

#[test]
fn ibm_no_threshold_keeps_dense() {
    // Default encoder (no threshold) must NOT emit sparse for IBMs.
    let scene = skeleton_with_mostly_zero_ibms();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    for acc in accessors {
        if acc["type"] == "MAT4" {
            assert!(
                acc.get("sparse").is_none(),
                "default encoder must not emit sparse IBMs"
            );
            assert!(
                acc.get("bufferView").is_some(),
                "dense IBM must reference a bufferView"
            );
        }
    }
}

#[test]
fn ibm_high_threshold_keeps_dense() {
    // 4/6 ≈ 0.667 zero fraction — a 0.9 threshold should keep it dense.
    let scene = skeleton_with_mostly_zero_ibms();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.9);
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    for acc in accessors {
        if acc["type"] == "MAT4" {
            assert!(
                acc.get("sparse").is_none(),
                "0.9 threshold should keep 0.667-zero IBM dense"
            );
        }
    }
}

#[test]
fn ibm_all_nonzero_at_high_threshold_stays_dense() {
    // When no IBM matrix is all-zero (zero_fraction = 0.0) any
    // threshold > 0.0 must keep the accessor dense — sparse only
    // helps when at least some matrices are zero.
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let joint_ids: Vec<_> = (0..3).map(|_| scene.add_node(Node::new())).collect();
    let mesh_node = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(mesh_node);
    for &j in &joint_ids {
        scene.add_root(j);
    }
    let mut skel = Skeleton::new();
    skel.joints = joint_ids;
    skel.inverse_bind_matrices = vec![identity_mat4(), identity_mat4(), identity_mat4()];
    let skel_id = scene.add_skeleton(skel);
    scene.add_skin(Skin::new(skel_id));

    // Any threshold > 0.0 should keep this dense (zero_fraction = 0.0
    // < threshold). 0.5 is plenty far from the data's actual zero
    // fraction.
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    let any_sparse_mat4 = accessors
        .iter()
        .any(|a| a["type"] == "MAT4" && a.get("sparse").is_some());
    assert!(
        !any_sparse_mat4,
        "all-non-zero IBM at threshold 0.5 must stay dense"
    );

    // Round-trip preserves the matrices either way.
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let skel = decoded.skeletons.first().expect("skeleton round-trip");
    for m in &skel.inverse_bind_matrices {
        assert_eq!(*m, identity_mat4());
    }
}

// --- helpers --------------------------------------------------------------

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert!(glb.len() >= 20);
    let chunk0_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    let chunk0_kind = &glb[16..20];
    assert_eq!(chunk0_kind, b"JSON");
    glb[20..20 + chunk0_len].to_vec()
}

fn identity_mat4() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn zero_mat4() -> [[f32; 4]; 4] {
    [[0.0; 4]; 4]
}
