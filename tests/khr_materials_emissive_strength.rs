//! KHR_materials_emissive_strength extension — a scalar multiplier on
//! the core material's emissive value per
//! `docs/3d/gltf/extensions/KHR_materials_emissive_strength.md`. The
//! extension object carries a single optional `emissiveStrength` number
//! (default 1.0); we surface it through the typed
//! `Material::extras["KHR_materials_emissive_strength"]` side-channel as
//! a JSON number so downstream raster consumers can amplify emission
//! without us widening `oxideav_mesh3d::Material`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::Value;

fn strength_of(m: &Material) -> Option<f64> {
    m.extras
        .get("KHR_materials_emissive_strength")
        .and_then(|v| v.as_f64())
}

#[test]
fn emissive_strength_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.emissive_factor = [1.0, 1.0, 1.0];
    mat.extras.insert(
        "KHR_materials_emissive_strength".to_owned(),
        Value::from(5.0_f64),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let dm = &decoded.materials[0];
    assert_eq!(
        strength_of(dm),
        Some(5.0),
        "emissiveStrength survives round-trip"
    );
    assert_eq!(dm.emissive_factor, [1.0, 1.0, 1.0]);
}

#[test]
fn emissive_strength_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_emissive_strength".to_owned(),
        Value::from(2.5_f64),
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
        raw.contains("\"KHR_materials_emissive_strength\""),
        "KHR_materials_emissive_strength must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"emissiveStrength\":2.5"),
        "the scalar emissiveStrength must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_emissive_strength_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_emissive_strength"),
        "extension must NOT appear when no material sets a strength, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_default_strength() {
    // Per the spec §Parameters, `emissiveStrength` is optional with a
    // default of 1.0 — so a bare `{}` extension object resolves to 1.0.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_emissive_strength"],
        "materials": [
            {
                "emissiveFactor": [1.0, 1.0, 1.0],
                "extensions": { "KHR_materials_emissive_strength": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.materials.len(), 1);
    assert_eq!(
        strength_of(&scene.materials[0]),
        Some(1.0),
        "bare extension object must default to 1.0"
    );
}

#[test]
fn explicit_strength_decodes() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_emissive_strength"],
        "materials": [
            {
                "emissiveFactor": [1.0, 1.0, 1.0],
                "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 5.0 } }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(strength_of(&scene.materials[0]), Some(5.0));
}

#[test]
fn emissive_strength_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "emissiveFactor": [1.0, 1.0, 1.0],
                "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 5.0 } }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared")
            && msg.contains("KHR_materials_emissive_strength"),
        "expected ExtensionStackUsedNotDeclared for \
         KHR_materials_emissive_strength, got {msg}"
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
