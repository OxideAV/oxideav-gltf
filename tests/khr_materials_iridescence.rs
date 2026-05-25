//! KHR_materials_iridescence extension — adds a thin-film interference
//! layer on top of the metallic-roughness material so the hue varies with
//! viewing angle and thin-film thickness, per
//! `docs/3d/gltf/extensions/KHR_materials_iridescence.md`. It contributes
//! a scalar `iridescenceFactor` (default `0.0`; zero disables the whole
//! effect), an optional `iridescenceTexture` (a `textureInfo` whose `.r`
//! channel multiplies the factor), a scalar `iridescenceIor` (default
//! `1.3`), and a thickness range expressed as
//! `iridescenceThicknessMinimum` (default `100.0`) and
//! `iridescenceThicknessMaximum` (default `400.0`) in nanometres plus an
//! optional `iridescenceThicknessTexture` whose `.g` channel selects
//! between the two thickness bounds. The spec explicitly allows
//! `iridescenceThicknessMinimum > iridescenceThicknessMaximum`. The
//! decoder lifts the full extension object into
//! `Material::extras["KHR_materials_iridescence"]` as a JSON
//! `Value::Object`; the encoder lifts that object back into the typed
//! extensions block on write and appends `KHR_materials_iridescence` to
//! `extensionsUsed`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::{json, Value};

fn iridescence_of(m: &Material) -> Option<&serde_json::Map<String, Value>> {
    m.extras
        .get("KHR_materials_iridescence")
        .and_then(|v| v.as_object())
}

#[test]
fn iridescence_factor_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_iridescence".to_owned(),
        json!({ "iridescenceFactor": 0.75 }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let obj = iridescence_of(&decoded.materials[0]).expect("iridescence present");
    let factor = obj
        .get("iridescenceFactor")
        .and_then(|v| v.as_f64())
        .expect("factor present");
    assert!(
        (factor - 0.75).abs() < 1e-6,
        "iridescenceFactor round-trips, got {factor}"
    );
}

#[test]
fn iridescence_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_iridescence".to_owned(),
        json!({
            "iridescenceFactor": 1.0,
            "iridescenceIor": 1.3,
            "iridescenceThicknessMaximum": 400.0
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
        raw.contains("\"KHR_materials_iridescence\""),
        "KHR_materials_iridescence must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"iridescenceFactor\":1"),
        "iridescenceFactor must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"iridescenceIor\""),
        "iridescenceIor must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"iridescenceThicknessMaximum\""),
        "iridescenceThicknessMaximum must be emitted, got: {raw}"
    );
}

#[test]
fn material_without_iridescence_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_material(Material::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_materials_iridescence"),
        "extension must NOT appear when no material sets iridescence, got: {raw}"
    );
}

#[test]
fn bare_extension_object_resolves_to_spec_defaults() {
    // Per the spec the iridescence factor defaults to `0.0` (disabled), the
    // IOR defaults to `1.3`, the thickness minimum defaults to `100.0` and
    // the maximum defaults to `400.0` (all per §Properties). A bare `{}`
    // extension object should resolve to a fully-specified record.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_iridescence"],
        "materials": [
            {
                "extensions": { "KHR_materials_iridescence": {} }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = iridescence_of(&scene.materials[0]).expect("iridescence present");

    let factor = obj
        .get("iridescenceFactor")
        .and_then(|v| v.as_f64())
        .expect("iridescenceFactor default materialised");
    assert!(
        factor.abs() < 1e-9,
        "default iridescenceFactor is 0.0, got {factor}"
    );

    let ior = obj
        .get("iridescenceIor")
        .and_then(|v| v.as_f64())
        .expect("iridescenceIor default materialised");
    assert!(
        (ior - 1.3).abs() < 1e-6,
        "default iridescenceIor is 1.3, got {ior}"
    );

    let thmin = obj
        .get("iridescenceThicknessMinimum")
        .and_then(|v| v.as_f64())
        .expect("iridescenceThicknessMinimum default materialised");
    assert!(
        (thmin - 100.0).abs() < 1e-6,
        "default iridescenceThicknessMinimum is 100.0, got {thmin}"
    );

    let thmax = obj
        .get("iridescenceThicknessMaximum")
        .and_then(|v| v.as_f64())
        .expect("iridescenceThicknessMaximum default materialised");
    assert!(
        (thmax - 400.0).abs() < 1e-6,
        "default iridescenceThicknessMaximum is 400.0, got {thmax}"
    );

    assert!(!obj.contains_key("iridescenceTexture"));
    assert!(!obj.contains_key("iridescenceThicknessTexture"));
}

#[test]
fn explicit_thickness_range_decodes() {
    // From the spec example: iridescenceFactor 1.0, iridescenceIor 1.3,
    // iridescenceThicknessMaximum 400.0.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_iridescence"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_iridescence": {
                        "iridescenceFactor": 1.0,
                        "iridescenceIor": 1.3,
                        "iridescenceThicknessMaximum": 400.0
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = iridescence_of(&scene.materials[0]).expect("iridescence present");
    assert!((obj.get("iridescenceFactor").unwrap().as_f64().unwrap() - 1.0).abs() < 1e-6);
    assert!((obj.get("iridescenceIor").unwrap().as_f64().unwrap() - 1.3).abs() < 1e-6);
    assert!(
        (obj.get("iridescenceThicknessMaximum")
            .unwrap()
            .as_f64()
            .unwrap()
            - 400.0)
            .abs()
            < 1e-6
    );
}

#[test]
fn inverted_thickness_range_allowed_by_spec() {
    // Per spec §Properties: "The iridescenceThicknessMinimum value MAY be
    // greater than iridescenceThicknessMaximum value." The decoder must
    // accept it and pass it through unmodified.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_iridescence"],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_iridescence": {
                        "iridescenceThicknessMinimum": 800.0,
                        "iridescenceThicknessMaximum": 200.0
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = iridescence_of(&scene.materials[0]).expect("iridescence present");
    assert!(
        (obj.get("iridescenceThicknessMinimum")
            .unwrap()
            .as_f64()
            .unwrap()
            - 800.0)
            .abs()
            < 1e-6
    );
    assert!(
        (obj.get("iridescenceThicknessMaximum")
            .unwrap()
            .as_f64()
            .unwrap()
            - 200.0)
            .abs()
            < 1e-6
    );
}

#[test]
fn thickness_texture_round_trips_with_texcoord() {
    // Build a JSON document carrying an `iridescenceThicknessTexture` with a
    // non-default texCoord. Verify both the index and the explicit
    // `texCoord` survive a decode->encode->decode cycle. It is a plain
    // `textureInfo` so it carries no `scale`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_iridescence"],
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
                    "KHR_materials_iridescence": {
                        "iridescenceFactor": 1.0,
                        "iridescenceThicknessTexture": { "index": 0, "texCoord": 1 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = iridescence_of(&scene.materials[0]).expect("iridescence present");

    let tt = obj
        .get("iridescenceThicknessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(tt.get("texCoord").unwrap().as_u64().unwrap(), 1);

    // Round-trip through encoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw = std::str::from_utf8(&extract_json_chunk(&glb))
        .unwrap()
        .to_owned();
    assert!(
        raw.contains("\"iridescenceThicknessTexture\""),
        "got: {raw}"
    );

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = iridescence_of(&decoded.materials[0]).expect("iridescence present");
    let tt2 = obj2
        .get("iridescenceThicknessTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(tt2.get("index").unwrap().as_u64().unwrap(), 0);
    assert_eq!(tt2.get("texCoord").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn intensity_texture_round_trips() {
    // The other `textureInfo` on the extension: `iridescenceTexture`. Same
    // round-trip rules.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_iridescence"],
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
                    "KHR_materials_iridescence": {
                        "iridescenceFactor": 0.5,
                        "iridescenceTexture": { "index": 0 }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = iridescence_of(&scene.materials[0]).expect("iridescence present");
    let it = obj
        .get("iridescenceTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(it.get("index").unwrap().as_u64().unwrap(), 0);
    assert!(it.get("texCoord").is_none(), "default texCoord 0 omitted");

    // Round-trip through the encoder + decoder.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj2 = iridescence_of(&decoded.materials[0]).expect("iridescence present");
    let it2 = obj2
        .get("iridescenceTexture")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(it2.get("index").unwrap().as_u64().unwrap(), 0);
}

#[test]
fn iridescence_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "materials": [
            {
                "extensions": {
                    "KHR_materials_iridescence": { "iridescenceFactor": 0.5 }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_iridescence"),
        "expected ExtensionStackUsedNotDeclared for KHR_materials_iridescence, got {msg}"
    );
}

#[test]
fn full_record_roundtrips_through_glb() {
    // End-to-end: build a scene programmatically with the full set of
    // four scalar properties + texture references, round-trip via .glb,
    // and check every value survives.
    let mut scene = Scene3D::new();
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_iridescence".to_owned(),
        json!({
            "iridescenceFactor": 0.25,
            "iridescenceIor": 1.5,
            "iridescenceThicknessMinimum": 50.0,
            "iridescenceThicknessMaximum": 750.0
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let obj = iridescence_of(&decoded.materials[0]).expect("iridescence present");

    assert!((obj.get("iridescenceFactor").unwrap().as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert!((obj.get("iridescenceIor").unwrap().as_f64().unwrap() - 1.5).abs() < 1e-6);
    assert!(
        (obj.get("iridescenceThicknessMinimum")
            .unwrap()
            .as_f64()
            .unwrap()
            - 50.0)
            .abs()
            < 1e-6
    );
    assert!(
        (obj.get("iridescenceThicknessMaximum")
            .unwrap()
            .as_f64()
            .unwrap()
            - 750.0)
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
