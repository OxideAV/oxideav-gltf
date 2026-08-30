//! Sampled-`MorphWeights` channels through the typed `oxideav-mesh3d`
//! 0.0.6 synthesis path.
//!
//! * Decode builds every `weights` channel through
//!   `AnimationSampler::morph_weights` / `morph_weights_cubic`, with
//!   the per-keyframe stride = the driven mesh's morph-target count
//!   (§3.11 "A morph target animation frame is defined by a sequence
//!   of scalars of length equal to the number of targets").
//! * Encode reads the frames back through `morph_weight_frames` /
//!   `morph_weight_cubic_frame` and re-flattens them in §3.6 wire
//!   order, rejecting a sampler whose stride disagrees with the mesh.
//! * The read-back is lossless: a channel authored purely through the
//!   typed constructors + `Animation::with_channel` writes exactly the
//!   frames it was given and reads them back after a round trip.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Animation, AnimationProperty, AnimationSampler, AnimationValues, Interpolation, Mesh,
    Mesh3DDecoder, Mesh3DEncoder, MorphTarget, Node, NodeId, Primitive, Scene3D, Topology,
};

/// A scene with one triangle carrying `targets` zero-delta morph
/// targets, instantiated by one root node; returns the node id.
fn morph_scene(targets: usize) -> (Scene3D, NodeId) {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for i in 0..targets {
        let mut t = MorphTarget::new();
        t.position = Some(vec![[0.1 * (i + 1) as f32, 0.0, 0.0]; 3]);
        prim.targets.push(t);
    }
    let mesh = Mesh::new(Some("morphy".to_owned())).with_primitive(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    (scene, n)
}

fn glb_json(glb: &[u8]) -> serde_json::Value {
    let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    serde_json::from_slice(&glb[20..20 + n]).unwrap()
}

fn round_trip(scene: &Scene3D) -> (Scene3D, Vec<u8>) {
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(scene).expect("encode");
    let mut dec = GltfDecoder::new();
    (dec.decode(&glb).expect("decode"), glb)
}

#[test]
fn linear_frames_survive_round_trip_losslessly() {
    let (mut scene, n) = morph_scene(2);
    let frames = vec![vec![0.0, 0.0], vec![0.5, -0.25], vec![1.0, 1.5]];
    let sampler =
        AnimationSampler::morph_weights(vec![0.0, 0.5, 1.0], frames.clone(), Interpolation::Linear)
            .expect("well-formed sampler");
    scene.add_animation(Animation::new(Some("blend".to_owned())).with_channel(
        n,
        AnimationProperty::MorphWeights,
        sampler,
    ));
    scene.validate().expect("authored scene validates");

    let (decoded, glb) = round_trip(&scene);
    let ch = decoded.animations[0]
        .channel_for(n, AnimationProperty::MorphWeights)
        .expect("weights channel present");
    assert_eq!(ch.sampler.interpolation, Interpolation::Linear);
    assert_eq!(ch.sampler.morph_weight_stride(), Some(2));
    let back: Vec<Vec<f32>> = ch
        .sampler
        .morph_weight_frames()
        .unwrap()
        .into_iter()
        .map(<[f32]>::to_vec)
        .collect();
    assert_eq!(back, frames, "authored frames read back verbatim");
    assert_eq!(ch.sampler.morph_weight_frame(1), Some(&[0.5f32, -0.25][..]));
    // The wire layout is the flat row-major stream §3.11 describes.
    assert_eq!(
        ch.sampler.values,
        AnimationValues::Scalar(vec![0.0, 0.0, 0.5, -0.25, 1.0, 1.5])
    );
    // Fixed point.
    let (_, glb2) = round_trip(&decoded);
    assert_eq!(glb_json(&glb2), glb_json(&glb));
    assert_eq!(glb2, glb);
}

#[test]
fn step_frames_survive_round_trip_losslessly() {
    let (mut scene, n) = morph_scene(3);
    let frames = vec![
        vec![1.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0],
        vec![0.0, 0.0, 1.0],
    ];
    let sampler =
        AnimationSampler::morph_weights(vec![0.0, 1.0, 2.0], frames.clone(), Interpolation::Step)
            .unwrap();
    scene.add_animation(Animation::new(Some("poses".to_owned())).with_channel(
        n,
        AnimationProperty::MorphWeights,
        sampler,
    ));
    let (decoded, glb) = round_trip(&scene);
    assert_eq!(
        glb_json(&glb)["animations"][0]["samplers"][0]["interpolation"],
        "STEP"
    );
    let ch = decoded.animations[0]
        .channel_for(n, AnimationProperty::MorphWeights)
        .unwrap();
    assert_eq!(ch.sampler.interpolation, Interpolation::Step);
    let back: Vec<Vec<f32>> = ch
        .sampler
        .morph_weight_frames()
        .unwrap()
        .into_iter()
        .map(<[f32]>::to_vec)
        .collect();
    assert_eq!(back, frames);
}

#[test]
fn cubic_triples_survive_round_trip_losslessly() {
    let (mut scene, n) = morph_scene(2);
    let in_t = vec![vec![0.0, 0.0], vec![0.1, -0.1], vec![0.0, 0.0]];
    let val = vec![vec![0.0, 1.0], vec![0.5, 0.5], vec![1.0, 0.0]];
    let out_t = vec![vec![0.2, -0.2], vec![0.3, -0.3], vec![0.0, 0.0]];
    let sampler = AnimationSampler::morph_weights_cubic(
        vec![0.0, 0.5, 1.0],
        in_t.clone(),
        val.clone(),
        out_t.clone(),
    )
    .unwrap();
    scene.add_animation(Animation::new(Some("smooth".to_owned())).with_channel(
        n,
        AnimationProperty::MorphWeights,
        sampler,
    ));
    scene.validate().expect("authored scene validates");

    let (decoded, glb) = round_trip(&scene);
    let json = glb_json(&glb);
    assert_eq!(
        json["animations"][0]["samplers"][0]["interpolation"],
        "CUBICSPLINE"
    );
    // §3.11 — output count = 3 × keyframes × targets.
    assert_eq!(
        json["accessors"][json["animations"][0]["samplers"][0]["output"]
            .as_u64()
            .unwrap() as usize]["count"],
        18
    );
    let ch = decoded.animations[0]
        .channel_for(n, AnimationProperty::MorphWeights)
        .unwrap();
    assert_eq!(ch.sampler.interpolation, Interpolation::CubicSpline);
    assert_eq!(ch.sampler.morph_weight_stride(), Some(2));
    for k in 0..3 {
        let (a, v, b) = ch.sampler.morph_weight_cubic_frame(k).unwrap();
        assert_eq!(a, &in_t[k][..], "in-tangent {k}");
        assert_eq!(v, &val[k][..], "value {k}");
        assert_eq!(b, &out_t[k][..], "out-tangent {k}");
        assert_eq!(ch.sampler.morph_weight_frame(k), Some(&val[k][..]));
    }
    let (_, glb2) = round_trip(&decoded);
    assert_eq!(glb2, glb, "fixed point");
}

#[test]
fn decoded_weights_channel_regroups_by_mesh_target_count() {
    // A hand-built flat stream (the pre-typed authoring shape) still
    // encodes, and the decoded channel exposes the typed frame view
    // with stride = target count.
    let (mut scene, n) = morph_scene(2);
    let mut anim = Animation::new(Some("flat".to_owned()));
    anim.channels.push(oxideav_mesh3d::AnimationChannel::new(
        n,
        AnimationProperty::MorphWeights,
        AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Scalar(vec![0.25, 0.75, 1.0, 0.0]),
            interpolation: Interpolation::Linear,
        },
    ));
    scene.add_animation(anim);
    let (decoded, _) = round_trip(&scene);
    let ch = &decoded.animations[0].channels[0];
    assert_eq!(ch.sampler.morph_weight_stride(), Some(2));
    assert_eq!(ch.sampler.morph_weight_frame(0), Some(&[0.25f32, 0.75][..]));
    assert_eq!(ch.sampler.morph_weight_frame(1), Some(&[1.0f32, 0.0][..]));
}

#[test]
fn encode_rejects_stride_disagreeing_with_mesh() {
    // 2 targets, but frames are 3 wide.
    let (mut scene, n) = morph_scene(2);
    let sampler = AnimationSampler::morph_weights(
        vec![0.0, 1.0],
        vec![vec![0.0, 0.0, 0.0], vec![1.0, 1.0, 1.0]],
        Interpolation::Linear,
    )
    .unwrap();
    scene.add_animation(Animation::new(None).with_channel(
        n,
        AnimationProperty::MorphWeights,
        sampler,
    ));
    let mut enc = GltfEncoder::new();
    let err = enc.encode(&scene).unwrap_err();
    assert!(
        format!("{err}").contains("AnimationSamplerOutputCount"),
        "unexpected error: {err}"
    );
}

#[test]
fn encode_rejects_stream_that_does_not_regroup() {
    // 2 keyframes × 2 targets needs 4 values; 3 cannot regroup.
    let (mut scene, n) = morph_scene(2);
    let mut anim = Animation::new(None);
    anim.channels.push(oxideav_mesh3d::AnimationChannel::new(
        n,
        AnimationProperty::MorphWeights,
        AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Scalar(vec![0.0, 0.5, 1.0]),
            interpolation: Interpolation::Linear,
        },
    ));
    scene.add_animation(anim);
    let mut enc = GltfEncoder::new();
    let err = enc.encode(&scene).unwrap_err();
    assert!(
        format!("{err}").contains("AnimationSamplerOutputCount"),
        "unexpected error: {err}"
    );
}

#[test]
fn encode_rejects_weights_channel_on_meshless_node() {
    let (mut scene, _n) = morph_scene(1);
    let bare = scene.add_node(Node::new());
    scene.add_root(bare);
    let sampler = AnimationSampler::morph_weights(
        vec![0.0, 1.0],
        vec![vec![0.0], vec![1.0]],
        Interpolation::Linear,
    )
    .unwrap();
    scene.add_animation(Animation::new(None).with_channel(
        bare,
        AnimationProperty::MorphWeights,
        sampler,
    ));
    let mut enc = GltfEncoder::new();
    let err = enc.encode(&scene).unwrap_err();
    assert!(
        format!("{err}").contains("AnimationChannelWeightsNoMesh"),
        "unexpected error: {err}"
    );
}
