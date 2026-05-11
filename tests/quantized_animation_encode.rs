//! Encoder-side normalised-int animation outputs per glTF 2.0 §3.11
//! + §3.6.2.2 (round 4 — symmetric to r3 decode).
//!
//! `GltfEncoder::with_quantize_animation(QuantizeMode)` selects the
//! component type for ROTATION (VEC4) and MORPH_WEIGHTS (SCALAR)
//! sampler outputs:
//!
//! - `Float`  → 5126 (lossless, default)
//! - `UByte`  → 5121 normalized; `f = round(c / 255 inverse)`
//! - `UShort` → 5123 normalized; `f = round(c / 65535 inverse)`
//!
//! TRANSLATION + SCALE accessors are unaffected — the spec only allows
//! FLOAT for those paths. After re-encoding through the quantised
//! encoder we round-trip back through the decoder and check the
//! decoded values are within the spec equation's quantisation
//! tolerance of the source f32s.

use oxideav_gltf::{GltfDecoder, GltfEncoder, QuantizeMode};
use oxideav_mesh3d::{
    Animation, AnimationChannel, AnimationProperty, AnimationSampler, AnimationTarget,
    AnimationValues, Interpolation, Mesh, Mesh3DDecoder, Mesh3DEncoder, MorphTarget, Node,
    Primitive, Scene3D, Topology,
};

fn scene_with_morph_and_rotation() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    // r7: a `weights` animation channel requires the targeted mesh to
    // declare at least one morph target (spec §3.11). Add a zero-delta
    // POSITION target so the encoder-side validator (and the
    // round-trip decoder) both accept the document.
    prim.targets.push(MorphTarget {
        position: Some(vec![[0.0, 0.0, 0.0]; 3]),
        normal: None,
        tangent: None,
    });
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);

    let mut anim = Animation::new(Some("a".to_owned()));
    // 4 morph weight keyframes — values intentionally exercise the
    // 0/1 endpoints plus the half/quarter midpoints.
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::MorphWeights,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 0.25, 0.5, 0.75],
            values: AnimationValues::Scalar(vec![0.0, 0.25, 0.5, 1.0]),
            interpolation: Interpolation::Linear,
        },
    });
    // Rotation channel: 2 quaternions. First identity, second a
    // 45-degree rotation so xyzw are non-trivial.
    let s = std::f32::consts::FRAC_1_SQRT_2; // ~0.7071
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Rotation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Quat(vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, s, s]]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);
    scene
}

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    let chunk0_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    glb[20..20 + chunk0_len].to_vec()
}

fn count_normalized_int_animation_outputs(glb: &[u8], expected_ct: u32) -> usize {
    let json = extract_json_chunk(glb);
    let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let accs = v["accessors"].as_array().unwrap();
    accs.iter()
        .filter(|a| {
            a["componentType"] == expected_ct
                && a.get("normalized").and_then(|x| x.as_bool()) == Some(true)
        })
        .count()
}

#[test]
fn quantize_default_keeps_float() {
    // Default GltfEncoder::new() uses QuantizeMode::Float — no
    // accessor should carry a non-FLOAT animation output.
    let scene = scene_with_morph_and_rotation();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let n_ubyte = count_normalized_int_animation_outputs(&glb, 5121);
    let n_ushort = count_normalized_int_animation_outputs(&glb, 5123);
    assert_eq!(n_ubyte + n_ushort, 0);
}

#[test]
fn quantize_ubyte_morph_weights_emits_normalized() {
    let scene = scene_with_morph_and_rotation();
    let mut enc = GltfEncoder::new().with_quantize_animation(QuantizeMode::UByte);
    let glb = enc.encode(&scene).unwrap();
    // Both ROTATION (VEC4) and MORPH_WEIGHTS (SCALAR) outputs should
    // become UNSIGNED_BYTE normalized.
    let n_ubyte = count_normalized_int_animation_outputs(&glb, 5121);
    assert_eq!(
        n_ubyte, 2,
        "expected 2 UBYTE normalized accessors (rotation + morph), got {n_ubyte}"
    );

    // Decode it back and check tolerance: ubyte step is 1/255 ≈ 0.00392.
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let weights = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::MorphWeights))
        .expect("morph channel");
    let original_weights = [0.0f32, 0.25, 0.5, 1.0];
    if let AnimationValues::Scalar(v) = &weights.sampler.values {
        for (got, want) in v.iter().zip(original_weights.iter()) {
            assert!(
                (got - want).abs() <= 1.0 / 255.0,
                "ubyte morph weight: got {got}, want {want}"
            );
        }
    } else {
        panic!("expected Scalar morph values");
    }

    let rot = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::Rotation))
        .expect("rotation channel");
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let original_rot = [[0.0f32, 0.0, 0.0, 1.0], [0.0, 0.0, s, s]];
    if let AnimationValues::Quat(v) = &rot.sampler.values {
        for (got, want) in v.iter().zip(original_rot.iter()) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() <= 1.0 / 255.0,
                    "ubyte rotation comp {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    } else {
        panic!("expected Quat rotation values");
    }
}

#[test]
fn quantize_ushort_round_trip_within_tolerance() {
    let scene = scene_with_morph_and_rotation();
    let mut enc = GltfEncoder::new().with_quantize_animation(QuantizeMode::UShort);
    let glb = enc.encode(&scene).unwrap();
    let n_ushort = count_normalized_int_animation_outputs(&glb, 5123);
    assert_eq!(n_ushort, 2);

    // Decode and check tolerance — ushort step is 1/65535 ≈ 1.5e-5.
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let weights = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::MorphWeights))
        .expect("morph channel");
    let original_weights = [0.0f32, 0.25, 0.5, 1.0];
    if let AnimationValues::Scalar(v) = &weights.sampler.values {
        for (got, want) in v.iter().zip(original_weights.iter()) {
            assert!(
                (got - want).abs() <= 1.0 / 65535.0,
                "ushort morph weight: got {got}, want {want}"
            );
        }
    } else {
        panic!("expected Scalar morph values");
    }

    let rot = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::Rotation))
        .expect("rotation channel");
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let original_rot = [[0.0f32, 0.0, 0.0, 1.0], [0.0, 0.0, s, s]];
    if let AnimationValues::Quat(v) = &rot.sampler.values {
        for (got, want) in v.iter().zip(original_rot.iter()) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() <= 1.0 / 65535.0,
                    "ushort rotation comp {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    } else {
        panic!("expected Quat rotation values");
    }
}

/// Build a scene whose rotation channel exercises the full `[-1, 1]`
/// signed range — useful for testing IByte / IShort which are the
/// only quantize modes that can represent negative components.
fn scene_with_signed_rotation() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    // r7: a `weights` animation channel requires the mesh to declare
    // at least one morph target (spec §3.11).
    prim.targets.push(MorphTarget {
        position: Some(vec![[0.0, 0.0, 0.0]; 3]),
        normal: None,
        tangent: None,
    });
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);

    let mut anim = Animation::new(Some("a".to_owned()));
    let s = std::f32::consts::FRAC_1_SQRT_2; // ~0.7071
                                             // Quaternions intentionally include negative components so the
                                             // signed quantizers actually have signed work to do.
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Rotation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0, 2.0],
            values: AnimationValues::Quat(vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, s, s],
                [-s, 0.0, 0.0, s],
            ]),
            interpolation: Interpolation::Linear,
        },
    });
    // Morph weights with negative values too — spec allows the BYTE /
    // SHORT signed forms here.
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::MorphWeights,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0, 2.0, 3.0],
            values: AnimationValues::Scalar(vec![-1.0, -0.5, 0.5, 1.0]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);
    scene
}

#[test]
fn quantize_ibyte_round_trip_within_tolerance() {
    let scene = scene_with_signed_rotation();
    let mut enc = GltfEncoder::new().with_quantize_animation(QuantizeMode::IByte);
    let glb = enc.encode(&scene).unwrap();
    // Both ROTATION (VEC4) and MORPH_WEIGHTS (SCALAR) outputs should
    // become BYTE (5120) normalized.
    let n_byte = count_normalized_int_animation_outputs(&glb, 5120);
    assert_eq!(
        n_byte, 2,
        "expected 2 BYTE normalized accessors (rotation + morph), got {n_byte}"
    );

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let original_rot = [[0.0f32, 0.0, 0.0, 1.0], [0.0, 0.0, s, s], [-s, 0.0, 0.0, s]];
    let rot = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::Rotation))
        .expect("rotation channel");
    if let AnimationValues::Quat(v) = &rot.sampler.values {
        for (got, want) in v.iter().zip(original_rot.iter()) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() <= 1.0 / 127.0,
                    "ibyte rotation comp {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    } else {
        panic!("expected Quat rotation values");
    }

    let original_weights = [-1.0f32, -0.5, 0.5, 1.0];
    let weights = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::MorphWeights))
        .expect("morph channel");
    if let AnimationValues::Scalar(v) = &weights.sampler.values {
        for (got, want) in v.iter().zip(original_weights.iter()) {
            assert!(
                (got - want).abs() <= 1.0 / 127.0,
                "ibyte morph weight: got {got}, want {want}"
            );
        }
    } else {
        panic!("expected Scalar morph values");
    }
}

#[test]
fn quantize_ishort_round_trip_within_tolerance() {
    let scene = scene_with_signed_rotation();
    let mut enc = GltfEncoder::new().with_quantize_animation(QuantizeMode::IShort);
    let glb = enc.encode(&scene).unwrap();
    let n_short = count_normalized_int_animation_outputs(&glb, 5122);
    assert_eq!(
        n_short, 2,
        "expected 2 SHORT normalized accessors, got {n_short}"
    );

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let original_rot = [[0.0f32, 0.0, 0.0, 1.0], [0.0, 0.0, s, s], [-s, 0.0, 0.0, s]];
    let rot = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::Rotation))
        .expect("rotation channel");
    if let AnimationValues::Quat(v) = &rot.sampler.values {
        for (got, want) in v.iter().zip(original_rot.iter()) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() <= 1.0 / 32767.0,
                    "ishort rotation comp {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    } else {
        panic!("expected Quat rotation values");
    }

    let original_weights = [-1.0f32, -0.5, 0.5, 1.0];
    let weights = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::MorphWeights))
        .expect("morph channel");
    if let AnimationValues::Scalar(v) = &weights.sampler.values {
        for (got, want) in v.iter().zip(original_weights.iter()) {
            assert!(
                (got - want).abs() <= 1.0 / 32767.0,
                "ishort morph weight: got {got}, want {want}"
            );
        }
    } else {
        panic!("expected Scalar morph values");
    }
}

#[test]
fn quantize_ibyte_reserves_minus_128_slot() {
    // Spec §3.6.2.2 reserves -128 (the dequantised value would
    // exceed -1.0). Even an input of -1.0 must round to -127, and
    // any negative outlier must clamp to -127, never -128.
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    // r7: a `weights` animation channel requires the mesh to declare
    // at least one morph target (spec §3.11).
    prim.targets.push(MorphTarget {
        position: Some(vec![[0.0, 0.0, 0.0]; 3]),
        normal: None,
        tangent: None,
    });
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    let mut anim = Animation::new(Some("clamp".to_owned()));
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::MorphWeights,
        },
        sampler: AnimationSampler {
            // Out-of-range -2.0 must clamp to -127 / 32767, not the
            // reserved -128 / -32768 slot.
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Scalar(vec![-1.0, -2.0]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);

    let mut enc = GltfEncoder::new().with_quantize_animation(QuantizeMode::IByte);
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let weights = decoded
        .animations
        .iter()
        .flat_map(|a| a.channels.iter())
        .find(|c| matches!(c.target.property, AnimationProperty::MorphWeights))
        .expect("morph channel");
    if let AnimationValues::Scalar(v) = &weights.sampler.values {
        // Both values dequantise to exactly -1.0 (i8 -127 → -127/127 = -1.0).
        assert!(
            (v[0] + 1.0).abs() <= f32::EPSILON,
            "expected -1.0, got {}",
            v[0]
        );
        assert!(
            (v[1] + 1.0).abs() <= f32::EPSILON,
            "expected -1.0 (clamped), got {}",
            v[1]
        );
    } else {
        panic!("expected Scalar morph values");
    }
}

#[test]
fn quantize_does_not_touch_translation_or_scale() {
    // Even with QuantizeMode::UByte, TRANSLATION + SCALE outputs
    // remain FLOAT (the spec restricts them to FLOAT).
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    let mut anim = Animation::new(Some("ts".to_owned()));
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Translation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Vec3(vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]),
            interpolation: Interpolation::Linear,
        },
    });
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Scale,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Vec3(vec![[1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);

    let mut enc = GltfEncoder::new().with_quantize_animation(QuantizeMode::UByte);
    let glb = enc.encode(&scene).unwrap();
    // Decode should produce identical FLOAT vec3 outputs (no quantisation loss).
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    for ch in decoded.animations[0].channels.iter() {
        match (&ch.target.property, &ch.sampler.values) {
            (AnimationProperty::Translation, AnimationValues::Vec3(v)) => {
                assert_eq!(v[0], [1.0, 2.0, 3.0]);
                assert_eq!(v[1], [4.0, 5.0, 6.0]);
            }
            (AnimationProperty::Scale, AnimationValues::Vec3(v)) => {
                assert_eq!(v[0], [1.0, 1.0, 1.0]);
                assert_eq!(v[1], [2.0, 2.0, 2.0]);
            }
            _ => {}
        }
    }
}
