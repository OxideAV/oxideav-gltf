//! KHR_texture_transform extension — an affine 2D transform
//! (offset / rotation / scale / texCoord) applied to the UV
//! coordinates of any `textureInfo` per
//! `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
//! Updates.
//!
//! All four fields are optional. The decoder surfaces the per-slot
//! transform through `Material::extras["KHR_texture_transform:<slot>"]`
//! as a JSON object so downstream raster consumers can apply the
//! transform without us widening `oxideav_mesh3d::TextureRef`. The five
//! recognised slot names mirror the core PBR textureInfo keys:
//! `baseColor`, `metallicRoughness`, `normal`, `occlusion`,
//! `emissive`.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Material, Mesh3DDecoder, Mesh3DEncoder, Scene3D, Texture, TextureRef};
use serde_json::Value;

fn dummy_texture() -> Texture {
    Texture::from_encoded("image/png".to_owned(), vec![0xFFu8; 16])
}

fn transform_object<'a>(m: &'a Material, slot: &str) -> Option<&'a serde_json::Map<String, Value>> {
    m.extras
        .get(&format!("KHR_texture_transform:{slot}"))
        .and_then(|v| v.as_object())
}

fn scene_with_emissive_transform(offset: [f64; 2], rotation: f64, scale: [f64; 2]) -> Scene3D {
    let mut scene = Scene3D::new();
    let tex_id = scene.add_texture(dummy_texture());

    let mut mat = Material::new();
    mat.emissive_factor = [1.0, 1.0, 1.0];
    mat.emissive_texture = Some(TextureRef::new(tex_id));
    let mut obj = serde_json::Map::new();
    obj.insert(
        "offset".to_owned(),
        Value::Array(vec![Value::from(offset[0]), Value::from(offset[1])]),
    );
    obj.insert("rotation".to_owned(), Value::from(rotation));
    obj.insert(
        "scale".to_owned(),
        Value::Array(vec![Value::from(scale[0]), Value::from(scale[1])]),
    );
    mat.extras.insert(
        "KHR_texture_transform:emissive".to_owned(),
        Value::Object(obj),
    );
    scene.add_material(mat);
    scene
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

#[test]
fn texture_transform_roundtrips_via_glb() {
    let scene = scene_with_emissive_transform([0.25, 0.5], 1.25, [2.0, 4.0]);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.materials.len(), 1);
    let m = &decoded.materials[0];
    let obj = transform_object(m, "emissive").expect("transform survives round-trip");

    let offset = obj
        .get("offset")
        .and_then(|v| v.as_array())
        .expect("offset present");
    assert_eq!(offset[0].as_f64().unwrap(), 0.25);
    assert_eq!(offset[1].as_f64().unwrap(), 0.5);

    let rotation = obj.get("rotation").and_then(|v| v.as_f64()).unwrap();
    assert!((rotation - 1.25).abs() < 1e-5);

    let scale = obj
        .get("scale")
        .and_then(|v| v.as_array())
        .expect("scale present");
    assert_eq!(scale[0].as_f64().unwrap(), 2.0);
    assert_eq!(scale[1].as_f64().unwrap(), 4.0);
}

#[test]
fn texture_transform_emits_extensions_used_on_encode() {
    let scene = scene_with_emissive_transform([0.0, 1.0], 0.0, [0.5, 0.5]);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_texture_transform\""),
        "KHR_texture_transform must appear in JSON, got: {raw}"
    );
    // The textureInfo block must carry the extensions object inline —
    // not surface as a stray `extras` key on the material.
    assert!(
        raw.contains("\"emissiveTexture\""),
        "emissiveTexture must be present, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_texture_transform\":{"),
        "KHR_texture_transform must be emitted as a typed object, got: {raw}"
    );
    assert!(
        !raw.contains("KHR_texture_transform:emissive"),
        "the per-slot extras key must be lifted into the typed block, not leaked into JSON, got: {raw}"
    );
}

#[test]
fn material_without_texture_transform_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    let tex_id = scene.add_texture(dummy_texture());
    let mut mat = Material::new();
    mat.emissive_factor = [1.0, 1.0, 1.0];
    mat.emissive_texture = Some(TextureRef::new(tex_id));
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_texture_transform"),
        "extension must NOT appear when no texture carries a transform, got: {raw}"
    );
}

#[test]
fn bare_extension_object_decodes_to_empty_transform() {
    // Per the spec §glTF Schema Updates, all four fields (`offset`,
    // `rotation`, `scale`, `texCoord`) are optional with defaults
    // `[0, 0]`, `0`, `[1, 1]`, and the parent texCoord respectively —
    // so a bare `{}` extension object resolves to an empty record on
    // our side (consumers materialise the defaults at use time).
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_transform"],
        "textures": [],
        "materials": [
            {
                "emissiveFactor": [1.0, 1.0, 1.0],
                "emissiveTexture": {
                    "index": 0,
                    "extensions": { "KHR_texture_transform": {} }
                }
            }
        ]
    }"#;
    // Stub a single texture by sneaking in an image that the validator
    // won't load (we never resolve the texture; the decoder only needs
    // the material's slot wired to the extension block).
    let mut json_obj: serde_json::Value = serde_json::from_slice(json).unwrap();
    json_obj["textures"] = serde_json::json!([{ "source": 0 }]);
    json_obj["images"] = serde_json::json!([{ "uri": "data:image/png;base64,AAAA" }]);
    let augmented = serde_json::to_vec(&json_obj).unwrap();
    let scene = GltfDecoder::new().decode(&augmented).unwrap();
    assert_eq!(scene.materials.len(), 1);
    let obj = transform_object(&scene.materials[0], "emissive")
        .expect("bare transform still surfaces on the slot key");
    assert!(
        obj.is_empty(),
        "bare {{}} extension object decodes as an empty map (defaults applied at use time), got {obj:?}"
    );
}

#[test]
fn explicit_transform_decodes_with_all_fields() {
    // Mirrors the spec's lower-left-quadrant example (rotated 90°).
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_texture_transform"],
        "textures": [{ "source": 0 }],
        "images": [{ "uri": "data:image/png;base64,AAAA" }],
        "materials": [
            {
                "emissiveFactor": [1.0, 1.0, 1.0],
                "emissiveTexture": {
                    "index": 0,
                    "extensions": {
                        "KHR_texture_transform": {
                            "offset": [0, 1],
                            "rotation": 1.57079632679,
                            "scale": [0.5, 0.5],
                            "texCoord": 1
                        }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    let obj = transform_object(&scene.materials[0], "emissive").expect("transform present");
    let offset = obj.get("offset").and_then(|v| v.as_array()).unwrap();
    assert_eq!(offset[0].as_f64().unwrap(), 0.0);
    assert_eq!(offset[1].as_f64().unwrap(), 1.0);
    // The spec example rotates 90° (π/2 radians); the stored value
    // round-trips through the extension's `f32` field, so compare
    // against the same quarter-turn expressed without spelling out the
    // approximate constant (which clippy flags).
    let quarter_turn = (std::f64::consts::PI / 2.0) as f32 as f64;
    assert!((obj.get("rotation").and_then(|v| v.as_f64()).unwrap() - quarter_turn).abs() < 1e-5);
    let scale = obj.get("scale").and_then(|v| v.as_array()).unwrap();
    assert_eq!(scale[0].as_f64().unwrap(), 0.5);
    assert_eq!(scale[1].as_f64().unwrap(), 0.5);
    assert_eq!(obj.get("texCoord").and_then(|v| v.as_u64()).unwrap(), 1);
}

#[test]
fn texture_transform_data_block_without_extensions_used_is_rejected() {
    // Data block present but the extension is not declared in
    // `extensionsUsed` — spec §3.12 violation.
    let json = br#"{
        "asset": { "version": "2.0" },
        "textures": [{ "source": 0 }],
        "images": [{ "uri": "data:image/png;base64,AAAA" }],
        "materials": [
            {
                "emissiveFactor": [1.0, 1.0, 1.0],
                "emissiveTexture": {
                    "index": 0,
                    "extensions": {
                        "KHR_texture_transform": { "scale": [2, 2] }
                    }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_texture_transform"),
        "expected ExtensionStackUsedNotDeclared for KHR_texture_transform, got {msg}"
    );
}

#[test]
fn transform_on_base_color_slot_roundtrips() {
    let mut scene = Scene3D::new();
    let tex_id = scene.add_texture(dummy_texture());
    let mut mat = Material::new();
    mat.base_color_texture = Some(TextureRef::new(tex_id));
    let mut obj = serde_json::Map::new();
    obj.insert("rotation".to_owned(), Value::from(0.6_f64));
    mat.extras.insert(
        "KHR_texture_transform:baseColor".to_owned(),
        Value::Object(obj),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    let obj = transform_object(&decoded.materials[0], "baseColor")
        .expect("baseColor slot transform survives round-trip");
    let rotation = obj.get("rotation").and_then(|v| v.as_f64()).unwrap();
    assert!((rotation - 0.6).abs() < 1e-5);
}

// --- KHR_texture_transform on material-EXTENSION texture slots -------
//
// Per `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
// Updates the transform "may be defined on `textureInfo` structures" —
// *any* textureInfo, including the ones nested inside a material
// extension (e.g. `KHR_materials_specular.specularTexture`). The
// decoder parks the whole material-extension block in
// `Material::extras["KHR_materials_<x>"]`, so the nested transform
// rides through verbatim; the §3.12 stack validator and the encoder's
// `extensionsUsed` declaration must reach it too.

#[test]
fn transform_nested_in_specular_texture_roundtrips_and_declares_extension() {
    // specularTexture carries a KHR_texture_transform; both the
    // KHR_materials_specular extension AND KHR_texture_transform must be
    // declared on encode, and the nested transform must survive the
    // glb round-trip.
    let mut scene = Scene3D::new();
    // The on-wire texture index is this texture's position (0 — it is the
    // only texture in the document).
    scene.add_texture(dummy_texture());
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_specular".to_owned(),
        serde_json::json!({
            "specularFactor": 1.0,
            "specularTexture": {
                "index": 0,
                "extensions": {
                    "KHR_texture_transform": { "offset": [0.1, 0.2], "rotation": 0.5 }
                }
            }
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        raw.contains("\"KHR_materials_specular\""),
        "specular extension declared, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_texture_transform\""),
        "nested texture-transform must trigger the KHR_texture_transform declaration, got: {raw}"
    );

    // Re-decode: the document we just wrote must be §3.12-valid (the
    // decoder runs validate_extension_stack), and the transform must
    // still be on the specularTexture.
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let sp = decoded.materials[0]
        .extras
        .get("KHR_materials_specular")
        .and_then(|v| v.as_object())
        .expect("specular block present");
    let tt = sp
        .get("specularTexture")
        .and_then(|v| v.get("extensions"))
        .and_then(|v| v.get("KHR_texture_transform"))
        .and_then(|v| v.as_object())
        .expect("nested transform survives round-trip");
    let off = tt.get("offset").and_then(|v| v.as_array()).unwrap();
    assert!((off[0].as_f64().unwrap() - 0.1).abs() < 1e-6);
    assert!((off[1].as_f64().unwrap() - 0.2).abs() < 1e-6);
    assert!((tt.get("rotation").and_then(|v| v.as_f64()).unwrap() - 0.5).abs() < 1e-5);
}

#[test]
fn transform_nested_in_clearcoat_normal_texture_roundtrips() {
    // clearcoatNormalTexture is a `normalTextureInfo` (it carries an
    // optional `scale`), so it travels the encoder's
    // `normal_texture_info_from_value` re-emission path rather than the
    // plain textureInfo one — exercise that the nested transform AND the
    // `scale` both survive the glb round-trip and the extension is
    // declared.
    let mut scene = Scene3D::new();
    scene.add_texture(dummy_texture());
    let mut mat = Material::new();
    mat.extras.insert(
        "KHR_materials_clearcoat".to_owned(),
        serde_json::json!({
            "clearcoatFactor": 1.0,
            "clearcoatNormalTexture": {
                "index": 0,
                "scale": 0.75,
                "extensions": {
                    "KHR_texture_transform": { "scale": [3.0, 3.0] }
                }
            }
        }),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        raw.contains("\"KHR_texture_transform\""),
        "nested transform on clearcoatNormalTexture triggers the declaration, got: {raw}"
    );

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let cc = decoded.materials[0]
        .extras
        .get("KHR_materials_clearcoat")
        .and_then(|v| v.as_object())
        .expect("clearcoat block present");
    let nt = cc
        .get("clearcoatNormalTexture")
        .and_then(|v| v.as_object())
        .expect("clearcoatNormalTexture present");
    assert!((nt.get("scale").and_then(|v| v.as_f64()).unwrap() - 0.75).abs() < 1e-6);
    let tt = nt
        .get("extensions")
        .and_then(|v| v.get("KHR_texture_transform"))
        .and_then(|v| v.get("scale"))
        .and_then(|v| v.as_array())
        .expect("nested transform scale survives the normalTextureInfo path");
    assert!((tt[0].as_f64().unwrap() - 3.0).abs() < 1e-6);
    assert!((tt[1].as_f64().unwrap() - 3.0).abs() < 1e-6);
}

#[test]
fn transform_nested_in_extension_slot_without_extensions_used_is_rejected() {
    // KHR_texture_transform appears ONLY inside
    // KHR_materials_clearcoat.clearcoatTexture, and KHR_texture_transform
    // is not in extensionsUsed — spec §3.12 violation. Before this round
    // the §3.12 scan only looked at the five core PBR slots and let this
    // slip through.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "textures": [{ "source": 0 }],
        "images": [{ "uri": "data:image/png;base64,AAAA" }],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_clearcoat": {
                        "clearcoatFactor": 1.0,
                        "clearcoatTexture": {
                            "index": 0,
                            "extensions": {
                                "KHR_texture_transform": { "scale": [2, 2] }
                            }
                        }
                    }
                }
            }
        ]
    }"#;
    let err = GltfDecoder::new().decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_texture_transform"),
        "expected ExtensionStackUsedNotDeclared for the nested KHR_texture_transform, got {msg}"
    );
}

#[test]
fn transform_nested_in_extension_slot_with_extensions_used_is_accepted() {
    // Same as above but with KHR_texture_transform properly declared —
    // must decode cleanly.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat", "KHR_texture_transform"],
        "textures": [{ "source": 0 }],
        "images": [{ "uri": "data:image/png;base64,AAAA" }],
        "materials": [
            {
                "extensions": {
                    "KHR_materials_clearcoat": {
                        "clearcoatFactor": 1.0,
                        "clearcoatTexture": {
                            "index": 0,
                            "extensions": {
                                "KHR_texture_transform": { "scale": [2, 2] }
                            }
                        }
                    }
                }
            }
        ]
    }"#;
    let scene = GltfDecoder::new().decode(json).unwrap();
    assert_eq!(scene.materials.len(), 1);
}

#[test]
fn large_finite_rotation_roundtrips() {
    // A large but finite rotation keeps the affine UV mat3 finite, so it
    // must pass the §Overview finiteness check and round-trip. (Non-finite
    // rotation can't be expressed in JSON, so the NaN / ±∞ rejection path
    // is covered by the validation.rs unit test
    // `texture_transform_non_finite_rotation_rejected`.)
    let mut scene = Scene3D::new();
    let tex_id = scene.add_texture(dummy_texture());
    let mut mat = Material::new();
    mat.emissive_texture = Some(TextureRef::new(tex_id));
    let mut obj = serde_json::Map::new();
    obj.insert("rotation".to_owned(), Value::from(1.0e30_f64));
    mat.extras.insert(
        "KHR_texture_transform:emissive".to_owned(),
        Value::Object(obj),
    );
    scene.add_material(mat);
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.materials.len(), 1);
}

#[test]
fn transform_on_normal_slot_roundtrips_with_scale_too() {
    let mut scene = Scene3D::new();
    let tex_id = scene.add_texture(dummy_texture());
    let mut mat = Material::new();
    mat.normal_texture = Some(TextureRef::new(tex_id));
    mat.normal_scale = 1.5; // distinct from the default to confirm both round-trip
    let mut obj = serde_json::Map::new();
    obj.insert(
        "offset".to_owned(),
        Value::Array(vec![Value::from(0.1_f64), Value::from(0.2_f64)]),
    );
    mat.extras.insert(
        "KHR_texture_transform:normal".to_owned(),
        Value::Object(obj),
    );
    scene.add_material(mat);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    let dm = &decoded.materials[0];
    assert!(
        (dm.normal_scale - 1.5).abs() < 1e-6,
        "normal scale survives transform integration, got {}",
        dm.normal_scale
    );
    let obj = transform_object(dm, "normal").expect("normal slot transform present");
    let offset = obj.get("offset").and_then(|v| v.as_array()).unwrap();
    assert!((offset[0].as_f64().unwrap() - 0.1).abs() < 1e-6);
    assert!((offset[1].as_f64().unwrap() - 0.2).abs() < 1e-6);
}
