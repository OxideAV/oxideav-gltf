//! KHR_materials_ior extension — a scalar index of refraction that
//! overrides the metallic-roughness dielectric BRDF's fixed 1.5 per
//! `docs/3d/gltf/extensions/KHR_materials_ior.md`. The extension object
//! carries a single optional `ior` number (default 1.5); we surface it
//! through the typed `Material::extras["KHR_materials_ior"]`
//! side-channel as a JSON number so downstream raster consumers can
//! refract dielectrics without us widening `oxideav_mesh3d::Material`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::Value;

fn ior_of(m: &Material) -> Option<f64> {
    m.extras.get("KHR_materials_ior").and_then(|v| v.as_f64())
}

#[test]
fn ior_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras
        .insert("KHR_materials_ior".to_owned(), Value::from(1.33_f64));
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let dm = &decoded.materials[0];
    // Stored as f32 then promoted back to f64, so compare with an
    // f32-epsilon tolerance rather than exact bit equality.
    let got = ior_of(dm).expect("ior present after round-trip");
    assert!(
        (got - 1.33).abs() < 1e-6,
        "ior survives round-trip, got {got}"
    );
}

#[test]
fn ior_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras
        .insert("KHR_materials_ior".to_owned(), Value::from(2.42_f64));
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_materials_ior\""),
        "KHR_materials_ior must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"ior\":2.42"),
        "the scalar ior must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_ior_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_ior"),
        "extension must NOT appear when no material sets an ior, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_default_ior() {
    // Per the spec, `ior` is optional with a default of 1.5 — so a bare
    // `{}` extension object resolves to 1.5.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_ior"],
        "materials": [
            {
                "extensions": { "KHR_materials_ior": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.materials.len(), 1);
    assert_eq!(
        ior_of(&scene.materials[0]),
        Some(1.5),
        "bare extension object must default to 1.5"
    );
}

#[test]
fn explicit_ior_decodes() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_ior"],
        "materials": [
            {
                "extensions": { "KHR_materials_ior": { "ior": 1.76 } }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let got = ior_of(&scene.materials[0]).expect("ior present");
    assert!((got - 1.76).abs() < 1e-6, "explicit ior decodes, got {got}");
}

#[test]
fn special_zero_ior_sentinel_decodes() {
    // The spec reserves `ior == 0` as the specular-glossiness
    // backwards-compatibility sentinel (effective IOR → +inf). We carry
    // the literal value through; we do not coerce it to the default.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_ior"],
        "materials": [
            {
                "extensions": { "KHR_materials_ior": { "ior": 0.0 } }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(
        ior_of(&scene.materials[0]),
        Some(0.0),
        "the ior == 0 spec-glossiness sentinel must survive decode"
    );
}

#[test]
fn ior_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": { "KHR_materials_ior": { "ior": 1.4 } }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_ior"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_ior, got {msg}"
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
