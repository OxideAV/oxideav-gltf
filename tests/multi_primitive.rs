//! Mesh with three primitives + three distinct materials.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Material, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology,
};

#[test]
fn three_primitives_three_materials() {
    let mut scene = Scene3D::new();
    let m0 = scene.add_material(Material::new().with_name("m0"));
    let m1 = scene.add_material(Material::new().with_name("m1"));
    let m2 = scene.add_material(Material::new().with_name("m2"));

    let mut mesh = Mesh::new(Some("multi".to_owned()));
    for (i, mid) in [m0, m1, m2].iter().enumerate() {
        let mut p = Primitive::new(Topology::Triangles);
        let off = i as f32;
        p.positions = vec![[off, 0.0, 0.0], [off + 1.0, 0.0, 0.0], [off, 1.0, 0.0]];
        p.material = Some(*mid);
        mesh.primitives.push(p);
    }
    let mid = scene.add_mesh(mesh);
    let nid = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(nid);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();

    assert_eq!(decoded.meshes.len(), 1);
    assert_eq!(decoded.meshes[0].primitives.len(), 3);
    assert_eq!(decoded.materials.len(), 3);
    for (i, p) in decoded.meshes[0].primitives.iter().enumerate() {
        assert_eq!(p.material.unwrap().0, i as u32);
        assert_eq!(p.positions[0][0], i as f32);
    }
}
