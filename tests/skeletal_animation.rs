//! Skeletal animation + skin round-trip per glTF 2.0 §3.7.3 + §3.11.
//!
//! Builds a tiny rigged scene (one mesh skinned to two joints, plus one
//! Animation with three channels — translation/rotation/scale — using
//! all three interpolation modes), encodes it to `.glb`, decodes back,
//! and checks the joint roster, inverse-bind matrices, and animation
//! sampler payloads survived intact.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Animation, AnimationChannel, AnimationProperty, AnimationSampler, AnimationTarget,
    AnimationValues, Interpolation, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, NodeId, Primitive,
    Scene3D, Skeleton, Skin, Topology, Transform,
};

/// Build a minimal rigged scene: one mesh node, two joint nodes, two
/// IBM matrices, three animation channels covering all paths +
/// interpolations.
fn rigged_scene() -> Scene3D {
    let mut scene = Scene3D::new();

    // Geometry: a triangle with JOINTS_0 + WEIGHTS_0 attributes (each
    // vertex fully bound to joint 0).
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    prim.joints = Some(vec![[0, 0, 0, 0]; 3]);
    prim.weights = Some(vec![[1.0, 0.0, 0.0, 0.0]; 3]);
    let mut mesh = Mesh::new(Some("rigged_tri".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);

    // Skeleton: two joints with non-identity IBMs to verify
    // column-major round-trip.
    let mut skel = Skeleton::new();
    skel.name = Some("test_skel".to_owned());
    // Joint nodes will be 1, 2 (mesh node is 0). We add the nodes
    // below — IDs are stable because Scene3D::add_node hands out
    // sequential ids.
    skel.joints = vec![NodeId(1), NodeId(2)];
    // IBM 0 = translate +1 on X; IBM 1 = translate -2 on Y. Stored
    // in our row-major form; the encoder transposes to column-major
    // on the wire.
    skel.inverse_bind_matrices = vec![
        [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ];
    let skel_id = scene.add_skeleton(skel);
    let skin = Skin::new(skel_id).with_root(NodeId(1));
    let skin_id = scene.add_skin(skin);

    // Mesh node + skin reference (mesh node is `node 0`, joints become 1/2).
    let mut mesh_node = Node::new();
    mesh_node.name = Some("mesh_node".to_owned());
    mesh_node.mesh = Some(mid);
    mesh_node.skin = Some(skin_id);
    let mn = scene.add_node(mesh_node);
    scene.add_root(mn);

    // Joint node 1 — root joint.
    let mut j0 = Node::new();
    j0.name = Some("joint_root".to_owned());
    j0.transform = Transform::Trs {
        translation: [0.0, 1.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };
    j0.children = vec![NodeId(2)];
    scene.add_node(j0);

    // Joint node 2 — child joint.
    let mut j1 = Node::new();
    j1.name = Some("joint_tip".to_owned());
    j1.transform = Transform::Trs {
        translation: [0.0, 0.5, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };
    scene.add_node(j1);
    scene.add_root(NodeId(1)); // skeleton root is also a scene root

    // Animation: translation (LINEAR), rotation (STEP), scale (CUBICSPLINE).
    let mut anim = Animation::new(Some("walk_cycle".to_owned()));
    // Translation channel — drive joint root by [0,2,0] then [0,2.5,0].
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: NodeId(1),
            property: AnimationProperty::Translation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Vec3(vec![[0.0, 2.0, 0.0], [0.0, 2.5, 0.0]]),
            interpolation: Interpolation::Linear,
        },
    });
    // Rotation channel — STEP from identity to 90deg-Z.
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: NodeId(2),
            property: AnimationProperty::Rotation,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 0.5, 1.0],
            values: AnimationValues::Quat(vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.707_106_77, 0.707_106_77],
                [0.0, 0.0, 0.0, 1.0],
            ]),
            interpolation: Interpolation::Step,
        },
    });
    // Scale channel — CUBICSPLINE: 3 values per keyframe (in_tangent, value, out_tangent).
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: NodeId(2),
            property: AnimationProperty::Scale,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0],
            // 2 keyframes × 3 vec3 each = 6 entries.
            values: AnimationValues::Vec3(vec![
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 1.0],
                [0.5, 0.5, 0.5],
                [0.0, 0.0, 0.0],
                [2.0, 2.0, 2.0],
                [0.0, 0.0, 0.0],
            ]),
            interpolation: Interpolation::CubicSpline,
        },
    });
    scene.add_animation(anim);

    scene
}

#[test]
fn skin_and_animation_glb_roundtrip() {
    let scene = rigged_scene();
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();

    // Skin: one skeleton, one skin.
    assert_eq!(decoded.skeletons.len(), 1, "skeleton count");
    assert_eq!(decoded.skins.len(), 1, "skin count");
    let skel = &decoded.skeletons[0];
    assert_eq!(skel.joints, vec![NodeId(1), NodeId(2)], "joint roster");
    assert_eq!(skel.inverse_bind_matrices.len(), 2);
    // IBM 0 — [0][3] = 1.0 (X translation).
    assert!((skel.inverse_bind_matrices[0][0][3] - 1.0).abs() < 1e-5);
    assert!((skel.inverse_bind_matrices[0][1][3]).abs() < 1e-5);
    // IBM 1 — [1][3] = -2.0 (Y translation).
    assert!((skel.inverse_bind_matrices[1][1][3] - -2.0).abs() < 1e-5);
    assert!((skel.inverse_bind_matrices[1][0][3]).abs() < 1e-5);
    assert_eq!(decoded.skins[0].root_node, Some(NodeId(1)));

    // Mesh node (0) carries the skin reference.
    let mn = decoded.node(NodeId(0)).expect("mesh node missing");
    assert_eq!(mn.skin.map(|s| s.0), Some(0));

    // Animation: one Animation, three channels with the three
    // interpolations and matching property paths.
    assert_eq!(decoded.animations.len(), 1);
    let anim = &decoded.animations[0];
    assert_eq!(anim.name.as_deref(), Some("walk_cycle"));
    assert_eq!(anim.channels.len(), 3);

    // Channel 0 — translation, LINEAR, two keyframes.
    let c0 = &anim.channels[0];
    assert_eq!(c0.target.property, AnimationProperty::Translation);
    assert_eq!(c0.target.node, NodeId(1));
    assert_eq!(c0.sampler.interpolation, Interpolation::Linear);
    assert_eq!(c0.sampler.keyframes, vec![0.0, 1.0]);
    match &c0.sampler.values {
        AnimationValues::Vec3(v) => {
            assert_eq!(v.len(), 2);
            assert!((v[1][1] - 2.5).abs() < 1e-5);
        }
        other => panic!("expected Vec3, got {other:?}"),
    }
    // Channel 1 — rotation, STEP, three keyframes.
    let c1 = &anim.channels[1];
    assert_eq!(c1.target.property, AnimationProperty::Rotation);
    assert_eq!(c1.sampler.interpolation, Interpolation::Step);
    assert_eq!(c1.sampler.keyframes.len(), 3);
    match &c1.sampler.values {
        AnimationValues::Quat(v) => {
            assert_eq!(v.len(), 3);
            assert!((v[1][2] - 0.707_106_77).abs() < 1e-5);
        }
        other => panic!("expected Quat, got {other:?}"),
    }
    // Channel 2 — scale, CUBICSPLINE — 2 keyframes × 3 vec3 = 6 values.
    let c2 = &anim.channels[2];
    assert_eq!(c2.sampler.interpolation, Interpolation::CubicSpline);
    assert_eq!(c2.sampler.keyframes.len(), 2);
    match &c2.sampler.values {
        AnimationValues::Vec3(v) => {
            assert_eq!(v.len(), 6, "CUBICSPLINE 2 keyframes => 6 values");
            assert!((v[1][0] - 1.0).abs() < 1e-5);
            assert!((v[4][0] - 2.0).abs() < 1e-5);
        }
        other => panic!("expected Vec3, got {other:?}"),
    }
}

#[test]
fn morph_weights_channel_roundtrip() {
    // Targeting `weights` — output is SCALAR, not a vector.
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(Some("morphy".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);

    let mut anim = Animation::new(Some("morph".to_owned()));
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n,
            property: AnimationProperty::MorphWeights,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 0.5, 1.0],
            // 3 keyframes × 2 morph targets = 6 weights end-to-end.
            values: AnimationValues::Scalar(vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.add_animation(anim);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    let ch = &decoded.animations[0].channels[0];
    assert_eq!(ch.target.property, AnimationProperty::MorphWeights);
    match &ch.sampler.values {
        AnimationValues::Scalar(v) => {
            assert_eq!(v.len(), 6);
            assert!((v[5] - 1.0).abs() < 1e-5);
        }
        other => panic!("expected Scalar, got {other:?}"),
    }
}
