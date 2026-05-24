//! KHR_materials_volume extension — turns the surface into the boundary
//! of a homogeneous volumetric medium (refraction + absorption), per
//! `docs/3d/gltf/extensions/KHR_materials_volume.md`. It contributes a
//! scalar `thicknessFactor` (default `0.0`, a value of zero means the
//! material is thin-walled), an optional `thicknessTexture` (a
//! `textureInfo`, sampled from the `.g` channel), a scalar
//! `attenuationDistance` (default `+Infinity`, expressed in world space),
//! and an RGB `attenuationColor` (default `[1, 1, 1]`). The decoder lifts
//! the full extension object into `Material::extras["KHR_materials_volume"]`
//! as a JSON `Value::Object`; the encoder lifts that object back into the
//! typed extensions block on write and appends `KHR_materials_volume` to
//! `extensionsUsed`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn volume_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_volume")
        .and_then(|v| v.as_object())
}

#[test]
fn volume_thickness_factor_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_volume".to_owned(),
        json!({ "thicknessFactor": 0.42 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = volume_of(&decoded.materials[0]).expect("volume object present");
    let tf = obj
        .get("thicknessFactor")
        .and_then(|v| v.as_f64())
        .expect("factor present");
    assert!(
        (tf - 0.42).abs() < 1e-6,
        "thicknessFactor round-trips, got {tf}"
    );
}

#[test]
fn volume_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_volume".to_owned(),
        json!({
            "thicknessFactor": 1.0,
            "attenuationDistance": 0.006,
            "attenuationColor": [0.5, 0.5, 0.5]
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
        raw.contains("\"KHR_materials_volume\""),
        "KHR_materials_volume must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"thicknessFactor\":1"),
        "the thickness factor must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"attenuationDistance\""),
        "attenuationDistance must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"attenuationColor\""),
        "attenuationColor must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_volume_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_volume"),
        "extension must NOT appear when no material sets volume, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_defaults() {
    // Per the spec the thickness factor defaults to `0.0` (thin-walled)
    // and `attenuationColor` defaults to `[1, 1, 1]`. The
    // `attenuationDistance` default is `+Infinity`, which JSON cannot
    // encode — the decoder leaves the key absent and consumers interpret
    // missing-key as the +Infinity spec default. A bare `{}` extension
    // object should resolve to `thicknessFactor = 0`, `attenuationColor
    // = [1, 1, 1]`, no `attenuationDistance`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_volume"],
        "materials": [
            {
                "extensions": { "KHR_materials_volume": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = volume_of(&scene.materials[0]).expect("volume present");

    let tf = obj
        .get("thicknessFactor")
        .and_then(|v| v.as_f64())
        .expect("thicknessFactor default materialised");
    assert!(tf.abs() < 1e-9, "default thicknessFactor is 0.0, got {tf}");

    let ac = obj
        .get("attenuationColor")
        .and_then(|v| v.as_array())
        .expect("attenuationColor default materialised");
    assert_eq!(ac.len(), 3, "attenuationColor is a 3-vector");
    for (i, v) in ac.iter().enumerate() {
        let f = v.as_f64().expect("colour component is a number");
        assert!(
            (f - 1.0).abs() < 1e-9,
            "attenuationColor[{i}] default is 1.0, got {f}"
        );
    }

    assert!(
        !obj.contains_key("attenuationDistance"),
        "attenuationDistance with +Infinity default stays absent (JSON \
         cannot encode non-finite), got obj keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(!obj.contains_key("thicknessTexture"));
}

#[test]
fn explicit_attenuation_decodes() {
    // From the spec example: thicknessFactor 1.0, attenuationDistance
    // 0.006, attenuationColor [0.5, 0.5, 0.5].
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_volume"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_volume": {
                        "thicknessFactor": 1.0,
                        "attenuationDistance": 0.006,
                        "attenuationColor": [ 0.5, 0.5, 0.5 ]
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = volume_of(&scene.materials[0]).expect("volume present");
    assert!((obj.get("thicknessFactor").unwrap().as_f64().unwrap() - 1.0).abs() < 1e-6);
    assert!((obj.get("attenuationDistance").unwrap().as_f64().unwrap() - 0.006).abs() < 1e-9);
    let ac = obj.get("attenuationColor").unwrap().as_array().unwrap();
    assert_eq!(ac.len(), 3);
    for v in ac {
        assert!((v.as_f64().unwrap() - 0.5).abs() < 1e-6);
    }
}

#[test]
fn thickness_texture_round_trips_with_texcoord() {
    // Build a JSON document carrying a `thicknessTexture` with a
    // non-default texCoord. Verify both the index and the explicit
    // `texCoord` survive a decode->encode->decode cycle. It is a plain
    // `textureInfo` so it carries no `scale`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_volume"],
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
                    "KHR_materials_volume": {
                        "thicknessFactor": 1.0,
                        "thicknessTexture": { "index": 0, "texCoord": 1 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = volume_of(&scene.materials[0]).expect("volume present");

    let tt = obj
        .get("thicknessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(tt.get("texCoord").unwrap().as_u64().unwrap(), 1);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(raw.contains("\"thicknessTexture\""), "got: {raw}");

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = volume_of(&decoded.materials[0]).expect("volume present");
    let tt2 = obj2
        .get("thicknessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt2.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(tt2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn default_texcoord_omitted_on_thickness_texture() {
    // A `thicknessTexture` whose texCoord is the default 0 must not
    // emit a `texCoord` key on round-trip (textureInfo schema).
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_volume"],
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
                    "KHR_materials_volume": {
                        "thicknessTexture": { "index": 0 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = volume_of(&scene.materials[0]).expect("volume present");
    let tt = obj
        .get("thicknessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(tt.get("texCoord").is_none(), "default texCoord 0 omitted");
}

#[test]
fn volume_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_volume": { "thicknessFactor": 0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_volume"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_volume, got {msg}"
    );
}

#[test]
fn full_attenuation_roundtrips_through_glb() {
    // End-to-end: build a scene programmatically with the full set of
    // four properties, round-trip via .glb, and check every value
    // survives.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_volume".to_owned(),
        json!({
            "thicknessFactor": 0.25,
            "attenuationDistance": 3.5,
            "attenuationColor": [0.1, 0.4, 0.7]
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = volume_of(&decoded.materials[0]).expect("volume present");

    assert!((obj.get("thicknessFactor").unwrap().as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert!((obj.get("attenuationDistance").unwrap().as_f64().unwrap() - 3.5).abs() < 1e-6);
    let ac = obj.get("attenuationColor").unwrap().as_array().unwrap();
    let r = ac[0].as_f64().unwrap();
    let g = ac[1].as_f64().unwrap();
    let b = ac[2].as_f64().unwrap();
    assert!((r - 0.1).abs() < 1e-6);
    assert!((g - 0.4).abs() < 1e-6);
    assert!((b - 0.7).abs() < 1e-6);
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
