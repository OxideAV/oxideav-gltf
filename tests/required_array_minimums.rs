//! Required-array `minItems: 1` schema MUSTs per glTF 2.0:
//!
//! - `mesh.primitives` (§5.24.1, required) — `MeshPrimitivesEmpty`
//! - `animation.channels` (§5.4, required) — `AnimationChannelsEmpty`
//! - `animation.samplers` (§5.4, required) — `AnimationSamplersEmpty`
//!
//! These arrays are `Required: Yes`, so an empty (or absent → empty)
//! array is always a violation.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

#[test]
fn empty_mesh_primitives_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "meshes": [ { "primitives": [] } ],
        "nodes": [ { "mesh": 0 } ],
        "scenes": [ { "nodes": [0] } ], "scene": 0
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("a mesh with no primitives must be rejected");
    assert!(
        format!("{err}").contains("MeshPrimitivesEmpty"),
        "got: {err}"
    );
}

#[test]
fn empty_animation_channels_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "animations": [ { "channels": [], "samplers": [
            { "input": 0, "output": 0 }
        ] } ],
        "nodes": [ {} ],
        "scenes": [ { "nodes": [0] } ], "scene": 0
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("an animation with no channels must be rejected");
    assert!(
        format!("{err}").contains("AnimationChannelsEmpty"),
        "got: {err}"
    );
}

#[test]
fn empty_animation_samplers_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "animations": [ { "channels": [
            { "sampler": 0, "target": { "node": 0, "path": "translation" } }
        ], "samplers": [] } ],
        "nodes": [ { "translation": [0, 0, 0] } ],
        "scenes": [ { "nodes": [0] } ], "scene": 0
    }"#;
    let err = GltfDecoder::new()
        .decode(json)
        .expect_err("an animation with no samplers must be rejected");
    // An empty `samplers` array on a channel-bearing animation is rejected
    // by whichever §-MUST fires first: the channel's `sampler` index
    // resolution (`AnimationChannelSampler`, run earlier in the pipeline)
    // or the `minItems: 1` structural check (`AnimationSamplersEmpty`).
    // Both are spec-correct rejections of the same document; the
    // standalone `AnimationSamplersEmpty` branch is exercised directly by
    // the unit test in `src/validation.rs::tests`.
    let msg = format!("{err}");
    assert!(
        msg.contains("AnimationSamplersEmpty") || msg.contains("AnimationChannelSampler"),
        "got: {msg}"
    );
}

#[test]
fn absent_animations_array_accepted() {
    // No animations at all is fine — the minItems rule only bites a
    // present-but-empty channels/samplers array.
    let json = br#"{
        "asset": { "version": "2.0" },
        "nodes": [ {} ],
        "scenes": [ { "nodes": [0] } ], "scene": 0
    }"#;
    GltfDecoder::new()
        .decode(json)
        .expect("a document with no animations must decode");
}
