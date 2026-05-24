//! KHR_materials_clearcoat extension — layers a clear coating on top of
//! the metallic-roughness material per
//! `docs/3d/gltf/extensions/KHR_materials_clearcoat.md`. It contributes
//! two scalar factors (`clearcoatFactor` / `clearcoatRoughnessFactor`,
//! both default `0.0`) plus three optional texture references
//! (`clearcoatTexture`, `clearcoatRoughnessTexture` as `textureInfo`,
//! `clearcoatNormalTexture` as `normalTextureInfo`). The decoder lifts
//! the full extension object into
//! `Material::extras["KHR_materials_clearcoat"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends `KHR_materials_clearcoat` to
//! `extensionsUsed`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn clearcoat_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_clearcoat")
        .and_then(|v| v.as_object())
}

#[test]
fn clearcoat_factors_roundtrip_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_clearcoat".to_owned(),
        json!({
            "clearcoatFactor": 0.8,
            "clearcoatRoughnessFactor": 0.3
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = clearcoat_of(&decoded.materials[0]).expect("clearcoat object present");
    let f = obj
        .get("clearcoatFactor")
        .and_then(|v| v.as_f64())
        .expect("factor present");
    assert!(
        (f - 0.8).abs() < 1e-6,
        "clearcoatFactor round-trips, got {f}"
    );
    let r = obj
        .get("clearcoatRoughnessFactor")
        .and_then(|v| v.as_f64())
        .expect("roughness factor present");
    assert!(
        (r - 0.3).abs() < 1e-6,
        "clearcoatRoughnessFactor round-trips, got {r}"
    );
}

#[test]
fn clearcoat_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_clearcoat".to_owned(),
        json!({ "clearcoatFactor": 1.0 }),
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
        raw.contains("\"KHR_materials_clearcoat\""),
        "KHR_materials_clearcoat must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"clearcoatFactor\":1"),
        "the scalar clearcoatFactor must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_clearcoat_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_clearcoat"),
        "extension must NOT appear when no material sets a clearcoat, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_defaults() {
    // Per the spec both factors default to `0.0` and no textures are
    // present; a bare `{}` extension object resolves to those. (The spec
    // also notes that a zero `clearcoatFactor` disables the whole
    // clearcoat layer.)
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "materials": [
            {
                "extensions": { "KHR_materials_clearcoat": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = clearcoat_of(&scene.materials[0]).expect("clearcoat present");
    let f = obj
        .get("clearcoatFactor")
        .and_then(|v| v.as_f64())
        .expect("factor default materialised");
    assert!(f.abs() < 1e-9, "default clearcoatFactor is 0.0, got {f}");
    let r = obj
        .get("clearcoatRoughnessFactor")
        .and_then(|v| v.as_f64())
        .expect("roughness default materialised");
    assert!(
        r.abs() < 1e-9,
        "default clearcoatRoughnessFactor is 0.0, got {r}"
    );
    assert!(!obj.contains_key("clearcoatTexture"));
    assert!(!obj.contains_key("clearcoatRoughnessTexture"));
    assert!(!obj.contains_key("clearcoatNormalTexture"));
}

#[test]
fn explicit_clearcoat_factors_decode() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_clearcoat": {
                        "clearcoatFactor": 0.65,
                        "clearcoatRoughnessFactor": 0.12
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = clearcoat_of(&scene.materials[0]).expect("clearcoat present");
    assert!((obj.get("clearcoatFactor").unwrap().as_f64().unwrap() - 0.65).abs() < 1e-6);
    assert!(
        (obj.get("clearcoatRoughnessFactor")
            .unwrap()
            .as_f64()
            .unwrap()
            - 0.12)
            .abs()
            < 1e-6
    );
}

#[test]
fn clearcoat_textures_round_trip_including_normal_scale() {
    // Build a JSON document carrying all three clearcoat textures:
    // `clearcoatTexture` (default texCoord omitted),
    // `clearcoatRoughnessTexture` (texCoord 1), and
    // `clearcoatNormalTexture` (a `normalTextureInfo` carrying a `scale`
    // of 2.0 — the field the plain `textureInfo` lacks). Verify all
    // indices, the explicit `texCoord`, and the normal-map `scale`
    // survive a decode->encode->decode cycle.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "images": [
            { "uri": "data:image/png;base64,AAAA" },
            { "uri": "data:image/png;base64,AAAA" },
            { "uri": "data:image/png;base64,AAAA" }
        ],
        "samplers": [{}],
        "textures": [
            { "source": 0, "sampler": 0 },
            { "source": 1, "sampler": 0 },
            { "source": 2, "sampler": 0 }
        ],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_clearcoat": {
                        "clearcoatTexture": { "index": 0 },
                        "clearcoatRoughnessTexture": { "index": 1, "texCoord": 1 },
                        "clearcoatNormalTexture": { "index": 2, "scale": 2.0 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = clearcoat_of(&scene.materials[0]).expect("clearcoat present");

    let ct = obj
        .get("clearcoatTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(ct.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(ct.get("texCoord").is_none(), "default texCoord 0 omitted");

    let crt = obj
        .get("clearcoatRoughnessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(crt.get("index").unwrap().as_u64().unwrap(), 1);
    assert_eq!(crt.get("texCoord").unwrap().as_u64().unwrap(), 1);

    let cnt = obj
        .get("clearcoatNormalTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(cnt.get("index").unwrap().as_u64().unwrap(), 2);
    assert!((cnt.get("scale").unwrap().as_f64().unwrap() - 2.0).abs() < 1e-6);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(raw.contains("\"clearcoatTexture\""), "got: {raw}");
    assert!(raw.contains("\"clearcoatRoughnessTexture\""), "got: {raw}");
    assert!(raw.contains("\"clearcoatNormalTexture\""), "got: {raw}");
    assert!(
        raw.contains("\"scale\":2"),
        "normal scale emitted, got: {raw}"
    );

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = clearcoat_of(&decoded.materials[0]).expect("clearcoat present");
    let cnt2 = obj2
        .get("clearcoatNormalTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(cnt2.get("index").unwrap().as_u64().unwrap(), 2);
    assert!((cnt2.get("scale").unwrap().as_f64().unwrap() - 2.0).abs() < 1e-6);
    let crt2 = obj2
        .get("clearcoatRoughnessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(crt2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn clearcoat_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_clearcoat": { "clearcoatFactor": 0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_clearcoat"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_clearcoat, got {msg}"
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
