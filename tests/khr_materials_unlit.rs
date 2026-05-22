//! KHR_materials_unlit extension — boolean-flag shading-model selector
//! per `docs/3d/gltf/extensions/KHR_materials_unlit.md`. The extension
//! object is empty (`{}`) on the JSON side; we surface its presence
//! through the typed `Material::extras["KHR_materials_unlit"] = true`
//! side-channel so downstream raster consumers can branch.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::Value;

#[test]
fn unlit_material_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new().with_base_color([0.5, 0.8, 0.0, 1.0]);
    mat.extras
        .insert("KHR_materials_unlit".to_owned(), Value::Bool(true));
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let dm = &decoded.materials[0];
    assert_eq!(
        dm.extras.get("KHR_materials_unlit"),
        Some(&Value::Bool(true)),
        "unlit flag survives round-trip"
    );
    assert_eq!(dm.base_color, [0.5, 0.8, 0.0, 1.0]);
}

#[test]
fn unlit_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras
        .insert("KHR_materials_unlit".to_owned(), Value::Bool(true));
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();

    // Pull the JSON chunk out of the .glb and confirm both the
    // extensionsUsed declaration AND the per-material extension
    // object are present.
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_materials_unlit\""),
        "KHR_materials_unlit must appear in JSON, got: {raw}"
    );
    // The extension object value is `{}` per the spec
    // (see KHR_materials_unlit.md §Extending Materials).
    assert!(
        raw.contains("\"KHR_materials_unlit\":{}"),
        "KHR_materials_unlit extension value must be the literal empty \
         object per spec, got: {raw}"
    );
}

#[test]
fn material_without_unlit_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_unlit"),
        "extension must NOT appear when no material sets the flag, got: {raw}"
    );
}

#[test]
fn unlit_data_block_without_extensions_used_is_rejected() {
    // Hand-build JSON with a per-material extension block but no
    // `extensionsUsed` declaration — spec §3.12 violation, the
    // validator should reject with `ExtensionStackUsedNotDeclared`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "pbrMetallicRoughness": { "baseColorFactor": [1.0, 0.0, 0.0, 1.0] },
                "extensions": { "KHR_materials_unlit": {} }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_unlit"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_unlit, got {msg}"
    );
}

#[test]
fn unlit_data_block_with_extensions_used_decodes() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_unlit"],
        "materials": [
            {
                "pbrMetallicRoughness": { "baseColorFactor": [1.0, 0.0, 0.0, 1.0] },
                "extensions": { "KHR_materials_unlit": {} }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(json)
        .expect("declared extension decodes cleanly");
    assert_eq!(scene.materials.len(), 1);
    assert_eq!(
        scene.materials[0].extras.get("KHR_materials_unlit"),
        Some(&Value::Bool(true)),
        "unlit flag must be surfaced through Material::extras"
    );
}

/// Walk the `.glb` container and return its JSON chunk's payload bytes.
/// Matches the layout from glTF 2.0 spec §4 (12-byte file header,
/// then chunks of `length:u32, type:u32, payload`).
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}
