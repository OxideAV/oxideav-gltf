//! `extras` round-trips on root, node, material, primitive — all
//! surfaces that mesh3d carries an `extras: HashMap<String, Value>`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Material, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology,
};
use serde_json::json;

#[test]
fn extras_round_trip() {
    let mut scene = Scene3D::new();
    scene
        .extras
        .insert("authoring_tool".into(), json!("oxideav-gltf-r1"));
    scene.extras.insert("rev".into(), json!(7));

    let mut mat = Material::new();
    mat.extras.insert("custom_shader".into(), json!("toon_v2"));
    let mid = scene.add_material(mat);

    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    prim.material = Some(mid);
    prim.extras.insert("draw_priority".into(), json!(3));
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let meshid = scene.add_mesh(mesh);

    let mut node = Node::new().with_mesh(meshid).with_name("n");
    node.extras.insert("user_id".into(), json!("abc"));
    let nid = scene.add_node(node);
    scene.add_root(nid);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();

    assert_eq!(
        decoded.extras.get("authoring_tool"),
        Some(&json!("oxideav-gltf-r1"))
    );
    assert_eq!(decoded.extras.get("rev"), Some(&json!(7)));
    assert_eq!(
        decoded.materials[0].extras.get("custom_shader"),
        Some(&json!("toon_v2"))
    );
    assert_eq!(
        decoded.meshes[0].primitives[0].extras.get("draw_priority"),
        Some(&json!(3))
    );
    assert_eq!(decoded.nodes[0].extras.get("user_id"), Some(&json!("abc")));
}
