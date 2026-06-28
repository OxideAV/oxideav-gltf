//! Core material factor / scalar range validation per glTF 2.0
//! §5.19–§5.22 (the JSON schema's closed-range MUSTs the typed `f32`
//! representation does not enforce):
//!
//! - baseColorFactor / metallicFactor / roughnessFactor / emissiveFactor
//!   each in [0, 1]
//! - alphaCutoff >= 0
//! - occlusionTexture.strength in [0, 1]
//! - normalTexture.scale finite
//!
//! Out-of-range values surface as `Material…Range` / `Material…Finite`.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

fn doc(materials: &str) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "materials": [ {materials} ]
    }}"#
    )
    .into_bytes()
}

fn decode_err(materials: &str) -> String {
    let err = GltfDecoder::new()
        .decode(&doc(materials))
        .expect_err("out-of-range material factor must be rejected");
    format!("{err}")
}

#[test]
fn in_range_factors_accepted() {
    GltfDecoder::new()
        .decode(&doc(r#"{
                "pbrMetallicRoughness": {
                    "baseColorFactor": [0.2, 0.4, 0.6, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0
                },
                "emissiveFactor": [0.1, 0.0, 0.5],
                "alphaMode": "MASK",
                "alphaCutoff": 0.25
            }"#))
        .expect("a fully in-range material must decode");
}

#[test]
fn base_color_factor_above_one_rejected() {
    let err =
        decode_err(r#"{ "pbrMetallicRoughness": { "baseColorFactor": [1.5, 0.0, 0.0, 1.0] } }"#);
    assert!(err.contains("MaterialBaseColorFactorRange"), "got: {err}");
}

#[test]
fn base_color_factor_negative_rejected() {
    let err =
        decode_err(r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0.0, -0.1, 0.0, 1.0] } }"#);
    assert!(err.contains("MaterialBaseColorFactorRange"), "got: {err}");
}

#[test]
fn metallic_factor_above_one_rejected() {
    let err = decode_err(r#"{ "pbrMetallicRoughness": { "metallicFactor": 2.0 } }"#);
    assert!(err.contains("MaterialMetallicFactorRange"), "got: {err}");
}

#[test]
fn roughness_factor_negative_rejected() {
    let err = decode_err(r#"{ "pbrMetallicRoughness": { "roughnessFactor": -0.5 } }"#);
    assert!(err.contains("MaterialRoughnessFactorRange"), "got: {err}");
}

#[test]
fn emissive_factor_above_one_rejected() {
    let err = decode_err(r#"{ "emissiveFactor": [0.0, 1.2, 0.0] }"#);
    assert!(err.contains("MaterialEmissiveFactorRange"), "got: {err}");
}

#[test]
fn negative_alpha_cutoff_rejected() {
    let err = decode_err(r#"{ "alphaMode": "MASK", "alphaCutoff": -0.1 }"#);
    assert!(err.contains("MaterialAlphaCutoffRange"), "got: {err}");
}

#[test]
fn occlusion_strength_above_one_rejected() {
    // index 0 is a dangling texture ref, but the material-range pass runs
    // before texture resolution only catches the strength; to isolate the
    // strength rule we give a textures roster of one entry.
    let json = br#"{
        "asset": { "version": "2.0" },
        "images": [ { "uri": "data:image/png;base64,AAAA" } ],
        "textures": [ { "source": 0 } ],
        "materials": [
            { "occlusionTexture": { "index": 0, "strength": 1.5 } }
        ]
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("occlusion strength > 1 must be rejected");
    assert!(
        format!("{err}").contains("MaterialOcclusionStrengthRange"),
        "got: {err}"
    );
}

#[test]
fn occlusion_strength_in_range_accepted() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "images": [ { "uri": "data:image/png;base64,AAAA" } ],
        "textures": [ { "source": 0 } ],
        "materials": [
            { "occlusionTexture": { "index": 0, "strength": 0.5 } }
        ]
    }"#;
    GltfDecoder::new()
        .decode(json)
        .expect("occlusion strength 0.5 must decode");
}
