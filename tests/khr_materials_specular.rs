//! KHR_materials_specular extension — adds a `specularFactor` scalar
//! and `specularColorFactor` RGB triple (plus optional textures) to the
//! metallic-roughness material per
//! `docs/3d/gltf/extensions/KHR_materials_specular.md`. The decoder
//! lifts the full extension object into
//! `Material::extras["KHR_materials_specular"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends `KHR_materials_specular` to
//! `extensionsUsed`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn specular_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_specular")
        .and_then(|v| v.as_object())
}

#[test]
fn specular_scalar_and_colour_roundtrip_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_specular".to_owned(),
        json!({
            "specularFactor": 0.42,
            "specularColorFactor": [0.7, 0.8, 0.9]
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = specular_of(&decoded.materials[0]).expect("specular object present");
    let f = obj
        .get("specularFactor")
        .and_then(|v| v.as_f64())
        .expect("factor present");
    assert!(
        (f - 0.42).abs() < 1e-6,
        "specularFactor round-trips, got {f}"
    );
    let cf = obj
        .get("specularColorFactor")
        .and_then(|v| v.as_array())
        .expect("colour factor present");
    assert_eq!(cf.len(), 3);
    let got: [f64; 3] = [
        cf[0].as_f64().unwrap(),
        cf[1].as_f64().unwrap(),
        cf[2].as_f64().unwrap(),
    ];
    for (g, want) in got.iter().zip([0.7, 0.8, 0.9].iter()) {
        assert!((g - want).abs() < 1e-6, "colour comp round-trips");
    }
}

#[test]
fn specular_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_specular".to_owned(),
        json!({ "specularFactor": 0.25 }),
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
        raw.contains("\"KHR_materials_specular\""),
        "KHR_materials_specular must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"specularFactor\":0.25"),
        "the scalar specularFactor must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_specular_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_specular"),
        "extension must NOT appear when no material sets a specular, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_defaults() {
    // Per the spec all four fields are optional with defaults
    // `specularFactor = 1.0`, `specularColorFactor = [1, 1, 1]`, and no
    // textures. A bare `{}` extension object resolves to those.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_specular"],
        "materials": [
            {
                "extensions": { "KHR_materials_specular": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = specular_of(&scene.materials[0]).expect("specular present");
    let f = obj
        .get("specularFactor")
        .and_then(|v| v.as_f64())
        .expect("factor default materialised");
    assert!(
        (f - 1.0).abs() < 1e-9,
        "default specularFactor is 1.0, got {f}"
    );
    let cf = obj
        .get("specularColorFactor")
        .and_then(|v| v.as_array())
        .expect("default colour materialised");
    assert_eq!(cf.len(), 3);
    for c in cf {
        assert!((c.as_f64().unwrap() - 1.0).abs() < 1e-9);
    }
    assert!(!obj.contains_key("specularTexture"));
    assert!(!obj.contains_key("specularColorTexture"));
}

#[test]
fn explicit_specular_factor_and_colour_decode() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_specular"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_specular": {
                        "specularFactor": 0.6,
                        "specularColorFactor": [2.0, 1.5, 0.25]
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = specular_of(&scene.materials[0]).expect("specular present");
    assert!((obj.get("specularFactor").unwrap().as_f64().unwrap() - 0.6).abs() < 1e-6);
    let cf = obj.get("specularColorFactor").unwrap().as_array().unwrap();
    // Spec explicitly allows components above 1.0 — the F0 reflectance
    // gets clamped at render time, not here.
    assert!((cf[0].as_f64().unwrap() - 2.0).abs() < 1e-6);
    assert!((cf[1].as_f64().unwrap() - 1.5).abs() < 1e-6);
    assert!((cf[2].as_f64().unwrap() - 0.25).abs() < 1e-6);
}

#[test]
fn specular_textures_round_trip_with_index_and_texcoord() {
    // Build a JSON document carrying a `specularTexture` (texCoord 0,
    // default omitted) and `specularColorTexture` (texCoord 1) and
    // verify both indices and the explicit `texCoord` survive a
    // decode→encode→decode cycle. The texture array is shared between
    // the core material and the specular extension to keep the test
    // small.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_specular"],
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
                    "KHR_materials_specular": {
                        "specularTexture": { "index": 0 },
                        "specularColorTexture": { "index": 1, "texCoord": 1 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = specular_of(&scene.materials[0]).expect("specular present");
    let st = obj
        .get("specularTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(st.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(st.get("texCoord").is_none(), "default texCoord 0 omitted");
    let sct = obj
        .get("specularColorTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(sct.get("index").unwrap().as_u64().unwrap(), 1);
    assert_eq!(sct.get("texCoord").unwrap().as_u64().unwrap(), 1);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    // Texture indices stay aligned with the textures array's ordering;
    // the encoder emits the typed `MaterialSpecular` block, so we look
    // for the wire-shape JSON keys.
    assert!(raw.contains("\"specularTexture\""), "got: {raw}");
    assert!(raw.contains("\"specularColorTexture\""), "got: {raw}");
    assert!(raw.contains("\"texCoord\":1"), "got: {raw}");

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = specular_of(&decoded.materials[0]).expect("specular present");
    let st2 = obj2
        .get("specularTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(st2.get("index").unwrap().as_u64().unwrap(), 0);
    let sct2 = obj2
        .get("specularColorTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(sct2.get("index").unwrap().as_u64().unwrap(), 1);
    assert_eq!(sct2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn specular_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_specular": { "specularFactor": 0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_specular"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_specular, got {msg}"
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
