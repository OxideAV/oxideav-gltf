//! KHR_materials_dispersion extension — adds optical dispersion
//! (chromatic aberration) to the volumetric transmission of a
//! metallic-roughness material per
//! `docs/3d/gltf/extensions/KHR_materials_dispersion.md`. It contributes
//! a single scalar `dispersion` (default `0.0`, range `[0, +∞)` —
//! values above `1.0` are explicitly allowed for artistic exaggeration,
//! Rutile = `2.04` is the spec-listed example). The decoder lifts the
//! full extension object into
//! `Material::extras["KHR_materials_dispersion"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends `KHR_materials_dispersion` to
//! `extensionsUsed`. The §3.12 stack validator additionally enforces
//! `dispersion ≥ 0` and finite (negative or NaN/Inf rejected).
//!
//! Dispersion stores `20/Vd` where `Vd` is the Abbe number (the same
//! transform Adobe Standard Material and ASWF's OpenPBR use). A value
//! of `0` means no dispersion (the backwards-compatibility default).

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn dispersion_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_dispersion")
        .and_then(|v| v.as_object())
}

#[test]
fn dispersion_factor_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_dispersion".to_owned(),
        json!({ "dispersion": 0.36 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = dispersion_of(&decoded.materials[0]).expect("dispersion present");
    let v = obj
        .get("dispersion")
        .and_then(|v| v.as_f64())
        .expect("dispersion present");
    assert!((v - 0.36).abs() < 1e-6, "dispersion round-trips, got {v}");
}

#[test]
fn dispersion_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_dispersion".to_owned(),
        json!({ "dispersion": 0.1 }),
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
        raw.contains("\"KHR_materials_dispersion\""),
        "KHR_materials_dispersion must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"dispersion\""),
        "dispersion field must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_dispersion_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_dispersion"),
        "extension must NOT appear when no material sets dispersion, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_default() {
    // Per the spec the `dispersion` field defaults to `0.0` (no
    // dispersion — backwards-compatibility default). A bare `{}` object
    // resolves to a fully-specified record with the scalar materialised.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_dispersion"],
        "materials": [
            {
                "extensions": { "KHR_materials_dispersion": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = dispersion_of(&scene.materials[0]).expect("dispersion present");

    let v = obj
        .get("dispersion")
        .and_then(|v| v.as_f64())
        .expect("dispersion default materialised");
    assert!(v.abs() < 1e-9, "default dispersion is 0.0, got {v}");
}

#[test]
fn spec_sample_object_decodes() {
    // From the spec §Extending Materials sample (verbatim): a single
    // `dispersion` value of `0.1`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_dispersion"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_dispersion": {
                        "dispersion": 0.1
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = dispersion_of(&scene.materials[0]).expect("dispersion present");
    assert!(
        (obj.get("dispersion").unwrap().as_f64().unwrap() - 0.1).abs() < 1e-6,
        "dispersion survives decode"
    );
}

#[test]
fn dispersion_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_dispersion": { "dispersion": 0.36 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_dispersion"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_dispersion, got {msg}"
    );
}

#[test]
fn dispersion_negative_is_rejected() {
    // Spec §Extending Materials: "Any value zero or larger is
    // considered to be a valid dispersion value". A negative value is
    // out-of-spec and the §3.12 validator must reject it.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_dispersion"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_dispersion": { "dispersion": -0.1 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDispersionRange"),
        "expected ExtensionStackDispersionRange, got {msg}"
    );
}

#[test]
fn dispersion_above_one_passes_through() {
    // The spec explicitly allows dispersion values above `1.0` for
    // artistic exaggeration. The spec table lists Rutile at `2.04`
    // (Vd = 9.8 → 20/9.8 ≈ 2.04) as a realistic high-dispersion
    // material. The validator must accept it.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_dispersion"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_dispersion": {
                        "dispersion": 2.04
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = dispersion_of(&scene.materials[0]).expect("dispersion present");
    let v = obj.get("dispersion").and_then(|v| v.as_f64()).unwrap();
    assert!(
        (v - 2.04).abs() < 1e-6,
        "dispersion > 1.0 passes through, got {v}"
    );
}

#[test]
fn dispersion_zero_explicit_is_accepted() {
    // Zero is the spec default and explicitly valid (means "no
    // dispersion"). A material that explicitly stores `0` round-trips.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_dispersion".to_owned(),
        json!({ "dispersion": 0.0 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dispersion_of(&decoded.materials[0]).expect("dispersion present");
    let v = obj.get("dispersion").and_then(|v| v.as_f64()).unwrap();
    assert!(v.abs() < 1e-9, "dispersion = 0 survives, got {v}");
}

#[test]
fn full_record_roundtrips_through_glb() {
    // End-to-end: build a scene programmatically (Diamond Vd=55, so
    // dispersion = 20/55 ≈ 0.36), round-trip via .glb, check the value
    // survives.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_dispersion".to_owned(),
        json!({ "dispersion": 0.36 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dispersion_of(&decoded.materials[0]).expect("dispersion present");
    assert!((obj.get("dispersion").unwrap().as_f64().unwrap() - 0.36).abs() < 1e-6);
}

#[test]
fn dispersion_coexists_with_volume_and_ior() {
    // Spec §Dependencies: the extension is meant to build on
    // KHR_materials_volume (it modulates the volumetric transmission's
    // IOR per wavelength using the IOR from KHR_materials_ior). All
    // three should be able to land on the same material.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_volume".to_owned(),
        json!({ "thicknessFactor": 1.0, "attenuationColor": [1.0, 1.0, 1.0] }),
    );
    // KHR_materials_ior extras shape is a bare JSON number, not an
    // object — the decoder parks the scalar directly.
    mat.extras
        .insert("KHR_materials_ior".to_owned(), Value::from(2.4_f64));
    mat.extras.insert(
        "KHR_materials_dispersion".to_owned(),
        json!({ "dispersion": 0.36 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(raw.contains("\"KHR_materials_volume\""));
    assert!(raw.contains("\"KHR_materials_ior\""));
    assert!(raw.contains("\"KHR_materials_dispersion\""));

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dispersion_of(&decoded.materials[0]).expect("dispersion present");
    assert!((obj.get("dispersion").unwrap().as_f64().unwrap() - 0.36).abs() < 1e-6);
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
