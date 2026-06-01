//! KHR_materials_variants extension — root-level named variants paired
//! with per-primitive mappings that switch a primitive's material at
//! runtime, per `docs/3d/gltf/extensions/KHR_materials_variants.md`.
//!
//! The decoder lifts the root-level variants roster into
//! `Scene3D::extras["KHR_materials_variants"]` and each primitive's
//! mappings list into `Primitive::extras["KHR_materials_variants"]`.
//! The encoder lifts both back into the typed glTF JSON extension
//! blocks and appends `KHR_materials_variants` to `extensionsUsed`.
//! The §3.12 validator rejects data blocks without the declaration
//! and additionally enforces three KHR-spec value-range rules: every
//! mapping's `material` index must resolve, every variant index in a
//! mapping must resolve, and no variant index may appear more than
//! once across a single primitive's mappings.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Material, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, NodeId, Primitive, Scene3D, Topology,
};
use serde_json::{json, Value};

/// Build a minimal `Scene3D` with `material_count` materials, one mesh
/// with one triangle primitive, and an instancing node — sufficient
/// for round-trip exercises that need a valid glTF document plus the
/// variants extension data block.
fn make_scene(material_count: usize) -> Scene3D {
    let mut scene = Scene3D::new();
    for _ in 0..material_count {
        scene.add_material(Material::default());
    }
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(None);
    mesh.primitives.push(prim);
    let mesh_id = scene.add_mesh(mesh);
    let mut node = Node::new();
    node.mesh = Some(mesh_id);
    let node_id = NodeId(scene.nodes.len() as u32);
    scene.add_node(node);
    scene.add_root(node_id);
    scene
}

/// Walk a `.glb` container and return its JSON chunk's payload bytes.
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}

#[test]
fn variants_root_roster_and_mappings_roundtrip_via_glb() {
    let mut scene = make_scene(2);
    // Root-level variants roster on scene.extras.
    scene.extras.insert(
        "KHR_materials_variants".into(),
        json!({
            "variants": [
                { "name": "Red Sneaker" },
                { "name": "Blue Sneaker" },
            ]
        }),
    );
    // Per-primitive mapping table.
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_materials_variants".into(),
        json!({
            "mappings": [
                { "material": 0, "variants": [0] },
                { "material": 1, "variants": [1] },
            ]
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    // Root roster survives.
    let roster = decoded
        .extras
        .get("KHR_materials_variants")
        .expect("root variants roster must round-trip");
    let variants = roster
        .get("variants")
        .and_then(|v| v.as_array())
        .expect("variants array");
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants[0].get("name").and_then(|v| v.as_str()),
        Some("Red Sneaker")
    );
    assert_eq!(
        variants[1].get("name").and_then(|v| v.as_str()),
        Some("Blue Sneaker")
    );

    // Per-primitive mappings survive.
    let prim_map = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_materials_variants")
        .expect("primitive mappings must round-trip");
    let mappings = prim_map
        .get("mappings")
        .and_then(|v| v.as_array())
        .expect("mappings array");
    assert_eq!(mappings.len(), 2);
    assert_eq!(
        mappings[0].get("material").and_then(|v| v.as_u64()),
        Some(0)
    );
    assert_eq!(
        mappings[1].get("material").and_then(|v| v.as_u64()),
        Some(1)
    );
}

#[test]
fn variants_emits_extensions_used_on_encode() {
    let mut scene = make_scene(1);
    scene.extras.insert(
        "KHR_materials_variants".into(),
        json!({ "variants": [{ "name": "Only" }] }),
    );
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_materials_variants".into(),
        json!({ "mappings": [{ "material": 0, "variants": [0] }] }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_materials_variants\""),
        "KHR_materials_variants must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"variants\"") && raw.contains("\"Only\""),
        "root variants roster must round-trip into the JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"mappings\""),
        "per-primitive mappings must round-trip into the JSON, got: {raw}"
    );
}

#[test]
fn variants_extension_omitted_when_no_roster() {
    let scene = make_scene(1);
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_variants"),
        "extension must NOT appear when no variants/mappings present, got: {raw}"
    );
}

#[test]
fn variants_data_without_extensions_used_is_rejected() {
    // Hand-build JSON with both the root variants roster AND a
    // per-primitive mapping but NO `extensionsUsed` declaration — spec
    // §3.12 violation. The validator rejects with
    // `ExtensionStackUsedNotDeclared`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [ {} ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "extensions": {
                            "KHR_materials_variants": {
                                "mappings": [
                                    { "material": 0, "variants": [0] }
                                ]
                            }
                        }
                    }
                ]
            }
        ],
        "extensions": {
            "KHR_materials_variants": {
                "variants": [ { "name": "A" } ]
            }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_variants"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_variants, got {msg}"
    );
}

#[test]
fn variants_data_with_extensions_used_decodes() {
    // Hand-build a document with only the root-level roster declared.
    // The §3.12 validator must accept it now that the declaration is
    // in extensionsUsed.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_variants"],
        "materials": [ {} ],
        "extensions": {
            "KHR_materials_variants": {
                "variants": [ { "name": "Only" } ]
            }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(json)
        .expect("declared extension must decode cleanly");
    let roster = scene.extras.get("KHR_materials_variants").unwrap();
    let arr = roster.get("variants").and_then(|v| v.as_array()).unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].get("name").and_then(|v| v.as_str()), Some("Only"));
}

#[test]
fn variants_out_of_range_variant_index_is_rejected() {
    // Variant index 5 doesn't exist (roster has only one entry).
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_variants"],
        "materials": [ {} ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "extensions": {
                            "KHR_materials_variants": {
                                "mappings": [
                                    { "material": 0, "variants": [5] }
                                ]
                            }
                        }
                    }
                ]
            }
        ],
        "extensions": {
            "KHR_materials_variants": {
                "variants": [ { "name": "A" } ]
            }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("ExtensionStackVariantsIndex"),
        "expected ExtensionStackVariantsIndex, got {err}"
    );
}

#[test]
fn variants_duplicate_variant_index_in_primitive_is_rejected() {
    // Per the spec: "Across the entire mappings array, each variant
    // index must be used no more than one time." Variant 0 appears
    // twice across this primitive's mappings → reject.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_variants"],
        "materials": [ {}, {} ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "extensions": {
                            "KHR_materials_variants": {
                                "mappings": [
                                    { "material": 0, "variants": [0] },
                                    { "material": 1, "variants": [0] }
                                ]
                            }
                        }
                    }
                ]
            }
        ],
        "extensions": {
            "KHR_materials_variants": {
                "variants": [ { "name": "A" }, { "name": "B" } ]
            }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("ExtensionStackVariantsDuplicate"),
        "expected ExtensionStackVariantsDuplicate, got {err}"
    );
}

#[test]
fn variants_full_sneaker_example_roundtrips() {
    // Inspired by docs/3d/gltf/extensions/KHR_materials_variants.md
    // §Example (the sneakers vignette): four variants, one primitive
    // mapping three of them to specific material indices, fourth
    // sharing a material with one of the others.
    let mut scene = make_scene(6);
    scene.extras.insert(
        "KHR_materials_variants".into(),
        json!({
            "variants": [
                { "name": "Yellow Sneaker" },
                { "name": "Red Sneaker" },
                { "name": "Black Sneaker" },
                { "name": "Orange Sneaker" }
            ]
        }),
    );
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_materials_variants".into(),
        json!({
            "mappings": [
                { "material": 2, "variants": [0, 3] },
                { "material": 4, "variants": [1] },
                { "material": 5, "variants": [2] }
            ]
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    let names: Vec<String> = decoded
        .extras
        .get("KHR_materials_variants")
        .and_then(|r| r.get("variants"))
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .map(|v| v.get("name").and_then(|x| x.as_str()).unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "Yellow Sneaker".to_string(),
            "Red Sneaker".to_string(),
            "Black Sneaker".to_string(),
            "Orange Sneaker".to_string(),
        ]
    );

    let mappings = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_materials_variants")
        .and_then(|m| m.get("mappings"))
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(mappings.len(), 3);
    let pair = |i: usize| -> (u64, Vec<u64>) {
        let mat = mappings[i]
            .get("material")
            .and_then(|v| v.as_u64())
            .unwrap();
        let v: Vec<u64> = mappings[i]
            .get("variants")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        (mat, v)
    };
    assert_eq!(pair(0), (2, vec![0, 3]));
    assert_eq!(pair(1), (4, vec![1]));
    assert_eq!(pair(2), (5, vec![2]));
}

#[test]
fn variants_bare_root_object_with_empty_variants_array_roundtrips() {
    // Edge case: an empty roster (variants: []) is legal — the
    // extension acts as a no-op marker. The decoder must surface the
    // empty list rather than silently dropping the extension.
    let mut scene = make_scene(1);
    scene
        .extras
        .insert("KHR_materials_variants".into(), json!({ "variants": [] }));

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let roster = decoded
        .extras
        .get("KHR_materials_variants")
        .expect("empty variants roster must still round-trip");
    let arr = roster.get("variants").and_then(|v| v.as_array()).unwrap();
    assert_eq!(arr.len(), 0);
}

#[test]
fn variants_mapping_name_and_extras_passthrough() {
    // Each mapping entry MAY carry `name` and `extras` per the spec
    // schema. Validate both survive the round-trip.
    let mut scene = make_scene(1);
    scene.extras.insert(
        "KHR_materials_variants".into(),
        json!({ "variants": [{ "name": "Only" }] }),
    );
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_materials_variants".into(),
        json!({
            "mappings": [
                {
                    "material": 0,
                    "variants": [0],
                    "name": "primary-binding",
                    "extras": { "note": "for the demo" }
                }
            ]
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let mappings = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_materials_variants")
        .and_then(|m| m.get("mappings"))
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(
        mappings[0].get("name").and_then(|v| v.as_str()),
        Some("primary-binding")
    );
    assert_eq!(
        mappings[0]
            .get("extras")
            .and_then(|e| e.get("note"))
            .and_then(|v| v.as_str()),
        Some("for the demo")
    );
}

#[test]
fn variants_root_value_is_object_not_array() {
    // Quick sanity check that the encoder emits a JSON object for the
    // root-level extension block (per the spec schema), not the
    // `variants` array directly.
    let mut scene = make_scene(1);
    scene.extras.insert(
        "KHR_materials_variants".into(),
        json!({ "variants": [{ "name": "A" }] }),
    );
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let parsed: Value = serde_json::from_slice(&json_bytes).unwrap();
    let ext = parsed
        .get("extensions")
        .and_then(|e| e.get("KHR_materials_variants"))
        .expect("root extension object expected");
    assert!(ext.is_object(), "root extension MUST be an object");
    assert!(
        ext.get("variants").is_some(),
        "object MUST own `variants` key"
    );
}
