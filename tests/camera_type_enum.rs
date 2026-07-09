//! End-to-end validation of the glTF 2.0 §5.12.3 (camera.type) rule that
//! `type` MUST be one of "perspective" / "orthographic" and that the
//! matching projection property MUST be defined.
//!
//! Each test drives a document through the public `GltfDecoder` API and
//! asserts either a `Camera…`-prefixed `Error::InvalidData` or a clean
//! decode.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A minimal document with a single camera whose JSON body is supplied
/// verbatim (so a bad `type` / missing projection block can be injected).
fn doc_with_camera(camera_json: &str) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "cameras": [ {camera_json} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(camera_json: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_camera(camera_json))
        .expect_err("camera should have been rejected");
    format!("{err}")
}

fn decode_ok(camera_json: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_camera(camera_json))
        .unwrap_or_else(|e| panic!("camera {camera_json} should be accepted: {e}"));
}

#[test]
fn accepts_perspective() {
    decode_ok(r#"{ "type": "perspective", "perspective": { "yfov": 0.7, "znear": 0.1 } }"#);
}

#[test]
fn accepts_orthographic() {
    decode_ok(
        r#"{ "type": "orthographic", "orthographic": { "xmag": 1.0, "ymag": 1.0, "zfar": 10.0, "znear": 0.0 } }"#,
    );
}

#[test]
fn rejects_unknown_type() {
    let msg = decode_err(r#"{ "type": "fisheye", "perspective": { "yfov": 0.7, "znear": 0.1 } }"#);
    assert!(msg.contains("CameraType"), "got: {msg}");
}

#[test]
fn rejects_perspective_without_block() {
    let msg = decode_err(r#"{ "type": "perspective" }"#);
    assert!(msg.contains("CameraProjectionMissing"), "got: {msg}");
}

#[test]
fn rejects_orthographic_without_block() {
    let msg = decode_err(r#"{ "type": "orthographic" }"#);
    assert!(msg.contains("CameraProjectionMissing"), "got: {msg}");
}
