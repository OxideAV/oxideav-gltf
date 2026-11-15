//! `KHR_draco_mesh_compression` loading: a primitive carrying the
//! extension decodes through the Draco bitstream decoder into the same
//! typed primitive an uncompressed document yields.
//!
//! Mapping under test (`docs/3d/gltf/draco/spec/09-gltf-join.md` and
//! `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md` §Conformance):
//! extension attribute ids are payload unique ids; accessor element `j` is
//! the value of point `j`; `indices` lists the faces' point indices in the
//! accessor's declared `componentType`; attributes outside the extension's
//! map load as usual; a primitive whose payload cannot be decoded loads
//! from its uncompressed copy when it has one.
//!
//! The assets are assembled in the tests from the staged payloads under
//! `tests/fixtures/draco/` (plus the staged `glb-grid4/input.glb`, written
//! by a black-box transcoder).

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_gltf::draco::{self, DataType, DracoMesh, Semantic};
use oxideav_mesh3d::{Indices, Mesh3DDecoder, Primitive, Scene3D};
use serde_json::{Value, json};
use std::path::PathBuf;

fn fixture(name: &str, file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/draco")
        .join(name)
        .join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn data_uri(bytes: &[u8]) -> String {
    format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn semantic_name(s: Semantic) -> &'static str {
    match s {
        Semantic::Position => "POSITION",
        Semantic::Normal => "NORMAL",
        Semantic::TexCoord => "TEXCOORD_0",
        Semantic::Color => "COLOR_0",
        _ => "_GENERIC",
    }
}

fn accessor_type(components: u8) -> &'static str {
    ["", "SCALAR", "VEC2", "VEC3", "VEC4"][usize::from(components)]
}

fn component_type(dt: DataType) -> u32 {
    match dt {
        DataType::Uint8 => 5121,
        DataType::Float32 => 5126,
        other => panic!("{other:?} not used by these tests"),
    }
}

/// Per-point float values of attribute `i`.
fn point_values(mesh: &DracoMesh, i: usize) -> Vec<Vec<f32>> {
    (0..mesh.num_points as usize)
        .map(|p| mesh.attributes[i].point_f32(p).unwrap())
        .collect()
}

/// Accessor JSON for attribute `i` of the decoded mesh (no bufferView).
fn attribute_accessor(mesh: &DracoMesh, i: usize) -> Value {
    let a = &mesh.attributes[i];
    let mut acc = json!({
        "componentType": component_type(a.data_type),
        "count": mesh.num_points,
        "type": accessor_type(a.num_components),
    });
    if a.semantic == Semantic::Position {
        let values = point_values(mesh, i);
        let fold = |f: fn(f32, f32) -> f32, init: f32| -> Vec<f32> {
            (0..3)
                .map(|c| values.iter().fold(init, |m, v| f(m, v[c])))
                .collect()
        };
        acc["min"] = json!(fold(f32::min, f32::INFINITY));
        acc["max"] = json!(fold(f32::max, f32::NEG_INFINITY));
    }
    acc
}

/// A glTF document whose single primitive is the Draco payload `drc`:
/// accessor 0 is `indices`, accessor `1 + i` is payload attribute `i`,
/// none with a `bufferView`; bufferView 0 / buffer 0 hold the payload
/// followed by `padding` zero bytes.
fn compressed_document(drc: &[u8], mesh: &DracoMesh, index_component_type: u32) -> Value {
    let mut payload = drc.to_vec();
    payload.extend_from_slice(&[0; 3][..(4 - drc.len() % 4) % 4]);
    let mut accessors = vec![json!({
        "componentType": index_component_type,
        "count": mesh.faces.len() * 3,
        "type": "SCALAR",
    })];
    let mut attributes = serde_json::Map::new();
    let mut ids = serde_json::Map::new();
    for (i, a) in mesh.attributes.iter().enumerate() {
        accessors.push(attribute_accessor(mesh, i));
        let name = semantic_name(a.semantic);
        attributes.insert(name.into(), json!(i + 1));
        ids.insert(name.into(), json!(a.unique_id));
    }
    json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "extensionsRequired": ["KHR_draco_mesh_compression"],
        "scene": 0,
        "scenes": [ { "nodes": [0] } ],
        "nodes": [ { "mesh": 0 } ],
        "buffers": [ { "byteLength": payload.len(), "uri": data_uri(&payload) } ],
        "bufferViews": [ { "buffer": 0, "byteLength": payload.len() } ],
        "accessors": accessors,
        "meshes": [ { "primitives": [ {
            "attributes": attributes,
            "indices": 0,
            "mode": 4,
            "extensions": { "KHR_draco_mesh_compression": {
                "bufferView": 0,
                "attributes": ids,
            } },
        } ] } ],
    })
}

/// The same geometry as an ordinary uncompressed document: one buffer with
/// u32 indices followed by each attribute's per-point data.
fn uncompressed_document(mesh: &DracoMesh) -> Value {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    for &i in mesh.faces.iter().flatten() {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    views.push(json!({ "buffer": 0, "byteOffset": 0, "byteLength": bin.len() }));
    accessors.push(json!({
        "bufferView": 0, "componentType": 5125, "count": mesh.faces.len() * 3, "type": "SCALAR",
    }));
    let mut attributes = serde_json::Map::new();
    for (i, a) in mesh.attributes.iter().enumerate() {
        let start = bin.len();
        for p in 0..mesh.num_points as usize {
            bin.extend_from_slice(a.point_bytes(p));
        }
        views.push(json!({ "buffer": 0, "byteOffset": start, "byteLength": bin.len() - start }));
        let mut acc = attribute_accessor(mesh, i);
        acc["bufferView"] = json!(i + 1);
        accessors.push(acc);
        attributes.insert(semantic_name(a.semantic).into(), json!(i + 1));
    }
    json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [ { "nodes": [0] } ],
        "nodes": [ { "mesh": 0 } ],
        "buffers": [ { "byteLength": bin.len(), "uri": data_uri(&bin) } ],
        "bufferViews": views,
        "accessors": accessors,
        "meshes": [ { "primitives": [ {
            "attributes": attributes, "indices": 0, "mode": 4,
        } ] } ],
    })
}

fn load(doc: &Value) -> Result<Scene3D, String> {
    GltfDecoder::new()
        .decode(serde_json::to_string(doc).unwrap().as_bytes())
        .map_err(|e| e.to_string())
}

fn indices_u32(p: &Primitive) -> Vec<u32> {
    match p.indices.as_ref().expect("indexed primitive") {
        Indices::U16(v) => v.iter().map(|&i| u32::from(i)).collect(),
        Indices::U32(v) => v.clone(),
    }
}

/// Assert the primitive carries exactly the decoded mesh.
fn assert_primitive_is_mesh(p: &Primitive, mesh: &DracoMesh) {
    let faces: Vec<u32> = mesh.faces.iter().flatten().copied().collect();
    assert_eq!(indices_u32(p), faces);
    for (i, a) in mesh.attributes.iter().enumerate() {
        if a.data_type != DataType::Float32 {
            continue;
        }
        let want = point_values(mesh, i);
        match a.semantic {
            Semantic::Position => {
                let got: Vec<Vec<f32>> = p.positions.iter().map(|v| v.to_vec()).collect();
                assert_eq!(got, want, "POSITION");
            }
            Semantic::Normal => {
                let got: Vec<Vec<f32>> = p
                    .normals
                    .as_ref()
                    .expect("NORMAL")
                    .iter()
                    .map(|v| v.to_vec())
                    .collect();
                assert_eq!(got, want, "NORMAL");
            }
            Semantic::TexCoord => {
                let got: Vec<Vec<f32>> = p.uvs[0].iter().map(|v| v.to_vec()).collect();
                assert_eq!(got, want, "TEXCOORD_0");
            }
            _ => {}
        }
    }
    assert!(
        !p.extras.contains_key("KHR_draco_mesh_compression"),
        "an inflated primitive is a plain primitive"
    );
}

/// Fixtures with float attributes only, covering both encoding methods,
/// seams (points ≠ vertices), every predictor and both symbol codings.
const FLOAT_FIXTURES: [&str; 9] = [
    "seq-grid4-quant",
    "seq-tri-raw",
    "eb-quad",
    "eb-cube-seams",
    "eb-cube-seams-valence",
    "eb-torus-normals-cl10",
    "eb-grid4-noquant",
    "lvl-grid7-cl7",
    "lvl-grid7-cl10",
];

#[test]
fn compressed_primitive_equals_uncompressed_primitive() {
    for name in FLOAT_FIXTURES {
        let drc = fixture(name, "input.drc");
        let mesh = draco::decode(&drc).unwrap();
        let compressed =
            load(&compressed_document(&drc, &mesh, 5123)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let plain = load(&uncompressed_document(&mesh)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let c = &compressed.meshes[0].primitives[0];
        let u = &plain.meshes[0].primitives[0];
        assert_primitive_is_mesh(c, &mesh);
        assert_eq!(c.positions, u.positions, "{name}");
        assert_eq!(c.normals, u.normals, "{name}");
        assert_eq!(c.uvs, u.uvs, "{name}");
        assert_eq!(indices_u32(c), indices_u32(u), "{name}");
        assert_eq!(c.topology, u.topology, "{name}");
    }
}

#[test]
fn staged_glb_loads_through_the_extension() {
    let glb = fixture("glb-grid4", "input.glb");
    let mesh = draco::decode(&fixture("glb-grid4", "input.drc")).unwrap();
    let scene = GltfDecoder::new().decode(&glb).unwrap();
    let p = &scene.meshes[0].primitives[0];
    assert_eq!(p.positions.len(), 16);
    assert_eq!(indices_u32(p).len(), 54);
    assert_eq!(p.uvs.len(), 1);
    assert!(p.normals.is_some());
    // NORMAL ↔ unique id 0 is the third payload attribute, POSITION ↔ 1 the
    // first, TEXCOORD_0 ↔ 2 the second (ch. 09 §2): resolved by unique id.
    assert_primitive_is_mesh(p, &mesh);
    assert!(p.material.is_some());
}

#[test]
fn glb_container_built_from_a_staged_payload() {
    let drc = fixture("eb-torus", "input.drc");
    let mesh = draco::decode(&drc).unwrap();
    let mut doc = compressed_document(&drc, &mesh, 5121);
    // Move the payload from the data URI into the GLB BIN chunk.
    let mut bin = drc.clone();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    doc["buffers"] = json!([ { "byteLength": bin.len() } ]);
    let mut json_bytes = serde_json::to_vec(&doc).unwrap();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json_bytes);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);

    let scene = GltfDecoder::new().decode(&glb).unwrap();
    assert_primitive_is_mesh(&scene.meshes[0].primitives[0], &mesh);
    assert_eq!(scene.meshes[0].primitives[0].positions.len(), 24);
}

#[test]
fn index_accessor_component_type_is_honoured() {
    // 272 points: unsigned short and unsigned int index accessors load,
    // an unsigned byte one cannot hold the indices.
    let drc = fixture("eb-grid17", "input.drc");
    let mesh = draco::decode(&drc).unwrap();
    for ct in [5123, 5125] {
        let scene = load(&compressed_document(&drc, &mesh, ct)).unwrap();
        assert_primitive_is_mesh(&scene.meshes[0].primitives[0], &mesh);
    }
    let err = load(&compressed_document(&drc, &mesh, 5121)).unwrap_err();
    assert!(err.contains("KHR_draco_mesh_compression"), "{err}");
    assert!(err.contains("does not fit"), "{err}");
}

#[test]
fn attribute_outside_the_extension_map_loads_as_usual() {
    let drc = fixture("eb-quad", "input.drc");
    let mesh = draco::decode(&drc).unwrap();
    let mut doc = compressed_document(&drc, &mesh, 5121);
    // A second buffer with an ordinary TEXCOORD_0 accessor.
    let uvs: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.25, 0.75], [1.0, 1.0]];
    let bytes: Vec<u8> = uvs.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    doc["buffers"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "byteLength": bytes.len(), "uri": data_uri(&bytes) }));
    doc["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "buffer": 1, "byteLength": bytes.len() }));
    doc["accessors"].as_array_mut().unwrap().push(json!({
        "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2",
    }));
    doc["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = json!(2);
    let scene = load(&doc).unwrap();
    let p = &scene.meshes[0].primitives[0];
    assert_primitive_is_mesh(p, &mesh);
    assert_eq!(p.uvs, vec![uvs.to_vec()]);
}

#[test]
fn undecodable_payload_falls_back_to_the_uncompressed_copy() {
    let drc = fixture("eb-quad", "input.drc");
    let mesh = draco::decode(&drc).unwrap();
    // Start from the uncompressed document and add the extension pointing
    // at a payload with a broken magic.
    let mut doc = uncompressed_document(&mesh);
    let mut broken = drc.clone();
    broken[0] = b'X';
    doc["extensionsUsed"] = json!(["KHR_draco_mesh_compression"]);
    doc["buffers"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "byteLength": broken.len(), "uri": data_uri(&broken) }));
    doc["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "buffer": 1, "byteLength": broken.len() }));
    doc["meshes"][0]["primitives"][0]["extensions"] = json!({
        "KHR_draco_mesh_compression": { "bufferView": 2, "attributes": { "POSITION": 0 } }
    });
    let scene = load(&doc).unwrap();
    let p = &scene.meshes[0].primitives[0];
    assert_eq!(p.positions.len(), 4);
    let want: Vec<Vec<f32>> = point_values(&mesh, 0);
    let got: Vec<Vec<f32>> = p.positions.iter().map(|v| v.to_vec()).collect();
    assert_eq!(got, want);
    // Not inflated: the descriptor is kept for round-trip.
    assert!(p.extras.contains_key("KHR_draco_mesh_compression"));

    // With a valid payload the same document inflates instead.
    doc["buffers"][1] = json!({ "byteLength": drc.len(), "uri": data_uri(&drc) });
    let scene = load(&doc).unwrap();
    assert_primitive_is_mesh(&scene.meshes[0].primitives[0], &mesh);
}

#[test]
fn undecodable_payload_without_a_copy_is_an_error() {
    let drc = fixture("eb-quad", "input.drc");
    let mesh = draco::decode(&drc).unwrap();
    type Edit = fn(&mut Vec<u8>);
    let cases: [(&str, Edit); 4] = [
        ("magic", |b| b[4] = b'0'),
        ("version", |b| b[6] = 1),
        ("point cloud", |b| b[7] = 0),
        ("truncated", |b| b.truncate(40)),
    ];
    for (what, edit) in cases {
        let mut bad = drc.clone();
        edit(&mut bad);
        let err = load(&compressed_document(&bad, &mesh, 5121)).unwrap_err();
        assert!(err.contains("KHR_draco_mesh_compression"), "{what}: {err}");
    }
}

#[test]
fn accessor_and_payload_disagreements_are_errors() {
    let drc = fixture("eb-quad", "input.drc");
    let mesh = draco::decode(&drc).unwrap();

    // Accessor count differs from the payload's point count.
    let mut doc = compressed_document(&drc, &mesh, 5121);
    doc["accessors"][1]["count"] = json!(5);
    let err = load(&doc).unwrap_err();
    assert!(err.contains("points"), "{err}");

    // Index count differs from 3 × faces.
    let mut doc = compressed_document(&drc, &mesh, 5121);
    doc["accessors"][0]["count"] = json!(9);
    let err = load(&doc).unwrap_err();
    assert!(err.contains("KHR_draco_mesh_compression"), "{err}");

    // Extension id that no payload attribute carries.
    let mut doc = compressed_document(&drc, &mesh, 5121);
    doc["meshes"][0]["primitives"][0]["extensions"]["KHR_draco_mesh_compression"]["attributes"]["POSITION"] =
        json!(7);
    let err = load(&doc).unwrap_err();
    assert!(err.contains("unique id 7"), "{err}");

    // Accessor shape differs from the payload attribute's.
    let mut doc = compressed_document(&drc, &mesh, 5121);
    doc["accessors"][1]["type"] = json!("VEC2");
    doc["accessors"][1]
        .as_object_mut()
        .unwrap()
        .retain(|k, _| k != "min" && k != "max");
    assert!(load(&doc).is_err());

    // Float payload data declared as an integer accessor: not specified.
    let mut doc = compressed_document(&drc, &mesh, 5121);
    doc["extensionsUsed"]
        .as_array_mut()
        .unwrap()
        .push(json!("KHR_mesh_quantization"));
    doc["accessors"][1]["componentType"] = json!(5123);
    assert!(load(&doc).is_err());
}

#[test]
fn primitive_without_an_indices_accessor_still_gets_the_faces() {
    let drc = fixture("eb-cube", "input.drc");
    let mesh = draco::decode(&drc).unwrap();
    let mut doc = compressed_document(&drc, &mesh, 5121);
    doc["meshes"][0]["primitives"][0]
        .as_object_mut()
        .unwrap()
        .remove("indices");
    let scene = load(&doc).unwrap();
    assert_primitive_is_mesh(&scene.meshes[0].primitives[0], &mesh);
}
