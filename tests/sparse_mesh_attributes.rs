//! Sparse-encoding heuristic for mesh vertex-attribute accessors per
//! glTF 2.0 §3.6.2.3. Extends the same `with_sparse_threshold(f32)`
//! knob used by animation outputs (r3) and IBM accessors (r5 item a)
//! to POSITION / NORMAL / TANGENT / COLOR_n / WEIGHTS_0 attributes:
//! when the all-components-zero element fraction crosses the
//! threshold the attribute accessor is re-emitted as zero-base
//! sparse with per-index overrides for the non-zero vertices.
//!
//! Spec note: POSITION accessors must declare min/max (§3.6.2.1.5);
//! the sparse path computes them from the dequantised data (which is
//! identical to the dense data — the decoder applies the overrides
//! over the zero base before the bounds check), so they remain
//! correct on both paths.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology};

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert!(glb.len() >= 20);
    let chunk0_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    glb[20..20 + chunk0_len].to_vec()
}

/// Build a primitive with mostly-zero POSITION + NORMAL + COLOR_0
/// data. 6 vertices total; only vertex index 2 is non-zero across
/// all attributes (zero fraction = 5/6 ≈ 0.833).
fn primitive_with_mostly_zero_attributes() -> Primitive {
    let mut prim = Primitive::new(Topology::Points);
    let mut positions = vec![[0.0f32; 3]; 6];
    positions[2] = [3.0, 4.0, 5.0];
    prim.positions = positions;
    let mut normals = vec![[0.0f32; 3]; 6];
    normals[2] = [0.0, 1.0, 0.0];
    prim.normals = Some(normals);
    let mut colors = vec![[0.0f32; 4]; 6];
    colors[2] = [1.0, 0.5, 0.25, 1.0];
    prim.colors.push(colors);
    prim
}

fn scene_with_sparse_friendly_mesh() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut mesh = Mesh::new(Some("sparse".to_owned()));
    mesh.primitives
        .push(primitive_with_mostly_zero_attributes());
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    scene
}

#[test]
fn position_attribute_sparse_at_threshold() {
    let scene = scene_with_sparse_friendly_mesh();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    let position = accessors
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some("POSITION"))
        .expect("POSITION accessor");
    assert!(
        position.get("sparse").is_some(),
        "POSITION should be sparse at threshold 0.5 (zero fraction 5/6)"
    );
    assert!(
        position.get("bufferView").is_none(),
        "sparse zero-base POSITION must drop bufferView"
    );
    // Spec §3.6.2.1.5: POSITION must declare min/max even when sparse.
    let min = position["min"].as_array().expect("POSITION min");
    let max = position["max"].as_array().expect("POSITION max");
    assert_eq!(min.len(), 3);
    assert_eq!(max.len(), 3);
    // Bounds reflect the dequantised data, including the zero base.
    assert_eq!(min[0].as_f64().unwrap(), 0.0);
    assert_eq!(min[1].as_f64().unwrap(), 0.0);
    assert_eq!(min[2].as_f64().unwrap(), 0.0);
    assert_eq!(max[0].as_f64().unwrap(), 3.0);
    assert_eq!(max[1].as_f64().unwrap(), 4.0);
    assert_eq!(max[2].as_f64().unwrap(), 5.0);

    // Round-trip preserves vertex data exactly.
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let prim = &decoded.meshes[0].primitives[0];
    assert_eq!(prim.positions.len(), 6);
    assert_eq!(prim.positions[0], [0.0, 0.0, 0.0]);
    assert_eq!(prim.positions[2], [3.0, 4.0, 5.0]);
    assert_eq!(prim.positions[5], [0.0, 0.0, 0.0]);
    let normals = prim.normals.as_ref().expect("NORMAL");
    assert_eq!(normals[0], [0.0, 0.0, 0.0]);
    assert_eq!(normals[2], [0.0, 1.0, 0.0]);
    let colors = &prim.colors[0];
    assert_eq!(colors[0], [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(colors[2], [1.0, 0.5, 0.25, 1.0]);
}

#[test]
fn position_no_threshold_keeps_dense() {
    // Default encoder must not emit sparse mesh attributes — preserves
    // the r1-r4 baseline behaviour for documents that don't opt in.
    let scene = scene_with_sparse_friendly_mesh();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    let any_sparse = accessors.iter().any(|a| a.get("sparse").is_some());
    assert!(
        !any_sparse,
        "default encoder must not emit sparse attributes"
    );
}

#[test]
fn normal_and_color_sparse_at_threshold() {
    let scene = scene_with_sparse_friendly_mesh();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();

    let normal = accessors
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some("NORMAL"))
        .expect("NORMAL accessor");
    assert!(
        normal.get("sparse").is_some(),
        "NORMAL should be sparse at threshold 0.5"
    );
    // NORMAL doesn't carry mandatory min/max; they should be absent on
    // both dense and sparse paths.
    assert!(normal.get("min").is_none(), "NORMAL must not declare min");
    assert!(normal.get("max").is_none(), "NORMAL must not declare max");

    let color = accessors
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some("COLOR"))
        .expect("COLOR accessor");
    assert!(
        color.get("sparse").is_some(),
        "COLOR_0 should be sparse at threshold 0.5"
    );
    assert_eq!(color["type"], "VEC4");
}

#[test]
fn tangent_vec4_sparse_round_trip() {
    // TANGENT is VEC4 with the W component constrained to ±1.0 by
    // spec §3.7.2.1. The zero-base sparse path would synthesise w=0
    // elements at every non-overridden slot, which is a hard spec
    // violation — so TANGENT always stays dense regardless of the
    // sparse threshold (round 6 fix; r5 emitted sparse here and the
    // resulting document failed VertexAttributeTangentW validation
    // when re-decoded). This test pins the contract.
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[1.0, 0.0, 0.0]; 4]; // all non-zero so POSITION stays dense
    prim.normals = Some(vec![[0.0, 1.0, 0.0]; 4]);
    // All TANGENT elements carry spec-valid w = ±1.0 even when xyz is
    // (0,0,0); the encoder must NOT try to compress them with a
    // zero-base sparse block.
    let tangents = vec![
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    prim.tangents = Some(tangents.clone());
    let mut mesh = Mesh::new(Some("t".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);

    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    let tangent = accessors
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some("TANGENT"))
        .expect("TANGENT accessor");
    assert!(
        tangent.get("sparse").is_none(),
        "TANGENT MUST stay dense regardless of threshold (w must be ±1.0 per spec §3.7.2.1)"
    );

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let prim = &decoded.meshes[0].primitives[0];
    let decoded_tangents = prim.tangents.as_ref().expect("TANGENT");
    assert_eq!(decoded_tangents, &tangents);
}

#[test]
fn dense_attributes_at_high_threshold() {
    // Threshold 0.99 with 5/6 zero fraction (~0.833) keeps dense.
    let scene = scene_with_sparse_friendly_mesh();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.99);
    let glb = enc.encode(&scene).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&extract_json_chunk(&glb)).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    for acc in accessors {
        let name = acc.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if matches!(name, "POSITION" | "NORMAL" | "COLOR") {
            assert!(
                acc.get("sparse").is_none(),
                "0.99 threshold must keep {name} dense"
            );
        }
    }
}
