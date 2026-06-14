//! End-to-end texture-sampler validation per glTF 2.0 §5.26 (round
//! r306).
//!
//! Each test wires a malformed `samplers[]` entry through the public
//! `GltfDecoder` API and confirms the spec-prefixed `Error::InvalidData`
//! surfaces. The unit tests in `src/validation.rs` cover the per-rule
//! logic; the tests here pin the wiring inside `convert()` so a future
//! refactor can't accidentally drop the call.
//!
//! Allowed-value tables (§5.26.1–§5.26.4):
//!   magFilter: 9728 NEAREST, 9729 LINEAR
//!   minFilter: 9728, 9729, 9984, 9985, 9986, 9987
//!   wrapS/wrapT: 33071 CLAMP_TO_EDGE, 33648 MIRRORED_REPEAT, 10497 REPEAT

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Minimal valid document with one sampler object supplied as raw JSON.
/// A texture + an image keep the sampler reachable through a complete
/// material/texture stack, matching real assets.
fn doc_with_sampler(sampler_json: &str) -> Vec<u8> {
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "samplers": [ {sampler_json} ],
        "images": [ {{ "uri": "data:image/png;base64,iVBORw0KGgo=" }} ],
        "textures": [ {{ "source": 0, "sampler": 0 }} ],
        "scenes": [ {{ "nodes": [] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(sampler_json: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_sampler(sampler_json))
        .expect_err("sampler document should have been rejected");
    format!("{err}")
}

#[test]
fn valid_samplers_pass_through_the_decoder() {
    // Every enumerated filter / wrap value plus the all-absent case
    // (wrapS/wrapT default to REPEAT; filters are implementation choice).
    for s in [
        r#"{ }"#,
        r#"{ "magFilter": 9728, "minFilter": 9729 }"#,
        r#"{ "magFilter": 9729, "minFilter": 9984 }"#,
        r#"{ "minFilter": 9985, "wrapS": 33071, "wrapT": 33648 }"#,
        r#"{ "minFilter": 9986, "wrapS": 10497, "wrapT": 10497 }"#,
        r#"{ "minFilter": 9987, "wrapS": 33648, "wrapT": 33071 }"#,
    ] {
        let mut dec = GltfDecoder::new();
        dec.decode(&doc_with_sampler(s))
            .unwrap_or_else(|e| panic!("sampler {s} should be accepted: {e}"));
    }
}

#[test]
fn rejects_out_of_range_mag_filter() {
    // §5.26.1 — magFilter has only NEAREST / LINEAR; a mipmap value is
    // minFilter-only.
    let msg = decode_err(r#"{ "magFilter": 9987 }"#);
    assert!(msg.contains("SamplerMagFilter"), "got: {msg}");
}

#[test]
fn rejects_out_of_range_min_filter() {
    // §5.26.2
    let msg = decode_err(r#"{ "minFilter": 9999 }"#);
    assert!(msg.contains("SamplerMinFilter"), "got: {msg}");
}

#[test]
fn rejects_out_of_range_wrap_s() {
    // §5.26.3 — 0 is not a valid WebGL wrap enum.
    let msg = decode_err(r#"{ "wrapS": 0 }"#);
    assert!(msg.contains("SamplerWrapS"), "got: {msg}");
}

#[test]
fn rejects_out_of_range_wrap_t() {
    // §5.26.4 — off-by-one from CLAMP_TO_EDGE (33071).
    let msg = decode_err(r#"{ "wrapT": 33072 }"#);
    assert!(msg.contains("SamplerWrapT"), "got: {msg}");
}
