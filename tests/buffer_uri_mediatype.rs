//! End-to-end validation of the glTF 2.0 §3.9.1 (Buffers) rule that a
//! `data:` URI buffer's mediatype MUST be `application/octet-stream` or
//! `application/gltf-buffer`.
//!
//! Each test drives a document through the public `GltfDecoder` API and
//! asserts either the `BufferUriMediaType`-prefixed `Error::InvalidData`
//! or a clean decode.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A minimal document whose single buffer's `uri` is supplied verbatim.
/// The 64-byte payload keeps the accessor + bufferView in range so the
/// mediatype rule is the pass under test.
fn doc_with_buffer_uri(uri: &str) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "uri": "{uri}", "byteLength": 64 }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 64 }} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

// 64 zero bytes, base64-encoded.
const B64_64: &str =
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn decode_err(uri: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_buffer_uri(uri))
        .expect_err("buffer document should have been rejected");
    format!("{err}")
}

fn decode_ok(uri: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc_with_buffer_uri(uri))
        .unwrap_or_else(|e| panic!("buffer uri {uri} should be accepted: {e}"));
}

#[test]
fn accepts_octet_stream_mediatype() {
    decode_ok(&format!("data:application/octet-stream;base64,{B64_64}"));
}

#[test]
fn accepts_gltf_buffer_mediatype() {
    decode_ok(&format!("data:application/gltf-buffer;base64,{B64_64}"));
}

#[test]
fn accepts_gltf_buffer_mediatype_with_extra_params() {
    // A `;charset=...` attribute before `;base64` MUST NOT change the
    // mediatype token itself.
    decode_ok(&format!(
        "data:application/gltf-buffer;charset=utf-8;base64,{B64_64}"
    ));
}

#[test]
fn rejects_image_png_mediatype() {
    // §3.9.1 — an image mediatype (e.g. a copy-paste from an image data
    // URI) is not a legal buffer mediatype.
    let msg = decode_err(&format!("data:image/png;base64,{B64_64}"));
    assert!(msg.contains("BufferUriMediaType"), "got: {msg}");
}

#[test]
fn rejects_empty_mediatype() {
    // `data:;base64,...` (or `data:,...`) carries no mediatype — not one
    // of the two spec-allowed strings.
    let msg = decode_err(&format!("data:;base64,{B64_64}"));
    assert!(msg.contains("BufferUriMediaType"), "got: {msg}");
}
