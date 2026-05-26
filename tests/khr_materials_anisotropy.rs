//! KHR_materials_anisotropy extension — adds an asymmetric specular lobe
//! (the elongated highlight visible on e.g. brushed metal) on top of the
//! metallic-roughness material per
//! `docs/3d/gltf/extensions/KHR_materials_anisotropy.md`. It contributes
//! a scalar `anisotropyStrength` (default `0.0`, range `[0, 1]`; zero
//! disables the whole effect), a scalar `anisotropyRotation` (default
//! `0.0` radians, counter-clockwise from the tangent), and an optional
//! `anisotropyTexture` whose red+green channels encode the direction
//! vector in `[-1, 1]` tangent / bitangent space and whose blue channel
//! carries strength in `[0, 1]`. The decoder lifts the full extension
//! object into `Material::extras["KHR_materials_anisotropy"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends `KHR_materials_anisotropy` to
//! `extensionsUsed`. The §3.12 stack validator additionally enforces the
//! spec's `anisotropyStrength ∈ [0, 1]` range.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn anisotropy_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_anisotropy")
        .and_then(|v| v.as_object())
}

#[test]
fn anisotropy_factor_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_anisotropy".to_owned(),
        json!({ "anisotropyStrength": 0.75 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = anisotropy_of(&decoded.materials[0]).expect("anisotropy present");
    let strength = obj
        .get("anisotropyStrength")
        .and_then(|v| v.as_f64())
        .expect("strength present");
    assert!(
        (strength - 0.75).abs() < 1e-6,
        "anisotropyStrength round-trips, got {strength}"
    );
}

#[test]
fn anisotropy_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_anisotropy".to_owned(),
        json!({
            "anisotropyStrength": 0.6,
            "anisotropyRotation": 1.57
        }),
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
        raw.contains("\"KHR_materials_anisotropy\""),
        "KHR_materials_anisotropy must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"anisotropyStrength\""),
        "anisotropyStrength must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"anisotropyRotation\""),
        "anisotropyRotation must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_anisotropy_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_anisotropy"),
        "extension must NOT appear when no material sets anisotropy, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_defaults() {
    // Per the spec the anisotropy strength defaults to `0.0` (disabled)
    // and the anisotropy rotation defaults to `0.0` radians (§Extending
    // Materials). A bare `{}` extension object resolves to a
    // fully-specified record with both scalars materialised.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "materials": [
            {
                "extensions": { "KHR_materials_anisotropy": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = anisotropy_of(&scene.materials[0]).expect("anisotropy present");

    let strength = obj
        .get("anisotropyStrength")
        .and_then(|v| v.as_f64())
        .expect("anisotropyStrength default materialised");
    assert!(
        strength.abs() < 1e-9,
        "default anisotropyStrength is 0.0, got {strength}"
    );

    let rotation = obj
        .get("anisotropyRotation")
        .and_then(|v| v.as_f64())
        .expect("anisotropyRotation default materialised");
    assert!(
        rotation.abs() < 1e-9,
        "default anisotropyRotation is 0.0, got {rotation}"
    );

    assert!(!obj.contains_key("anisotropyTexture"));
}

#[test]
fn spec_sample_object_decodes() {
    // From the spec §Extending Materials sample (verbatim values):
    // anisotropyStrength 0.6, anisotropyRotation 1.57,
    // anisotropyTexture index 0.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "images": [
            { "uri": "data:image/png;base64,AAAA" }
        ],
        "samplers": [{}],
        "textures": [
            { "source": 0, "sampler": 0 }
        ],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": {
                        "anisotropyStrength": 0.6,
                        "anisotropyRotation": 1.57,
                        "anisotropyTexture": { "index": 0 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = anisotropy_of(&scene.materials[0]).expect("anisotropy present");
    assert!(
        (obj.get("anisotropyStrength").unwrap().as_f64().unwrap() - 0.6).abs() < 1e-6,
        "anisotropyStrength survives decode"
    );
    assert!(
        (obj.get("anisotropyRotation").unwrap().as_f64().unwrap() - 1.57).abs() < 1e-6,
        "anisotropyRotation survives decode"
    );
    let tex = obj
        .get("anisotropyTexture")
        .and_then(|v| v.as_object())
        .expect("anisotropyTexture present");
    assert_eq!(tex.get("index").unwrap().as_u64().unwrap(), 0);
}

#[test]
fn anisotropy_texture_round_trips_with_texcoord() {
    // Build a JSON document carrying an `anisotropyTexture` with a
    // non-default texCoord. Verify both the index and the explicit
    // `texCoord` survive a decode->encode->decode cycle. It is a plain
    // `textureInfo` so it carries no `scale`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "images": [
            { "uri": "data:image/png;base64,AAAA" }
        ],
        "samplers": [{}],
        "textures": [
            { "source": 0, "sampler": 0 }
        ],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": {
                        "anisotropyStrength": 1.0,
                        "anisotropyTexture": { "index": 0, "texCoord": 1 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = anisotropy_of(&scene.materials[0]).expect("anisotropy present");

    let at = obj
        .get("anisotropyTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(at.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(at.get("texCoord").unwrap().as_u64().unwrap(), 1);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(
        raw.contains("\"anisotropyTexture\""),
        "anisotropyTexture must be emitted, got: {raw}"
    );

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = anisotropy_of(&decoded.materials[0]).expect("anisotropy present");
    let at2 = obj2
        .get("anisotropyTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(at2.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(at2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn anisotropy_texture_default_texcoord_omitted() {
    // textureInfo without an explicit `texCoord` round-trips with the
    // field absent (encoder must NOT spuriously emit `texCoord: 0`).
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "images": [
            { "uri": "data:image/png;base64,AAAA" }
        ],
        "samplers": [{}],
        "textures": [
            { "source": 0, "sampler": 0 }
        ],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": {
                        "anisotropyStrength": 0.5,
                        "anisotropyTexture": { "index": 0 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = anisotropy_of(&scene.materials[0]).expect("anisotropy present");
    let at = obj
        .get("anisotropyTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(at.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(at.get("texCoord").is_none(), "default texCoord 0 omitted");
}

#[test]
fn anisotropy_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": { "anisotropyStrength": 0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_anisotropy"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_anisotropy, got {msg}"
    );
}

#[test]
fn anisotropy_strength_outside_unit_range_is_rejected() {
    // Spec §Anisotropy: `anisotropyStrength` is "a dimensionless number
    // in the range [0, 1]". `1.5` is out-of-spec and the §3.12 validator
    // must reject it.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": { "anisotropyStrength": 1.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnisotropyStrengthRange"),
        "expected ExtensionStackAnisotropyStrengthRange, got {msg}"
    );
}

#[test]
fn anisotropy_strength_negative_is_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": { "anisotropyStrength": -0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnisotropyStrengthRange"),
        "expected ExtensionStackAnisotropyStrengthRange, got {msg}"
    );
}

#[test]
fn full_record_roundtrips_through_glb() {
    // End-to-end: build a scene programmatically with both scalars +
    // texture reference, round-trip via .glb, and check every value
    // survives.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_anisotropy".to_owned(),
        json!({
            "anisotropyStrength": 0.25,
            "anisotropyRotation": 0.785
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = anisotropy_of(&decoded.materials[0]).expect("anisotropy present");

    assert!((obj.get("anisotropyStrength").unwrap().as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert!((obj.get("anisotropyRotation").unwrap().as_f64().unwrap() - 0.785).abs() < 1e-5);
}

#[test]
fn anisotropy_rotation_can_exceed_two_pi() {
    // The spec only says rotation is in radians counter-clockwise from
    // the tangent — it gives no upper bound (any value is interpreted
    // modulo 2π). The validator must not reject values like 7.0.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_anisotropy"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_anisotropy": {
                        "anisotropyRotation": 7.0
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = anisotropy_of(&scene.materials[0]).expect("anisotropy present");
    let r = obj
        .get("anisotropyRotation")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!((r - 7.0).abs() < 1e-6, "rotation passes through, got {r}");
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
