//! `GltfEncoder::with_sparse_threshold` heuristic — animation-output
//! accessors whose zero-element fraction meets the threshold are
//! re-emitted using `accessor.sparse` storage per glTF 2.0 §3.6.2.3.
//!
//! r2 always emitted dense; r3 makes sparse opt-in via the encoder
//! builder. Round-trip parity (sparse-encode -> decode -> dense-encode
//! -> decode) yields bit-equal animation values both ways.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Animation, AnimationChannel, AnimationProperty, AnimationSampler, AnimationTarget,
    AnimationValues, Interpolation, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D,
    Topology,
};

/// Build a scene with a morph-weights animation that's mostly zero —
/// 8 keyframes of 4 morph targets, only 2 of the 32 weight slots are
/// non-zero. Sparse encoding should kick in at any threshold <= 30/32.
fn mostly_zero_morph_scene() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("morph_target_mesh".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);

    let mut anim = Animation::new(Some("morph".to_owned()));
    // 8 keyframes × 4 morph targets = 32 entries.
    let mut weights = vec![0.0f32; 32];
    weights[5] = 0.75;
    weights[20] = 0.5;
    let keyframes = (0..8).map(|i| i as f32 * 0.1).collect();
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::MorphWeights,
        },
        sampler: AnimationSampler {
            keyframes,
            values: AnimationValues::Scalar(weights),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);
    scene
}

#[test]
fn dense_default_no_sparse_block() {
    // Default encoder (no threshold) should emit the morph weights
    // accessor without a `sparse` block.
    let scene = mostly_zero_morph_scene();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();

    // Pull JSON chunk back out so we can assert on its shape.
    assert_eq!(&glb[0..4], b"glTF");
    let json_chunk = extract_json_chunk(&glb);
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    let any_sparse = accessors.iter().any(|a| a.get("sparse").is_some());
    assert!(!any_sparse, "default encoder should not emit sparse blocks");
}

#[test]
fn sparse_threshold_emits_sparse_for_morph_weights() {
    let scene = mostly_zero_morph_scene();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();

    let json_chunk = extract_json_chunk(&glb);
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let accessors = v["accessors"].as_array().unwrap();

    // Find the SCALAR FLOAT output accessor — it should now carry
    // a `sparse` block with count == 2 (the two non-zero weights).
    let mut found_sparse = false;
    for acc in accessors {
        if acc["type"] == "SCALAR" && acc["componentType"] == 5126 {
            if let Some(s) = acc.get("sparse") {
                assert_eq!(
                    s["count"], 2,
                    "expected exactly 2 sparse override slots, got {s}"
                );
                // sparse-base accessor should have NO bufferView.
                assert!(
                    acc.get("bufferView").is_none(),
                    "sparse + zero base should drop bufferView"
                );
                found_sparse = true;
            }
        }
    }
    assert!(
        found_sparse,
        "expected a SCALAR FLOAT accessor with sparse storage"
    );

    // Now verify round-trip: decode -> re-encode dense -> decode again.
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let original = mostly_zero_morph_scene();
    assert_animation_eq(&decoded.animations[0], &original.animations[0]);

    let mut enc_dense = GltfEncoder::new();
    let glb2 = enc_dense.encode(&decoded).unwrap();
    let decoded2 = dec.decode(&glb2).unwrap();
    assert_animation_eq(&decoded2.animations[0], &original.animations[0]);
}

#[test]
fn sparse_threshold_high_keeps_dense() {
    // Mostly zero (~ 30/32 = 0.9375 zero fraction). A 0.99 threshold
    // should still keep it dense.
    let scene = mostly_zero_morph_scene();
    let mut enc = GltfEncoder::new().with_sparse_threshold(0.99);
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = extract_json_chunk(&glb);
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    let any_sparse = accessors.iter().any(|a| a.get("sparse").is_some());
    assert!(
        !any_sparse,
        "0.99 threshold should keep this 0.9375-zero accessor dense"
    );
}

#[test]
fn sparse_translation_vec3_round_trip() {
    // Translation channel with mostly-zero positions. Channel 0 of 3
    // keyframes; only the middle one is non-zero translation.
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("anim".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    let mut anim = Animation::new(Some("translate".to_owned()));
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Translation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 0.5, 1.0],
            values: AnimationValues::Vec3(vec![[0.0, 0.0, 0.0], [3.0, 4.0, 5.0], [0.0, 0.0, 0.0]]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);

    let mut enc = GltfEncoder::new().with_sparse_threshold(0.5);
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = extract_json_chunk(&glb);
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    // The translation output accessor (VEC3 FLOAT) should be sparse.
    let translation_sparse = accessors
        .iter()
        .any(|a| a["type"] == "VEC3" && a["componentType"] == 5126 && a.get("sparse").is_some());
    assert!(
        translation_sparse,
        "translation Vec3 with 2/3 zero entries should sparse-encode at 0.5 threshold"
    );

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let ch = &decoded.animations[0].channels[0];
    match &ch.sampler.values {
        AnimationValues::Vec3(v) => {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0], [0.0, 0.0, 0.0]);
            assert_eq!(v[1], [3.0, 4.0, 5.0]);
            assert_eq!(v[2], [0.0, 0.0, 0.0]);
        }
        other => panic!("expected Vec3, got {other:?}"),
    }
}

#[test]
fn sparse_skipped_for_rotation_and_scale() {
    // Rotation identity is `[0,0,0,1]`, scale identity is `[1,1,1]` —
    // a zero-base sparse accessor would mis-represent the implicit
    // values. The encoder must keep these dense regardless of
    // threshold.
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);
    let mut anim = Animation::new(Some("rs".to_owned()));
    // Rotation: 4 identity quaternions plus one twist. xyz are zero on
    // 4/5 keyframes — high zero fraction *for xyz* but the w slot is
    // 1.0 always so per-element "all components zero" is FALSE.
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Rotation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 0.25, 0.5, 0.75, 1.0],
            values: AnimationValues::Quat(vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.707, 0.707],
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            ]),
            interpolation: Interpolation::Linear,
        },
    });
    // Scale: 5 identity scales — all `[1,1,1]`, no zeros.
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::Scale,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 0.25, 0.5, 0.75, 1.0],
            values: AnimationValues::Vec3(vec![[1.0; 3]; 5]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);

    let mut enc = GltfEncoder::new().with_sparse_threshold(0.0);
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = extract_json_chunk(&glb);
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let accessors = v["accessors"].as_array().unwrap();
    // Animation sampler outputs carry the encoder-assigned name
    // "output" (see push_*_accessor helpers). Mesh attribute
    // accessors get names like "POSITION" and are out of scope for
    // this test (they may be sparse in their own right since r5).
    for acc in accessors {
        if acc.get("name").and_then(|n| n.as_str()) != Some("output") {
            continue;
        }
        if acc.get("sparse").is_some() {
            panic!("rotation/scale animation accessors must stay dense, got sparse on {acc}");
        }
    }
}

// --- helpers --------------------------------------------------------------

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    // Header is 12 bytes; chunk 0 is JSON.
    assert!(glb.len() >= 20);
    let chunk0_len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    let chunk0_kind = &glb[16..20];
    assert_eq!(chunk0_kind, b"JSON");
    glb[20..20 + chunk0_len].to_vec()
}

fn assert_animation_eq(a: &Animation, b: &Animation) {
    assert_eq!(a.channels.len(), b.channels.len(), "channel count");
    for (ca, cb) in a.channels.iter().zip(b.channels.iter()) {
        assert_eq!(ca.target.property, cb.target.property);
        assert_eq!(ca.sampler.keyframes, cb.sampler.keyframes);
        match (&ca.sampler.values, &cb.sampler.values) {
            (AnimationValues::Scalar(va), AnimationValues::Scalar(vb)) => {
                assert_eq!(va, vb)
            }
            (AnimationValues::Vec3(va), AnimationValues::Vec3(vb)) => {
                assert_eq!(va, vb)
            }
            (AnimationValues::Quat(va), AnimationValues::Quat(vb)) => {
                assert_eq!(va, vb)
            }
            (a, b) => panic!("variant mismatch: {a:?} vs {b:?}"),
        }
    }
}
