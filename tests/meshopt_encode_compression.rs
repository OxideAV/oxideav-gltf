//! Write-side `KHR_meshopt_compression` — `GltfEncoder::with_meshopt_compression`
//! compresses index bufferViews on write and the document round-trips
//! back through this crate's decoder to the original indices.
//!
//! Layout produced per
//! `docs/3d/gltf/extensions/KHR_meshopt_compression.md`
//! §"Specifying compressed views":
//! * buffer 0 (the BIN) keeps the uncompressed indices (a plain buffer,
//!   NOT a fallback — it also backs the vertex views),
//! * a second `data:`-URI buffer carries the compressed payloads,
//! * each compressed index bufferView gains the `INDICES`-mode
//!   descriptor, and
//! * `KHR_meshopt_compression` is declared in `extensionsUsed` (the
//!   document stays readable without it, so not `extensionsRequired`).

use oxideav_gltf::{json_encoder, GltfDecoder, GltfEncoder, OutputFlavour};
use oxideav_mesh3d::{
    Indices, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology,
};
use serde_json::Value;

fn indexed_quad_scene(indices: Indices) -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    prim.indices = Some(indices);
    let mut mesh = Mesh::new(Some("quad".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);
    scene
}

/// A quad whose index buffer references up to `max(indices)`, with
/// enough positions to keep the indices in range. Forces a u16 index
/// accessor (some index exceeds u8::MAX).
fn quad_scene_with_indices_u16(indices: &[u16]) -> Scene3D {
    let max = indices.iter().copied().max().unwrap_or(0) as usize;
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0]; max + 1];
    prim.indices = Some(Indices::U16(indices.to_vec()));
    let mut mesh = Mesh::new(Some("quad".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);
    scene
}

fn decode_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("emitted .gltf is valid JSON")
}

#[test]
fn meshopt_write_u16_roundtrips() {
    // Indices exceed u8::MAX so the encoder keeps a u16 (byteStride 2)
    // index accessor — the smallest-component narrowing would otherwise
    // pick UNSIGNED_BYTE (stride 1), which meshopt INDICES can't carry.
    let original = vec![0u16, 300, 2, 300, 3, 2];
    let scene = quad_scene_with_indices_u16(&original);
    let mut enc = json_encoder().with_meshopt_compression(true);
    let bytes = enc.encode(&scene).expect("encode with meshopt");

    // Document shape: extension declared (used + required), descriptor on
    // the index bufferView, fallback marker on buffer 0, compressed
    // payload buffer present.
    let doc = decode_json(&bytes);
    let used: Vec<&str> = doc["extensionsUsed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(used.contains(&"KHR_meshopt_compression"));
    // NOT required — the document stays readable without the extension.
    assert!(
        doc.get("extensionsRequired").is_none()
            || doc["extensionsRequired"]
                .as_array()
                .map(|a| a
                    .iter()
                    .all(|v| v.as_str() != Some("KHR_meshopt_compression")))
                .unwrap_or(true),
        "meshopt index compression must not be required"
    );

    // Two bufferViews carry descriptors: the POSITION (ATTRIBUTES) view
    // and the index (INDICES) view.
    let bvs = doc["bufferViews"].as_array().unwrap();
    let descriptors: Vec<&Value> = bvs
        .iter()
        .filter_map(|bv| {
            bv.get("extensions")
                .and_then(|e| e.get("KHR_meshopt_compression"))
        })
        .collect();
    assert_eq!(descriptors.len(), 2, "POSITION + index views compressed");
    let index_desc = descriptors
        .iter()
        .find(|d| d["mode"].as_str() == Some("INDICES"))
        .expect("INDICES descriptor present");
    assert_eq!(index_desc["byteStride"].as_u64(), Some(2));
    assert_eq!(index_desc["count"].as_u64(), Some(6));
    let attr_desc = descriptors
        .iter()
        .find(|d| d["mode"].as_str() == Some("ATTRIBUTES"))
        .expect("ATTRIBUTES descriptor present");
    assert_eq!(attr_desc["byteStride"].as_u64(), Some(12)); // POSITION VEC3 f32

    // A second buffer holds the compressed payloads; buffer 0 stays a
    // plain real-data buffer (no fallback marker).
    let buffers = doc["buffers"].as_array().unwrap();
    assert!(buffers.len() >= 2, "uncompressed + compressed buffers");
    assert!(
        buffers[0].get("extensions").is_none(),
        "buffer 0 is not a fallback placeholder"
    );
    assert!(
        buffers[1]["uri"].as_str().unwrap().starts_with("data:"),
        "compressed payload buffer is a data URI"
    );

    // Round-trip: decode inflates the descriptor back to the indices.
    let mut dec = GltfDecoder::new();
    let scene2 = dec.decode(&bytes).expect("decode meshopt-compressed doc");
    let p = &scene2.meshes[0].primitives[0];
    assert_eq!(p.triangle_indices(), vec![[0, 300, 2], [300, 3, 2]]);
}

#[test]
fn meshopt_write_u32_roundtrips() {
    // Force u32 indices by using a value beyond u16::MAX.
    let big = 70_000u32;
    let mut positions = vec![[0.0f32, 0.0, 0.0]; (big + 1) as usize];
    positions[0] = [0.0, 0.0, 0.0];
    // A degenerate-but-valid triangle list referencing a high index.
    let original = vec![0u32, 1, big, big, 1, 2];
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = positions;
    prim.indices = Some(Indices::U32(original));
    let mut mesh = Mesh::new(Some("big".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);

    let mut enc = json_encoder().with_meshopt_compression(true);
    let bytes = enc.encode(&scene).expect("encode u32 meshopt");
    let doc = decode_json(&bytes);
    let bvs = doc["bufferViews"].as_array().unwrap();
    let d = bvs
        .iter()
        .filter_map(|bv| {
            bv.get("extensions")
                .and_then(|e| e.get("KHR_meshopt_compression"))
        })
        .find(|d| d["mode"].as_str() == Some("INDICES"))
        .expect("INDICES descriptor present");
    assert_eq!(d["byteStride"].as_u64(), Some(4));

    let mut dec = GltfDecoder::new();
    let scene2 = dec.decode(&bytes).expect("decode u32 meshopt doc");
    let p = &scene2.meshes[0].primitives[0];
    assert_eq!(p.triangle_indices(), vec![[0, 1, 70_000], [70_000, 1, 2]]);
}

#[test]
fn meshopt_write_off_by_default() {
    let scene = indexed_quad_scene(Indices::U16(vec![0, 1, 2, 1, 3, 2]));
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode plain");
    let doc = decode_json(&bytes);
    // No extension declared when the flag is off.
    assert!(
        doc.get("extensionsUsed")
            .and_then(|u| u.as_array())
            .map(|a| a
                .iter()
                .all(|v| v.as_str() != Some("KHR_meshopt_compression")))
            .unwrap_or(true),
        "meshopt must be opt-in"
    );
}

#[test]
fn meshopt_write_attributes_only_roundtrips() {
    // A non-indexed primitive: only the POSITION (ATTRIBUTES) view is
    // compressed; it round-trips back to the original positions.
    // Six positions = two triangles (non-indexed count divisible by 3).
    let positions = vec![
        [1.5f32, -2.0, 3.25],
        [1.75, -1.0, 3.0],
        [0.0, 0.5, 10.0],
        [2.0, 2.0, 2.0],
        [-1.0, -1.0, -1.0],
        [5.0, 0.0, -3.5],
    ];
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = positions.clone();
    let mut mesh = Mesh::new(Some("tri".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);

    let mut enc = json_encoder().with_meshopt_compression(true);
    let bytes = enc.encode(&scene).expect("encode attributes meshopt");
    let doc = decode_json(&bytes);
    // POSITION view compressed as ATTRIBUTES.
    let d = doc["bufferViews"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|bv| {
            bv.get("extensions")
                .and_then(|e| e.get("KHR_meshopt_compression"))
        })
        .find(|d| d["mode"].as_str() == Some("ATTRIBUTES"))
        .expect("ATTRIBUTES descriptor present");
    assert_eq!(d["byteStride"].as_u64(), Some(12));
    assert_eq!(d["count"].as_u64(), Some(6));

    let mut dec = GltfDecoder::new();
    let scene2 = dec.decode(&bytes).expect("decode attributes meshopt doc");
    assert_eq!(scene2.meshes[0].primitives[0].positions, positions);
}

#[test]
fn meshopt_write_glb_flavour_roundtrips() {
    // GLB flavour: buffer 0 is the uri-less BIN chunk holding the
    // uncompressed indices, compressed payload buffer is a data: URI.
    let scene = quad_scene_with_indices_u16(&[0, 300, 2, 300, 3, 2]);
    let mut enc = GltfEncoder::with_output(OutputFlavour::Glb).with_meshopt_compression(true);
    let glb = enc.encode(&scene).expect("encode glb meshopt");
    assert_eq!(&glb[0..4], b"glTF");

    let mut dec = GltfDecoder::new();
    let scene2 = dec.decode(&glb).expect("decode glb meshopt");
    let p = &scene2.meshes[0].primitives[0];
    assert_eq!(p.triangle_indices(), vec![[0, 300, 2], [300, 3, 2]]);
}

#[test]
fn meshopt_write_multi_attribute_roundtrips() {
    // POSITION (stride 12) + NORMAL (12) + TEXCOORD (8) + indices, all
    // compressed and round-tripped. Exercises several ATTRIBUTES strides
    // in one document.
    let positions = vec![
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let normals = vec![[0.0f32, 0.0, 1.0]; 4];
    let indices = vec![0u16, 300, 2, 300, 3, 2];

    // Pad positions/normals/uvs to reference index 300.
    let mut positions = positions;
    let mut normals = normals;
    let mut uvs0 = vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    positions.resize(301, [0.0, 0.0, 0.0]);
    normals.resize(301, [0.0, 0.0, 1.0]);
    uvs0.resize(301, [0.0, 0.0]);

    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = positions.clone();
    prim.normals = Some(normals.clone());
    prim.uvs = vec![uvs0.clone()];
    prim.indices = Some(Indices::U16(indices));
    let mut mesh = Mesh::new(Some("multi".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);

    let mut enc = json_encoder().with_meshopt_compression(true);
    let bytes = enc.encode(&scene).expect("encode multi-attr meshopt");

    // At least 4 descriptors: POSITION, NORMAL, TEXCOORD, indices.
    let doc = decode_json(&bytes);
    let descriptors: Vec<&Value> = doc["bufferViews"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|bv| {
            bv.get("extensions")
                .and_then(|e| e.get("KHR_meshopt_compression"))
        })
        .collect();
    assert!(
        descriptors.len() >= 4,
        "all attribute + index views compressed"
    );

    let mut dec = GltfDecoder::new();
    let scene2 = dec.decode(&bytes).expect("decode multi-attr meshopt");
    let p = &scene2.meshes[0].primitives[0];
    assert_eq!(p.positions, positions);
    assert_eq!(p.normals.as_ref().unwrap(), &normals);
    assert_eq!(p.uvs[0], uvs0);
    assert_eq!(p.triangle_indices(), vec![[0, 300, 2], [300, 3, 2]]);
}
