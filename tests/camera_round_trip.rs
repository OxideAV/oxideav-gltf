//! Both perspective and orthographic cameras survive a `.glb` round
//! trip, including the optional `aspectRatio` / infinite-`zfar` flavour.

use std::f32::consts::{FRAC_PI_3, FRAC_PI_4};

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Camera, Mesh3DDecoder, Mesh3DEncoder, Node, Scene3D};

#[test]
fn perspective_and_orthographic_roundtrip() {
    let mut scene = Scene3D::new();
    let pcam = scene.add_camera(Camera::Perspective {
        aspect_ratio: Some(16.0 / 9.0),
        yfov: FRAC_PI_3, // 60 deg
        znear: 0.1,
        zfar: Some(1000.0),
    });
    let infcam = scene.add_camera(Camera::perspective(FRAC_PI_4, 0.05)); // infinite zfar
    let ocam = scene.add_camera(Camera::Orthographic {
        xmag: 5.0,
        ymag: 3.0,
        znear: 0.1,
        zfar: 50.0,
    });

    let cam_ids = [pcam, infcam, ocam];
    for c in cam_ids {
        let mut node = Node::new();
        node.camera = Some(c);
        let n = scene.add_node(node);
        scene.add_root(n);
    }

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();

    assert_eq!(decoded.cameras.len(), 3);
    match decoded.cameras[0] {
        Camera::Perspective {
            aspect_ratio,
            yfov,
            znear,
            zfar,
        } => {
            assert!((aspect_ratio.unwrap() - 16.0 / 9.0).abs() < 1e-5);
            assert!((yfov - FRAC_PI_3).abs() < 1e-5);
            assert!((znear - 0.1).abs() < 1e-5);
            assert_eq!(zfar, Some(1000.0));
        }
        other => panic!("expected perspective, got {other:?}"),
    }
    match decoded.cameras[1] {
        Camera::Perspective {
            aspect_ratio, zfar, ..
        } => {
            assert_eq!(aspect_ratio, None);
            assert_eq!(zfar, None);
        }
        other => panic!("expected perspective with no aspect/zfar, got {other:?}"),
    }
    match decoded.cameras[2] {
        Camera::Orthographic {
            xmag,
            ymag,
            znear,
            zfar,
        } => {
            assert!((xmag - 5.0).abs() < 1e-5);
            assert!((ymag - 3.0).abs() < 1e-5);
            assert!((znear - 0.1).abs() < 1e-5);
            assert!((zfar - 50.0).abs() < 1e-5);
        }
        other => panic!("expected orthographic, got {other:?}"),
    }
}
