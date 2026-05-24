//! KHR_materials_transmission extension — makes the metallic-roughness
//! material optically transparent (light passes through the surface
//! rather than being diffusely re-emitted), enabling physically-plausible
//! glass / plastic, per
//! `docs/3d/gltf/extensions/KHR_materials_transmission.md`. It contributes
//! a scalar `transmissionFactor` (default `0.0`) plus one optional
//! `textureInfo` reference (`transmissionTexture`, whose `.r` channel is
//! multiplied by the factor). The decoder lifts the full extension object
//! into `Material::extras["KHR_materials_transmission"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends `KHR_materials_transmission` to
//! `extensionsUsed`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn transmission_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_transmission")
        .and_then(|v| v.as_object())
}

#[test]
fn transmission_factor_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_transmission".to_owned(),
        json!({ "transmissionFactor": 0.75 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = transmission_of(&decoded.materials[0]).expect("transmission object present");
    let f = obj
        .get("transmissionFactor")
        .and_then(|v| v.as_f64())
        .expect("factor present");
    assert!(
        (f - 0.75).abs() < 1e-6,
        "transmissionFactor round-trips, got {f}"
    );
}

#[test]
fn transmission_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_transmission".to_owned(),
        json!({ "transmissionFactor": 1.0 }),
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
        raw.contains("\"KHR_materials_transmission\""),
        "KHR_materials_transmission must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"transmissionFactor\":1"),
        "the factor must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_transmission_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_transmission"),
        "extension must NOT appear when no material sets transmission, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_default() {
    // Per the spec the transmission factor defaults to `0.0` with no
    // texture present; a bare `{}` extension object resolves to that.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_transmission"],
        "materials": [
            {
                "extensions": { "KHR_materials_transmission": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = transmission_of(&scene.materials[0]).expect("transmission present");
    let f = obj
        .get("transmissionFactor")
        .and_then(|v| v.as_f64())
        .expect("factor default materialised");
    assert!(f.abs() < 1e-9, "default transmissionFactor is 0.0, got {f}");
    assert!(!obj.contains_key("transmissionTexture"));
}

#[test]
fn explicit_transmission_factor_decodes() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_transmission"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_transmission": {
                        "transmissionFactor": 0.42
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = transmission_of(&scene.materials[0]).expect("transmission present");
    assert!((obj.get("transmissionFactor").unwrap().as_f64().unwrap() - 0.42).abs() < 1e-6);
}

#[test]
fn transmission_texture_round_trips_with_texcoord() {
    // Build a JSON document carrying a `transmissionTexture` with a
    // non-default texCoord. Verify both the index and the explicit
    // `texCoord` survive a decode->encode->decode cycle. It is a plain
    // `textureInfo` so it carries no `scale`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_transmission"],
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
                    "KHR_materials_transmission": {
                        "transmissionFactor": 0.5,
                        "transmissionTexture": { "index": 0, "texCoord": 1 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = transmission_of(&scene.materials[0]).expect("transmission present");

    let tt = obj
        .get("transmissionTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(tt.get("texCoord").unwrap().as_u64().unwrap(), 1);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(raw.contains("\"transmissionTexture\""), "got: {raw}");

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = transmission_of(&decoded.materials[0]).expect("transmission present");
    let tt2 = obj2
        .get("transmissionTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt2.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(tt2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn default_texcoord_omitted_on_transmission_texture() {
    // A `transmissionTexture` whose texCoord is the default 0 must not
    // emit a `texCoord` key on round-trip (textureInfo schema).
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_transmission"],
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
                    "KHR_materials_transmission": {
                        "transmissionTexture": { "index": 0 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = transmission_of(&scene.materials[0]).expect("transmission present");
    let tt = obj
        .get("transmissionTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(tt.get("texCoord").is_none(), "default texCoord 0 omitted");
}

#[test]
fn transmission_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_transmission": { "transmissionFactor": 0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_transmission"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_transmission, got {msg}"
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
