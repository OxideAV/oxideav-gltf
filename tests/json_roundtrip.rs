//! `.gltf` JSON variant round-trip — encode with the JsonEmbedded
//! flavour (binary buffer inlined as a base64 `data:` URI), decode,
//! and verify byte parity on the second encode.

use oxideav_gltf::{GltfDecoder, GltfEncoder, OutputFlavour};
use oxideav_mesh3d::{Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology};

#[test]
fn json_minimal_roundtrip() {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let mut mesh = Mesh::new(Some("triangle".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let nid = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(nid);

    let mut enc = GltfEncoder::with_output(OutputFlavour::JsonEmbedded);
    let bytes = enc.encode(&scene).unwrap();
    // First byte should be `{` (JSON, NOT GLB magic).
    assert_eq!(bytes[0], b'{', "JSON output must start with '{{'");

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&bytes).unwrap();
    assert_eq!(decoded.meshes.len(), 1);
    assert_eq!(
        decoded.meshes[0].primitives[0].positions,
        scene.meshes[0].primitives[0].positions
    );

    let bytes2 = enc.encode(&decoded).unwrap();
    assert_eq!(bytes, bytes2, "second JSON encode differs from first");
}
