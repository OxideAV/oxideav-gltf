//! End-to-end camera property validation per glTF 2.0 §5.12 + §5.13 +
//! §5.14 (round r277).
//!
//! Each test wires a malformed `cameras[]` entry through the public
//! `GltfDecoder` API and confirms the spec-prefixed
//! `Error::InvalidData` surfaces. The unit tests in `src/validation.rs`
//! cover the per-rule logic; the tests here pin the wiring inside
//! `convert()` so a future refactor can't accidentally drop the call.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Minimal valid document with one camera object supplied as raw JSON.
fn doc_with_camera(camera_json: &str) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "cameras": [ {camera_json} ],
        "nodes": [ {{ "camera": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(camera_json: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_camera(camera_json))
        .expect_err("camera document should have been rejected");
    format!("{err}")
}

#[test]
fn valid_cameras_pass_through_the_decoder() {
    // znear == 0 is valid on an orthographic camera (§5.13.4 schema
    // minimum is >= 0); negative magnification is SHOULD NOT (allowed);
    // a missing perspective zfar means an infinite projection.
    for cam in [
        r#"{ "type": "perspective", "perspective": { "yfov": 0.7, "znear": 0.01 } }"#,
        r#"{ "type": "perspective",
             "perspective": { "aspectRatio": 1.5, "yfov": 0.7, "znear": 0.1, "zfar": 256.0 } }"#,
        r#"{ "type": "orthographic",
             "orthographic": { "xmag": 2.0, "ymag": 2.0, "znear": 0.0, "zfar": 100.0 } }"#,
        r#"{ "type": "orthographic",
             "orthographic": { "xmag": -2.0, "ymag": -2.0, "znear": 0.5, "zfar": 100.0 } }"#,
    ] {
        let mut dec = GltfDecoder::new();
        let scene = dec.decode(&doc_with_camera(cam)).unwrap();
        assert_eq!(scene.cameras.len(), 1);
    }
}

#[test]
fn rejects_camera_with_both_projection_blocks() {
    // §5.12 — perspective MUST NOT be defined when orthographic is.
    let msg = decode_err(
        r#"{ "type": "perspective",
             "perspective": { "yfov": 0.7, "znear": 0.01 },
             "orthographic": { "xmag": 1.0, "ymag": 1.0, "znear": 0.0, "zfar": 10.0 } }"#,
    );
    assert!(msg.contains("CameraProjectionExclusive"), "{msg}");
}

#[test]
fn rejects_orthographic_zero_xmag() {
    // §5.13.1 — xmag MUST NOT be equal to zero.
    let msg = decode_err(
        r#"{ "type": "orthographic",
             "orthographic": { "xmag": 0.0, "ymag": 1.0, "znear": 0.0, "zfar": 10.0 } }"#,
    );
    assert!(msg.contains("CameraOrthographicXmag"), "{msg}");
}

#[test]
fn rejects_orthographic_zfar_not_greater_than_znear() {
    // §5.13.3 — zfar MUST be greater than znear.
    let msg = decode_err(
        r#"{ "type": "orthographic",
             "orthographic": { "xmag": 1.0, "ymag": 1.0, "znear": 10.0, "zfar": 10.0 } }"#,
    );
    assert!(msg.contains("CameraOrthographicZRange"), "{msg}");
}

#[test]
fn rejects_orthographic_negative_znear() {
    // §5.13.4 — schema minimum for orthographic znear is >= 0.
    let msg = decode_err(
        r#"{ "type": "orthographic",
             "orthographic": { "xmag": 1.0, "ymag": 1.0, "znear": -1.0, "zfar": 10.0 } }"#,
    );
    assert!(msg.contains("CameraOrthographicZnear"), "{msg}");
}

#[test]
fn rejects_perspective_zero_yfov() {
    // §5.14.2 — yfov minimum is > 0.
    let msg =
        decode_err(r#"{ "type": "perspective", "perspective": { "yfov": 0.0, "znear": 0.01 } }"#);
    assert!(msg.contains("CameraPerspectiveYfov"), "{msg}");
}

#[test]
fn rejects_perspective_zero_znear() {
    // §5.14.4 — perspective znear minimum is > 0 (stricter than the
    // orthographic camera's >= 0).
    let msg =
        decode_err(r#"{ "type": "perspective", "perspective": { "yfov": 0.7, "znear": 0.0 } }"#);
    assert!(msg.contains("CameraPerspectiveZnear"), "{msg}");
}

#[test]
fn rejects_perspective_zero_aspect_ratio() {
    // §5.14.1 — aspectRatio, when defined, minimum is > 0.
    let msg = decode_err(
        r#"{ "type": "perspective",
             "perspective": { "aspectRatio": 0.0, "yfov": 0.7, "znear": 0.01 } }"#,
    );
    assert!(msg.contains("CameraPerspectiveAspectRatio"), "{msg}");
}

#[test]
fn rejects_perspective_zfar_not_greater_than_znear() {
    // §5.14.3 — when defined, zfar MUST be greater than znear.
    let msg = decode_err(
        r#"{ "type": "perspective",
             "perspective": { "yfov": 0.7, "znear": 5.0, "zfar": 5.0 } }"#,
    );
    assert!(msg.contains("CameraPerspectiveZRange"), "{msg}");
}

#[test]
fn camera_validation_runs_even_when_camera_is_unreferenced() {
    // The validator walks the whole cameras[] array, not just the
    // entries reachable from the scene graph.
    let json = br#"{
        "asset": { "version": "2.0" },
        "cameras": [ { "type": "perspective", "perspective": { "yfov": 0.7, "znear": 0.0 } } ],
        "scenes": [ { "nodes": [] } ], "scene": 0
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(json)
        .expect_err("unreferenced bad camera must still be rejected");
    assert!(format!("{err}").contains("CameraPerspectiveZnear"));
}
