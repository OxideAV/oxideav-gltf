//! KHR_lights_punctual extension — directional, point, and spot
//! lights survive a `.glb` round trip via the root `extensions`
//! block + per-node `extensions.KHR_lights_punctual.light` reference.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Light, Mesh3DDecoder, Mesh3DEncoder, Node, Scene3D};

#[test]
fn three_punctual_lights_roundtrip() {
    let mut scene = Scene3D::new();
    let dir = scene.add_light(Light::Directional {
        color: [1.0, 0.9, 0.8],
        intensity: 5.0,
    });
    let point = scene.add_light(Light::Point {
        color: [0.5, 0.7, 1.0],
        intensity: 50.0,
        range: Some(20.0),
    });
    let spot = scene.add_light(Light::Spot {
        color: [1.0; 3],
        intensity: 100.0,
        range: None,
        inner_cone_angle: 0.2,
        outer_cone_angle: 0.5,
    });
    let lights = [dir, point, spot];
    for l in lights {
        let mut node = Node::new();
        node.light = Some(l);
        let n = scene.add_node(node);
        scene.add_root(n);
    }

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();

    assert_eq!(decoded.lights.len(), 3);
    match decoded.lights[0] {
        Light::Directional { color, intensity } => {
            assert_eq!(color, [1.0, 0.9, 0.8]);
            assert_eq!(intensity, 5.0);
        }
        other => panic!("expected directional, got {other:?}"),
    }
    match decoded.lights[1] {
        Light::Point {
            range, intensity, ..
        } => {
            assert_eq!(range, Some(20.0));
            assert_eq!(intensity, 50.0);
        }
        other => panic!("expected point, got {other:?}"),
    }
    match decoded.lights[2] {
        Light::Spot {
            inner_cone_angle,
            outer_cone_angle,
            ..
        } => {
            assert!((inner_cone_angle - 0.2).abs() < 1e-5);
            assert!((outer_cone_angle - 0.5).abs() < 1e-5);
        }
        other => panic!("expected spot, got {other:?}"),
    }

    // Per-node light reference survived.
    for (i, n) in decoded.nodes.iter().enumerate() {
        assert_eq!(n.light.map(|l| l.0), Some(i as u32), "node {i}");
    }
}
