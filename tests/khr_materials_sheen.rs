//! KHR_materials_sheen extension — layers a sheen BRDF (used to model
//! cloth / fabric) on top of the metallic-roughness material per
//! `docs/3d/gltf/extensions/KHR_materials_sheen.md`. It contributes an
//! RGB `sheenColorFactor` (default `[0, 0, 0]`) and a scalar
//! `sheenRoughnessFactor` (default `0.0`) plus two optional `textureInfo`
//! references (`sheenColorTexture`, `sheenRoughnessTexture`). The decoder
//! lifts the full extension object into
//! `Material::extras["KHR_materials_sheen"]` as a JSON `Value::Object`;
//! the encoder lifts that object back into the typed extensions block on
//! write and appends `KHR_materials_sheen` to `extensionsUsed`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn sheen_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_sheen")
        .and_then(|v| v.as_object())
}

#[test]
fn sheen_factors_roundtrip_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_sheen".to_owned(),
        json!({
            "sheenColorFactor": [0.8, 0.4, 0.1],
            "sheenRoughnessFactor": 0.3
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = sheen_of(&decoded.materials[0]).expect("sheen object present");
    let cf = obj
        .get("sheenColorFactor")
        .and_then(|v| v.as_array())
        .expect("colour factor present");
    let comps: Vec<f64> = cf.iter().filter_map(|v| v.as_f64()).collect();
    assert_eq!(comps.len(), 3);
    assert!(
        (comps[0] - 0.8).abs() < 1e-6,
        "R round-trips, got {comps:?}"
    );
    assert!(
        (comps[1] - 0.4).abs() < 1e-6,
        "G round-trips, got {comps:?}"
    );
    assert!(
        (comps[2] - 0.1).abs() < 1e-6,
        "B round-trips, got {comps:?}"
    );
    let r = obj
        .get("sheenRoughnessFactor")
        .and_then(|v| v.as_f64())
        .expect("roughness factor present");
    assert!(
        (r - 0.3).abs() < 1e-6,
        "sheenRoughnessFactor round-trips, got {r}"
    );
}

#[test]
fn sheen_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_sheen".to_owned(),
        json!({ "sheenColorFactor": [1.0, 1.0, 1.0] }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_materials_sheen\""),
        "KHR_materials_sheen must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"sheenColorFactor\":[1"),
        "the colour factor must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_sheen_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_sheen"),
        "extension must NOT appear when no material sets sheen, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_defaults() {
    // Per the spec the colour factor defaults to `[0, 0, 0]` and the
    // roughness factor to `0.0`, with no textures present; a bare `{}`
    // extension object resolves to those. (The spec also notes that a
    // zero `sheenColorFactor` disables the whole sheen layer.)
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_sheen"],
        "materials": [
            {
                "extensions": { "KHR_materials_sheen": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = sheen_of(&scene.materials[0]).expect("sheen present");
    let cf = obj
        .get("sheenColorFactor")
        .and_then(|v| v.as_array())
        .expect("colour default materialised");
    let comps: Vec<f64> = cf.iter().filter_map(|v| v.as_f64()).collect();
    assert_eq!(
        comps,
        vec![0.0, 0.0, 0.0],
        "default sheenColorFactor is [0,0,0]"
    );
    let r = obj
        .get("sheenRoughnessFactor")
        .and_then(|v| v.as_f64())
        .expect("roughness default materialised");
    assert!(
        r.abs() < 1e-9,
        "default sheenRoughnessFactor is 0.0, got {r}"
    );
    assert!(!obj.contains_key("sheenColorTexture"));
    assert!(!obj.contains_key("sheenRoughnessTexture"));
}

#[test]
fn explicit_sheen_factors_decode() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_sheen"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_sheen": {
                        "sheenColorFactor": [0.65, 0.50, 0.35],
                        "sheenRoughnessFactor": 0.12
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = sheen_of(&scene.materials[0]).expect("sheen present");
    let cf = obj.get("sheenColorFactor").unwrap().as_array().unwrap();
    assert!((cf[0].as_f64().unwrap() - 0.65).abs() < 1e-6);
    assert!((cf[1].as_f64().unwrap() - 0.50).abs() < 1e-6);
    assert!((cf[2].as_f64().unwrap() - 0.35).abs() < 1e-6);
    assert!((obj.get("sheenRoughnessFactor").unwrap().as_f64().unwrap() - 0.12).abs() < 1e-6);
}

#[test]
fn sheen_textures_round_trip_with_texcoord() {
    // Build a JSON document carrying both sheen textures:
    // `sheenColorTexture` (default texCoord omitted) and
    // `sheenRoughnessTexture` (texCoord 1). Verify both indices and the
    // explicit `texCoord` survive a decode->encode->decode cycle. Both
    // are plain `textureInfo` so neither carries a `scale`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_sheen"],
        "images": [
            { "uri": "data:image/png;base64,AAAA" },
            { "uri": "data:image/png;base64,AAAA" }
        ],
        "samplers": [{}],
        "textures": [
            { "source": 0, "sampler": 0 },
            { "source": 1, "sampler": 0 }
        ],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_sheen": {
                        "sheenColorTexture": { "index": 0 },
                        "sheenRoughnessTexture": { "index": 1, "texCoord": 1 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = sheen_of(&scene.materials[0]).expect("sheen present");

    let sct = obj
        .get("sheenColorTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(sct.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(sct.get("texCoord").is_none(), "default texCoord 0 omitted");

    let srt = obj
        .get("sheenRoughnessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(srt.get("index").unwrap().as_u64().unwrap(), 1);
    assert_eq!(srt.get("texCoord").unwrap().as_u64().unwrap(), 1);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(raw.contains("\"sheenColorTexture\""), "got: {raw}");
    assert!(raw.contains("\"sheenRoughnessTexture\""), "got: {raw}");

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = sheen_of(&decoded.materials[0]).expect("sheen present");
    let sct2 = obj2
        .get("sheenColorTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(sct2.get("index").unwrap().as_u64().unwrap(), 0);
    let srt2 = obj2
        .get("sheenRoughnessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(srt2.get("index").unwrap().as_u64().unwrap(), 1);
    assert_eq!(srt2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn sheen_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_sheen": { "sheenColorFactor": [0.5, 0.5, 0.5] }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_sheen"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_sheen, got {msg}"
    );
}

/// Walk the `.glb` container and return its JSON chunk's payload bytes.
/// Matches the layout from glTF 2.0 spec §4 (12-byte file header, then
/// chunks of `length:u32, type:u32, payload`).
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}
