//! KHR_xmp_json_ld extension — XMP (ISO 16684-1) metadata indirection
//! per `docs/3d/gltf/extensions/KHR_xmp_json_ld.md`. The extension
//! defines a root-level `packets[]` roster (§"Defining XMP Metadata")
//! plus a `{ "packet": N }` indirection emitted on the `asset`,
//! `scene`, `node`, `mesh`, or `material` object (§"Instantiating XMP
//! metadata"). The metadata content itself is opaque JSON-LD held
//! verbatim — the spec specifies a restricted JSON-LD subset
//! (§"Restrictions and Recommendations") without pinning the namespace
//! vocabulary, so byte-equality on the round-trip is the strong check.
//!
//! Decoder/encoder side-channels through `Scene3D::extras`:
//!
//! * root `packets[]` roster → `scene.extras["KHR_xmp_json_ld"] =
//!   { "packets": [...] }`
//! * asset packet ref       → `scene.extras["__asset_xmp_packet"] = N`
//! * primary-scene packet ref → `scene.extras["__primary_scene_xmp_packet"] = N`
//! * node packet ref         → `node.extras["KHR_xmp_json_ld"] = N`
//! * mesh packet ref         → `primitive[0].extras["__mesh_xmp_packet"] = N`
//! * material packet ref     → `material.extras["KHR_xmp_json_ld"] = N`

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D};
use serde_json::{json, Value};

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}

fn scene_with_packets(packets: Vec<Value>) -> Scene3D {
    let mut scene = Scene3D::new();
    scene
        .extras
        .insert("KHR_xmp_json_ld".to_owned(), json!({ "packets": packets }));
    scene
}

fn sample_packet() -> Value {
    json!({
        "@context": {
            "dc": "http://purl.org/dc/elements/1.1/"
        },
        "@id": "",
        "dc:identifier": "urn:stock-id:292930"
    })
}

#[test]
fn asset_packet_round_trips_via_glb() {
    let mut scene = scene_with_packets(vec![sample_packet()]);
    scene
        .extras
        .insert("__asset_xmp_packet".to_owned(), json!(0u32));

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(raw.contains("\"KHR_xmp_json_ld\""));
    assert!(raw.contains("\"extensionsUsed\""));
    assert!(raw.contains("\"asset\""));
    assert!(raw.contains("\"packet\":0"));

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(
        decoded.extras.get("__asset_xmp_packet"),
        Some(&json!(0u32)),
        "asset packet ref survives round-trip"
    );
    let root = decoded.extras.get("KHR_xmp_json_ld").unwrap();
    let packets = root.get("packets").and_then(|v| v.as_array()).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(
        packets[0],
        sample_packet(),
        "packet content preserved byte-for-byte"
    );
}

#[test]
fn node_packet_round_trips_via_glb() {
    let mut scene = scene_with_packets(vec![sample_packet()]);
    let mut node = Node::new();
    node.extras
        .insert("KHR_xmp_json_ld".to_owned(), json!(0u32));
    scene.add_node(node);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.nodes.len(), 1);
    assert_eq!(
        decoded.nodes[0].extras.get("KHR_xmp_json_ld"),
        Some(&json!(0u32)),
        "node packet ref surfaces through Node::extras"
    );
}

#[test]
fn material_packet_round_trips_via_glb() {
    let mut scene = scene_with_packets(vec![sample_packet()]);
    let mut mat = Material::new();
    mat.extras.insert("KHR_xmp_json_ld".to_owned(), json!(0u32));
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.materials.len(), 1);
    assert_eq!(
        decoded.materials[0].extras.get("KHR_xmp_json_ld"),
        Some(&json!(0u32))
    );
}

#[test]
fn mesh_packet_round_trips_via_glb() {
    // Spec's primary §"Instantiating XMP metadata" example uses a mesh.
    let mut scene = scene_with_packets(vec![sample_packet()]);
    let mut mesh = Mesh::new(Some("xmp-tagged".to_owned()));
    let mut prim = Primitive::new(oxideav_mesh3d::Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    prim.extras
        .insert("__mesh_xmp_packet".to_owned(), json!(0u32));
    mesh.primitives.push(prim);
    scene.add_mesh(mesh);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    // The typed Mesh.extensions.KHR_xmp_json_ld block must appear, not
    // a surplus extras key.
    assert!(raw.contains("\"meshes\""), "meshes array must be emitted");
    assert!(raw.contains("\"KHR_xmp_json_ld\""));

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.meshes.len(), 1);
    let p0 = &decoded.meshes[0].primitives[0];
    assert_eq!(
        p0.extras.get("__mesh_xmp_packet"),
        Some(&json!(0u32)),
        "mesh packet ref re-stashed on primitive[0] sentinel"
    );
}

#[test]
fn data_block_without_extensions_used_is_rejected() {
    // Hand-built JSON with a root-level KHR_xmp_json_ld packets roster
    // and no extensionsUsed declaration — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensions": {
            "KHR_xmp_json_ld": {
                "packets": [ { "@context": {}, "@id": "" } ]
            }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_xmp_json_ld"),
        "expected ExtensionStackUsedNotDeclared for KHR_xmp_json_ld, got: {msg}"
    );
}

#[test]
fn packet_index_out_of_range_is_rejected() {
    // Per the spec's indirection model, every `{ "packet": N }` ref
    // must resolve to a slot in `packets[]`. N >= packets.len() must
    // surface as `ExtensionStackXmpPacketIndex`.
    let json = br#"{
        "asset": {
            "version": "2.0",
            "extensions": { "KHR_xmp_json_ld": { "packet": 5 } }
        },
        "extensionsUsed": ["KHR_xmp_json_ld"],
        "extensions": {
            "KHR_xmp_json_ld": {
                "packets": [ { "@context": {}, "@id": "" } ]
            }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackXmpPacketIndex"),
        "expected ExtensionStackXmpPacketIndex, got: {msg}"
    );
}

#[test]
fn bare_packets_roster_without_refs_round_trips() {
    // A document MAY define `packets[]` without any per-object refs
    // (declarations only, possibly future-use). The encoder must still
    // declare the extension and the round-trip must preserve packet
    // content byte-for-byte.
    let packets = vec![
        sample_packet(),
        json!({
            "@context": { "dc": "http://purl.org/dc/elements/1.1/" },
            "@id": "",
            "dc:rights": "All rights reserved"
        }),
    ];
    let scene = scene_with_packets(packets.clone());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let got = decoded.extras.get("KHR_xmp_json_ld").unwrap();
    let got_packets = got.get("packets").and_then(|v| v.as_array()).unwrap();
    assert_eq!(got_packets.len(), 2);
    for (a, b) in packets.iter().zip(got_packets.iter()) {
        assert_eq!(a, b, "each packet byte-preserved");
    }
}

#[test]
fn primary_scene_packet_ref_round_trips_via_glb() {
    let mut scene = scene_with_packets(vec![sample_packet()]);
    scene
        .extras
        .insert("__primary_scene_xmp_packet".to_owned(), json!(0u32));

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(raw.contains("\"scenes\""));
    assert!(raw.contains("\"packet\":0"));

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(
        decoded.extras.get("__primary_scene_xmp_packet"),
        Some(&json!(0u32)),
        "primary scene packet ref survives round-trip"
    );
}
