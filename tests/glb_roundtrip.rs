//! Build a minimal scene → encode `.glb` → decode → re-encode → byte
//! parity on the second encode. Validates the decoder + encoder round
//! trip on a single tiny triangle (the smallest non-empty valid scene).

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, NodeId, Primitive, Scene3D, Topology,
};

fn one_triangle_scene() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("triangle".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid).with_name("triangle_node");
    let nid = scene.add_node(node);
    scene.add_root(nid);
    scene
}

#[test]
fn glb_minimal_roundtrip() {
    let scene = one_triangle_scene();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    assert_eq!(&glb[0..4], b"glTF", "missing GLB magic");

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    assert_eq!(decoded.meshes.len(), 1);
    assert_eq!(decoded.meshes[0].primitives.len(), 1);
    assert_eq!(decoded.meshes[0].primitives[0].positions.len(), 3);
    assert_eq!(decoded.nodes.len(), 1);
    assert_eq!(decoded.roots, vec![NodeId(0)]);

    // Second encode should be byte-stable.
    let glb2 = enc.encode(&decoded).unwrap();
    assert_eq!(glb, glb2, "second .glb encode differs from first");
}

#[test]
fn glb_with_normals_uvs_indices() {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    prim.normals = Some(vec![[0.0, 0.0, 1.0]; 4]);
    prim.uvs = vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]];
    prim.indices = Some(oxideav_mesh3d::Indices::U16(vec![0, 1, 2, 1, 3, 2]));
    let mut mesh = Mesh::new(Some("quad".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let p = &decoded.meshes[0].primitives[0];
    assert_eq!(p.positions, scene.meshes[0].primitives[0].positions);
    assert_eq!(p.normals, scene.meshes[0].primitives[0].normals);
    assert_eq!(p.uvs, scene.meshes[0].primitives[0].uvs);
    match &p.indices {
        Some(oxideav_mesh3d::Indices::U16(v)) => assert_eq!(v, &vec![0, 1, 2, 1, 3, 2]),
        other => panic!("indices should be U16, got {other:?}"),
    }
}
