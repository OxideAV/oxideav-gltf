//! glTF 2.0 lets a document carry several `scenes[]` and select one as
//! the default via top-level `scene`. Our typed `Scene3D` model holds a
//! single live scene-graph (because per spec only the active one is
//! rendered), so we preserve secondary scenes through round-trip via
//! `Scene3D::extras["__additional_scenes"]`. Tests below verify both
//! the round-trip and the active-scene selector.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, NodeId, Primitive, Scene3D, Topology,
};

fn one_triangle_mesh(scene: &mut Scene3D) -> oxideav_mesh3d::MeshId {
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(None);
    mesh.primitives.push(prim);
    scene.add_mesh(mesh)
}

#[test]
fn three_scenes_with_active_selector() {
    // Hand-craft a 3-scene document via JSON. The decoder picks scene 1
    // as primary; scenes 0 and 2 should round-trip through extras.
    let json = r#"{
        "asset": { "version": "2.0" },
        "scene": 1,
        "scenes": [
            { "name": "scene_zero", "nodes": [0] },
            { "name": "scene_one",  "nodes": [1] },
            { "name": "scene_two",  "nodes": [2] }
        ],
        "nodes": [
            { "name": "node_a" },
            { "name": "node_b" },
            { "name": "node_c" }
        ]
    }"#;

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();

    // Active scene is scene 1 → root list is [NodeId(1)].
    assert_eq!(scene.roots, vec![NodeId(1)]);
    // Secondary scenes preserved.
    let additional = scene
        .extras
        .get("__additional_scenes")
        .expect("__additional_scenes missing");
    let arr = additional.as_array().expect("array");
    assert_eq!(arr.len(), 2, "two secondary scenes");
    let names: Vec<&str> = arr
        .iter()
        .map(|v| v.get("name").unwrap().as_str().unwrap())
        .collect();
    assert!(names.contains(&"scene_zero"));
    assert!(names.contains(&"scene_two"));

    // Re-encode: should land 3 scenes back, with `scene = 1`.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let scene2 = dec.decode(&glb).unwrap();

    assert_eq!(scene2.roots, vec![NodeId(1)]);
    let additional2 = scene2
        .extras
        .get("__additional_scenes")
        .expect("__additional_scenes missing on re-decode");
    assert_eq!(additional2.as_array().unwrap().len(), 2);
}

#[test]
fn single_scene_no_extras_pollution() {
    // Sanity: a one-scene document doesn't leak `__additional_scenes`
    // into extras.
    let mut s = Scene3D::new();
    let mid = one_triangle_mesh(&mut s);
    let n = s.add_node(Node::new().with_mesh(mid));
    s.add_root(n);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&s).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    assert!(
        !decoded.extras.contains_key("__additional_scenes"),
        "single-scene document gained __additional_scenes key"
    );
}
