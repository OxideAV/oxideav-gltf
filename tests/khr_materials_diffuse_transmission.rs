//! KHR_materials_diffuse_transmission extension — models light that
//! diffuses through infinitely-thin surfaces (leaves, paper, candle
//! wax …) on top of the metallic-roughness material per
//! `docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md`. The
//! extension carries:
//!
//! * `diffuseTransmissionFactor` (default `0.0`, range `[0, 1]`) — the
//!   percentage of non-specularly-reflected light that is diffusely
//!   transmitted through the surface.
//! * `diffuseTransmissionTexture` (a `textureInfo`) — alpha-channel
//!   multiplier on `diffuseTransmissionFactor`.
//! * `diffuseTransmissionColorFactor` (default `[1, 1, 1]`, range
//!   `[0, 1]^3` per channel) — colour that modulates the transmitted
//!   light.
//! * `diffuseTransmissionColorTexture` (a `textureInfo`) — sRGB RGB
//!   multiplier on `diffuseTransmissionColorFactor`.
//!
//! The decoder lifts the full extension object into
//! `Material::extras["KHR_materials_diffuse_transmission"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends
//! `KHR_materials_diffuse_transmission` to `extensionsUsed`. The §3.12
//! stack validator additionally enforces the `[0, 1]` range on the
//! factor and on each colour component (finite, non-negative, ≤ 1).

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn dt_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_diffuse_transmission")
        .and_then(|v| v.as_object())
}

#[test]
fn diffuse_transmission_factor_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_diffuse_transmission".to_owned(),
        json!({ "diffuseTransmissionFactor": 0.25 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = dt_of(&decoded.materials[0]).expect("diffuse_transmission present");
    let v = obj
        .get("diffuseTransmissionFactor")
        .and_then(|v| v.as_f64())
        .expect("factor present");
    assert!(
        (v - 0.25).abs() < 1e-6,
        "diffuseTransmissionFactor round-trips, got {v}"
    );
}

#[test]
fn diffuse_transmission_color_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    // Spec §Extending Materials sample colour: [1.0, 0.9, 0.85] (a
    // warm tan tint).
    mat.extras.insert(
        "KHR_materials_diffuse_transmission".to_owned(),
        json!({
            "diffuseTransmissionFactor": 0.25,
            "diffuseTransmissionColorFactor": [1.0, 0.9, 0.85]
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dt_of(&decoded.materials[0]).expect("diffuse_transmission present");
    let arr = obj
        .get("diffuseTransmissionColorFactor")
        .and_then(|v| v.as_array())
        .expect("color present");
    assert_eq!(arr.len(), 3);
    assert!((arr[0].as_f64().unwrap() - 1.0).abs() < 1e-6);
    assert!((arr[1].as_f64().unwrap() - 0.9).abs() < 1e-6);
    assert!((arr[2].as_f64().unwrap() - 0.85).abs() < 1e-6);
}

#[test]
fn diffuse_transmission_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_diffuse_transmission".to_owned(),
        json!({ "diffuseTransmissionFactor": 0.5 }),
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
        raw.contains("\"KHR_materials_diffuse_transmission\""),
        "KHR_materials_diffuse_transmission must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"diffuseTransmissionFactor\""),
        "diffuseTransmissionFactor field must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_diffuse_transmission_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_diffuse_transmission"),
        "extension must NOT appear when no material sets it, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_default() {
    // Per the spec the `diffuseTransmissionFactor` defaults to `0.0`
    // and `diffuseTransmissionColorFactor` to `[1, 1, 1]`. A bare `{}`
    // object resolves to a fully-specified record with the defaults
    // materialised.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_diffuse_transmission"],
        "materials": [
            {
                "extensions": { "KHR_materials_diffuse_transmission": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = dt_of(&scene.materials[0]).expect("dt present");

    let f = obj
        .get("diffuseTransmissionFactor")
        .and_then(|v| v.as_f64())
        .expect("factor default materialised");
    assert!(f.abs() < 1e-9, "default factor is 0.0, got {f}");

    let arr = obj
        .get("diffuseTransmissionColorFactor")
        .and_then(|v| v.as_array())
        .expect("color default materialised");
    assert_eq!(arr.len(), 3);
    for c in arr {
        assert!(
            (c.as_f64().unwrap() - 1.0).abs() < 1e-9,
            "default color is 1.0"
        );
    }
}

#[test]
fn spec_sample_object_decodes() {
    // Verbatim from the spec §Extending Materials sample, minus the
    // textureInfo which would need a `textures[]` entry to round-trip
    // through the decoder.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_diffuse_transmission"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_diffuse_transmission": {
                        "diffuseTransmissionFactor": 0.25,
                        "diffuseTransmissionColorFactor": [1.0, 0.9, 0.85]
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = dt_of(&scene.materials[0]).expect("dt present");
    assert!(
        (obj.get("diffuseTransmissionFactor")
            .unwrap()
            .as_f64()
            .unwrap()
            - 0.25)
            .abs()
            < 1e-6,
        "factor survives decode"
    );
    let arr = obj
        .get("diffuseTransmissionColorFactor")
        .and_then(|v| v.as_array())
        .expect("color present");
    assert!((arr[0].as_f64().unwrap() - 1.0).abs() < 1e-6);
    assert!((arr[1].as_f64().unwrap() - 0.9).abs() < 1e-6);
    assert!((arr[2].as_f64().unwrap() - 0.85).abs() < 1e-6);
}

#[test]
fn diffuse_transmission_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_diffuse_transmission": {
                        "diffuseTransmissionFactor": 0.25
                    }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared")
            && msg.contains("KHR_materials_diffuse_transmission"),
        "expected ExtensionStackUsedNotDeclared for \
         KHR_materials_diffuse_transmission, got {msg}"
    );
}

#[test]
fn diffuse_transmission_factor_above_one_is_rejected() {
    // Spec: "A value of 1.0 indicates that 100% of the light that
    // penetrates the surface is transmitted through it." A factor
    // above 1.0 is non-sensical — the validator rejects it.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_diffuse_transmission"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_diffuse_transmission": {
                        "diffuseTransmissionFactor": 1.5
                    }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDiffuseTransmissionFactorRange"),
        "expected ExtensionStackDiffuseTransmissionFactorRange, got {msg}"
    );
}

#[test]
fn diffuse_transmission_factor_negative_is_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_diffuse_transmission"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_diffuse_transmission": {
                        "diffuseTransmissionFactor": -0.1
                    }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDiffuseTransmissionFactorRange"),
        "expected ExtensionStackDiffuseTransmissionFactorRange, got {msg}"
    );
}

#[test]
fn diffuse_transmission_color_out_of_range_is_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_diffuse_transmission"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_diffuse_transmission": {
                        "diffuseTransmissionColorFactor": [1.0, 2.0, 1.0]
                    }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDiffuseTransmissionColorRange"),
        "expected ExtensionStackDiffuseTransmissionColorRange, got {msg}"
    );
}

#[test]
fn diffuse_transmission_zero_explicit_is_accepted() {
    // Zero is the spec default and explicitly valid. A material that
    // explicitly stores `0` round-trips.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_diffuse_transmission".to_owned(),
        json!({ "diffuseTransmissionFactor": 0.0 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dt_of(&decoded.materials[0]).expect("dt present");
    let v = obj
        .get("diffuseTransmissionFactor")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!(v.abs() < 1e-9, "factor = 0 survives, got {v}");
}

#[test]
fn full_record_roundtrips_through_glb() {
    // End-to-end: build a scene programmatically with all four
    // simple-typed fields (factor + colour), round-trip via .glb, check
    // every value survives.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_diffuse_transmission".to_owned(),
        json!({
            "diffuseTransmissionFactor": 0.6,
            "diffuseTransmissionColorFactor": [0.4, 0.7, 0.2]
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dt_of(&decoded.materials[0]).expect("dt present");
    assert!(
        (obj.get("diffuseTransmissionFactor")
            .unwrap()
            .as_f64()
            .unwrap()
            - 0.6)
            .abs()
            < 1e-6
    );
    let arr = obj
        .get("diffuseTransmissionColorFactor")
        .and_then(|v| v.as_array())
        .unwrap();
    assert!((arr[0].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert!((arr[1].as_f64().unwrap() - 0.7).abs() < 1e-6);
    assert!((arr[2].as_f64().unwrap() - 0.2).abs() < 1e-6);
}

#[test]
fn diffuse_transmission_coexists_with_volume_and_transmission() {
    // Spec §Combining ... §KHR_materials_volume: the extension is
    // explicitly designed to combine with KHR_materials_volume (for
    // translucent objects) and KHR_materials_transmission (which
    // overrides the diffuse path). All three must coexist on a
    // material.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_volume".to_owned(),
        json!({ "thicknessFactor": 1.0, "attenuationColor": [1.0, 1.0, 1.0] }),
    );
    mat.extras.insert(
        "KHR_materials_transmission".to_owned(),
        json!({ "transmissionFactor": 0.5 }),
    );
    mat.extras.insert(
        "KHR_materials_diffuse_transmission".to_owned(),
        json!({
            "diffuseTransmissionFactor": 1.0,
            "diffuseTransmissionColorFactor": [0.9, 0.95, 0.85]
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(raw.contains("\"KHR_materials_volume\""));
    assert!(raw.contains("\"KHR_materials_transmission\""));
    assert!(raw.contains("\"KHR_materials_diffuse_transmission\""));

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = dt_of(&decoded.materials[0]).expect("dt present");
    assert!(
        (obj.get("diffuseTransmissionFactor")
            .unwrap()
            .as_f64()
            .unwrap()
            - 1.0)
            .abs()
            < 1e-6
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
