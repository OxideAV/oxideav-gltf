//! End-to-end animation-sampler keyframe-time validation per glTF 2.0
//! §3.11: "The values represent time in seconds with `time[0] >= 0.0`,
//! and strictly increasing values, i.e., `time[n + 1] > time[n]`."
//!
//! These checks run on the decoder side against the *materialised*
//! `&[f32]` keyframe timestamps (the ordering rule cannot be decided
//! from JSON metadata alone). A spec-non-conformant timeline surfaces
//! as `Error::InvalidData` with a stable `AnimationSamplerInputTime…`
//! prefix.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a minimal `.gltf` JSON document with a single node, a single
/// translation animation channel, and a LINEAR sampler whose input
/// accessor reads `times` (the keyframe timestamps) and whose output
/// accessor reads one VEC3 per keyframe.
///
/// The binary buffer layout is: `[ times: N×f32 SCALAR ]` then
/// `[ outputs: N×VEC3 f32 ]`. `min`/`max` on the input accessor are
/// derived from the supplied slice so the structural §3.11 min/max MUST
/// is satisfied — leaving the ordering rule as the thing under test.
fn build_anim_doc(times: &[f32]) -> Vec<u8> {
    let n = times.len();
    let mut bin = Vec::new();
    for &t in times {
        bin.extend_from_slice(&t.to_le_bytes());
    }
    let input_len = bin.len();
    // One translation VEC3 per keyframe (all zero — value irrelevant).
    for _ in 0..n {
        bin.extend_from_slice(&[0u8; 12]);
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);

    // min/max for the input accessor — JSON cannot encode non-finite
    // numbers, so clamp the literals to finite placeholders. The validator
    // under test reads the materialised f32 bytes, not these bounds, so
    // their exact value is irrelevant to the ordering rule.
    let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
    let tmin = finite(times.iter().cloned().fold(f32::INFINITY, f32::min), 0.0);
    let tmax = finite(
        times.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
        1.0e30,
    );

    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {input_len} }},
            {{ "buffer": 0, "byteOffset": {input_len}, "byteLength": {output_len} }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": {n}, "type": "SCALAR",
               "min": [{tmin}], "max": [{tmax}] }},
            {{ "bufferView": 1, "componentType": 5126, "count": {n}, "type": "VEC3" }}
        ],
        "nodes": [ {{ "translation": [0, 0, 0] }} ],
        "animations": [ {{
            "channels": [ {{ "sampler": 0, "target": {{ "node": 0, "path": "translation" }} }} ],
            "samplers": [ {{ "input": 0, "output": 1, "interpolation": "LINEAR" }} ]
        }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#,
        output_len = n * 12,
    )
    .into_bytes()
}

#[test]
fn strictly_increasing_times_accepted() {
    let doc = build_anim_doc(&[0.0, 0.5, 1.0, 2.5]);
    let scene = GltfDecoder::new()
        .decode(&doc)
        .expect("monotonically increasing keyframe times must decode");
    assert_eq!(scene.animations.len(), 1);
    assert_eq!(scene.animations[0].channels[0].sampler.keyframes.len(), 4);
}

#[test]
fn single_keyframe_accepted() {
    // A one-element timeline trivially satisfies both MUSTs.
    let doc = build_anim_doc(&[0.0]);
    GltfDecoder::new()
        .decode(&doc)
        .expect("single keyframe at t=0 must decode");
}

#[test]
fn negative_first_time_rejected() {
    let doc = build_anim_doc(&[-0.001, 1.0]);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("time[0] < 0 must be rejected");
    assert!(
        format!("{err}").contains("AnimationSamplerInputTimeStart"),
        "got: {err}"
    );
}

#[test]
fn non_increasing_times_rejected() {
    // time[2] == time[1] — equal is not "strictly" increasing.
    let doc = build_anim_doc(&[0.0, 1.0, 1.0, 2.0]);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("equal consecutive keyframe times must be rejected");
    assert!(
        format!("{err}").contains("AnimationSamplerInputTimeOrder"),
        "got: {err}"
    );
}

#[test]
fn decreasing_times_rejected() {
    let doc = build_anim_doc(&[0.0, 2.0, 1.0]);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("decreasing keyframe times must be rejected");
    assert!(
        format!("{err}").contains("AnimationSamplerInputTimeOrder"),
        "got: {err}"
    );
}

#[test]
fn non_finite_time_rejected() {
    // An Infinity keyframe time fails the strictly-increasing scan
    // (x > Inf and Inf > x are both false).
    let doc = build_anim_doc(&[0.0, f32::INFINITY, 2.0]);
    let err = GltfDecoder::new()
        .decode(&doc)
        .expect_err("non-finite keyframe time must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("AnimationSamplerInputTimeOrder")
            || msg.contains("AnimationSamplerInputTimeStart"),
        "got: {msg}"
    );
}
