//! Public-API coverage for round 7's new validators (spec §3.11 +
//! §3.12) and the JSON fuzz-resistance caps in `validation::check_*`.
//!
//! Each test feeds the decoder with a hand-built JSON document that
//! exercises exactly one failure mode and asserts the resulting error
//! message carries the documented prefix.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// `extensionsRequired` listing an extension that's not in
/// `extensionsUsed` violates spec §3.12.
#[test]
fn rejects_required_extension_not_in_used() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsRequired": ["KHR_materials_ior"]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("ExtensionStackRequiredNotListed"),
        "expected ExtensionStackRequiredNotListed, got {err}"
    );
}

/// A document that carries a `KHR_lights_punctual` data block at root
/// scope but omits the extension from `extensionsUsed` violates spec
/// §3.12.
#[test]
fn rejects_root_lights_punctual_without_used_declaration() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensions": {
            "KHR_lights_punctual": { "lights": [] }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("ExtensionStackUsedNotDeclared"),
        "expected ExtensionStackUsedNotDeclared, got {err}"
    );
}

/// Same data block, properly declared in `extensionsUsed`, must
/// decode without error.
#[test]
fn accepts_root_lights_punctual_when_used_lists_it() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_lights_punctual"],
        "extensions": {
            "KHR_lights_punctual": { "lights": [] }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    dec.decode(json)
        .expect("declared extension decodes cleanly");
}

/// Animation channel with an unknown `path` (one of the four reserved
/// keywords is required) violates spec §3.11.
#[test]
fn rejects_unknown_animation_channel_path() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "buffers": [ { "byteLength": 16,
                       "uri": "data:application/octet-stream;base64,AAAAAAAAgD8AAABAAABAQA==" } ],
        "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 16 } ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR",
              "min": [0.0], "max": [1.0] },
            { "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR",
              "min": [0.0], "max": [1.0] }
        ],
        "nodes": [ {} ],
        "animations": [
            {
                "channels": [
                    { "sampler": 0,
                      "target": { "node": 0, "path": "zoom" } }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("AnimationChannelPath"),
        "expected AnimationChannelPath, got {err}"
    );
}

/// `path == "weights"` targeting a node without a mesh violates spec
/// §3.11.
#[test]
fn rejects_weights_channel_targeting_node_without_mesh() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "buffers": [ { "byteLength": 16,
                       "uri": "data:application/octet-stream;base64,AAAAAAAAgD8AAABAAABAQA==" } ],
        "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 16 } ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR",
              "min": [0.0], "max": [1.0] },
            { "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR",
              "min": [0.0], "max": [1.0] }
        ],
        "nodes": [ {} ],
        "animations": [
            {
                "channels": [
                    { "sampler": 0,
                      "target": { "node": 0, "path": "weights" } }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("AnimationChannelWeightsNoMesh"),
        "expected AnimationChannelWeightsNoMesh, got {err}"
    );
}

/// Sampler index out of range — surfaces the AnimationChannelSampler
/// error prefix from the validator (rather than a downstream decode
/// error).
#[test]
fn rejects_animation_channel_with_out_of_range_sampler() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "animations": [
            {
                "channels": [
                    { "sampler": 9,
                      "target": { "node": 0, "path": "translation" } }
                ],
                "samplers": []
            }
        ],
        "nodes": [ {} ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    assert!(
        format!("{err}").contains("AnimationChannelSampler"),
        "expected AnimationChannelSampler, got {err}"
    );
}

/// Excessive JSON nesting depth must trip the fuzz-hardening cap
/// before serde sees it.
#[test]
fn rejects_deeply_nested_json_bomb() {
    let mut s: Vec<u8> = vec![b'['; 400];
    s.extend(std::iter::repeat_n(b']', 400));
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&s).unwrap_err();
    assert!(
        format!("{err}").contains("JsonDepthExceeded"),
        "expected JsonDepthExceeded, got {err}"
    );
}

/// Round-tripping a glTF JSON document through encode + decode after
/// the validators are active must still succeed (smoke check —
/// catches regressions that would mis-prefix the validator name).
#[test]
fn empty_scene_round_trips_cleanly() {
    let mut enc = oxideav_gltf::GltfEncoder::new();
    let bytes = oxideav_mesh3d::Mesh3DEncoder::encode(&mut enc, &oxideav_mesh3d::Scene3D::new())
        .expect("empty scene encodes");
    let mut dec = GltfDecoder::new();
    let _ = dec.decode(&bytes).expect("empty scene decodes back");
}
