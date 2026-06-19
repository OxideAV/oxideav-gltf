//! End-to-end texture / material reference validation per glTF 2.0
//! §5.29 (Texture), §5.30 (Texture Info) and §5.22 (Material PBR
//! Metallic Roughness) (round r346).
//!
//! Each index a texture or material carries into another top-level
//! array MUST resolve to a real entry. The `u32` field types pin the
//! `>= 0` minimum automatically; these tests cover the upper-bound MUST
//! the decoder previously did not police. The unit tests in
//! `src/validation.rs::tests` exercise the per-rule logic; these tests
//! pin the wiring inside `convert()`.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A 1×1 white PNG as a base64 data URI, usable as an `images[]` source
/// so a well-formed texture reference can fully decode.
const PNG_1X1_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn decode_err(body: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(body.as_bytes())
        .expect_err("malformed texture/material reference should have been rejected");
    format!("{err}")
}

fn decode_ok(body: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(body.as_bytes())
        .unwrap_or_else(|e| panic!("expected valid document, got: {e}"));
}

// ----------------------------------------------------------------------
// §5.29.1 — texture.source
// ----------------------------------------------------------------------

#[test]
fn texture_source_out_of_range_rejected() {
    // texture.source = 0 but no images declared.
    let body = r#"{
        "asset": { "version": "2.0" },
        "textures": [ { "source": 0 } ]
    }"#;
    let err = decode_err(body);
    assert!(err.contains("TextureSourceIndex"), "got: {err}");
}

// ----------------------------------------------------------------------
// §5.29.2 — texture.sampler
// ----------------------------------------------------------------------

#[test]
fn texture_sampler_out_of_range_rejected() {
    let body = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "images": [ {{ "uri": "data:image/png;base64,{PNG_1X1_B64}" }} ],
        "textures": [ {{ "source": 0, "sampler": 4 }} ]
    }}"#
    );
    let err = decode_err(&body);
    assert!(err.contains("TextureSamplerIndex"), "got: {err}");
}

// ----------------------------------------------------------------------
// §5.30.1 — material textureInfo.index
// ----------------------------------------------------------------------

#[test]
fn material_base_color_texture_out_of_range_rejected() {
    let body = r#"{
        "asset": { "version": "2.0" },
        "materials": [ { "pbrMetallicRoughness": { "baseColorTexture": { "index": 2 } } } ]
    }"#;
    let err = decode_err(body);
    assert!(err.contains("MaterialTextureIndex"), "got: {err}");
    assert!(
        err.contains("baseColorTexture"),
        "slot named in error: {err}"
    );
}

#[test]
fn material_normal_texture_out_of_range_rejected() {
    let body = r#"{
        "asset": { "version": "2.0" },
        "materials": [ { "normalTexture": { "index": 0 } } ]
    }"#;
    let err = decode_err(body);
    assert!(err.contains("MaterialTextureIndex"), "got: {err}");
    assert!(err.contains("normalTexture"), "slot named in error: {err}");
}

#[test]
fn material_occlusion_texture_out_of_range_rejected() {
    let body = r#"{
        "asset": { "version": "2.0" },
        "materials": [ { "occlusionTexture": { "index": 7 } } ]
    }"#;
    let err = decode_err(body);
    assert!(err.contains("MaterialTextureIndex"), "got: {err}");
    assert!(
        err.contains("occlusionTexture"),
        "slot named in error: {err}"
    );
}

#[test]
fn material_emissive_texture_out_of_range_rejected() {
    let body = r#"{
        "asset": { "version": "2.0" },
        "materials": [ { "emissiveTexture": { "index": 1 } } ]
    }"#;
    let err = decode_err(body);
    assert!(err.contains("MaterialTextureIndex"), "got: {err}");
    assert!(
        err.contains("emissiveTexture"),
        "slot named in error: {err}"
    );
}

#[test]
fn material_metallic_roughness_texture_out_of_range_rejected() {
    let body = r#"{
        "asset": { "version": "2.0" },
        "materials": [ { "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 3 } } } ]
    }"#;
    let err = decode_err(body);
    assert!(err.contains("MaterialTextureIndex"), "got: {err}");
    assert!(
        err.contains("metallicRoughnessTexture"),
        "slot named in error: {err}"
    );
}

// ----------------------------------------------------------------------
// well-formed references round-trip
// ----------------------------------------------------------------------

#[test]
fn well_formed_texture_and_material_references_pass() {
    // One image, one sampler, one texture pointing at both, and a
    // material whose baseColorTexture resolves to texture 0.
    let body = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "images": [ {{ "uri": "data:image/png;base64,{PNG_1X1_B64}" }} ],
        "samplers": [ {{}} ],
        "textures": [ {{ "source": 0, "sampler": 0 }} ],
        "materials": [ {{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }} }} ]
    }}"#
    );
    decode_ok(&body);
}
