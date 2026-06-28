//! Material-extension textureInfo `index` resolution per glTF 2.0
//! §5.30.1: every material textureInfo's `index` MUST resolve into the
//! document's `textures[]` array. The core PBR slots were already
//! policed (`MaterialTextureIndex`); this covers the textureInfos nested
//! inside the KHR material extensions (specular / clearcoat / sheen /
//! transmission / volume / iridescence / anisotropy /
//! diffuse-transmission), which surface a dangling reference as
//! `MaterialExtensionTextureIndex`.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

#[test]
fn specular_texture_in_range_accepted() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_specular"],
        "images": [ { "uri": "data:image/png;base64,AAAA" } ],
        "samplers": [{}],
        "textures": [ { "source": 0, "sampler": 0 } ],
        "materials": [
            { "extensions": { "KHR_materials_specular": {
                "specularTexture": { "index": 0 }
            } } }
        ]
    }"#;
    GltfDecoder::new()
        .decode(json)
        .expect("an in-range specularTexture index must decode");
}

#[test]
fn specular_texture_out_of_range_rejected() {
    // textures[] has one entry (index 0); specularTexture points at 1.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_specular"],
        "images": [ { "uri": "data:image/png;base64,AAAA" } ],
        "samplers": [{}],
        "textures": [ { "source": 0, "sampler": 0 } ],
        "materials": [
            { "extensions": { "KHR_materials_specular": {
                "specularTexture": { "index": 1 }
            } } }
        ]
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("an out-of-range specularTexture index must be rejected");
    assert!(
        format!("{err}").contains("MaterialExtensionTextureIndex"),
        "got: {err}"
    );
}

#[test]
fn clearcoat_normal_texture_out_of_range_rejected() {
    // clearcoatNormalTexture (a normalTextureInfo) at index 3 with an
    // empty textures roster.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "materials": [
            { "extensions": { "KHR_materials_clearcoat": {
                "clearcoatFactor": 1.0,
                "clearcoatNormalTexture": { "index": 3 }
            } } }
        ]
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("an out-of-range clearcoatNormalTexture index must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("MaterialExtensionTextureIndex") && msg.contains("clearcoatNormalTexture"),
        "got: {msg}"
    );
}

#[test]
fn transmission_texture_out_of_range_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_transmission"],
        "materials": [
            { "extensions": { "KHR_materials_transmission": {
                "transmissionFactor": 0.5,
                "transmissionTexture": { "index": 7 }
            } } }
        ]
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("an out-of-range transmissionTexture index must be rejected");
    assert!(
        format!("{err}").contains("MaterialExtensionTextureIndex"),
        "got: {err}"
    );
}
