//! End-to-end validation of the glTF 2.0 §3.7.2.1 rule that
//! application-specific (`_`-prefixed) attribute semantics MUST NOT use
//! the `UNSIGNED_INT` (5125) component type.
//!
//! Each test drives a document through the public `GltfDecoder` API and
//! asserts the `AttributeUnsignedIntComponent`-prefixed
//! `Error::InvalidData` surfaces (or that a conformant sibling document
//! decodes cleanly).

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// A minimal single-primitive document. `attributes_json` supplies the
/// primitive's `attributes` map; `extra_accessors` supplies any
/// accessors beyond the always-present POSITION accessor (accessor 0).
/// The 64-byte buffer + bufferView keep every layout in range so the
/// earlier fit/alignment passes succeed and the §3.7.2.1 pass is the one
/// that fires.
fn doc(attributes_json: &str, extra_accessors: &str) -> Vec<u8> {
    let b64 = "data:application/octet-stream;base64,\
        AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "uri": "{b64}", "byteLength": 64 }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 64 }} ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
               "min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0] }}
            {extra_accessors}
        ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {attributes_json}, "mode": 0 }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

fn decode_err(attributes_json: &str, extra_accessors: &str) -> String {
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc(attributes_json, extra_accessors))
        .expect_err("document should have been rejected");
    format!("{err}")
}

fn decode_ok(attributes_json: &str, extra_accessors: &str) {
    let mut dec = GltfDecoder::new();
    dec.decode(&doc(attributes_json, extra_accessors))
        .unwrap_or_else(|e| panic!("document should be accepted: {e}"));
}

#[test]
fn rejects_application_specific_semantic_with_unsigned_int() {
    // `_ID` is application-specific (leading underscore); its accessor is
    // UNSIGNED_INT (5125) — a §3.7.2.1 MUST NOT.
    let msg = decode_err(
        r#"{ "POSITION": 0, "_ID": 1 }"#,
        r#", { "bufferView": 0, "componentType": 5125, "count": 1, "type": "SCALAR" }"#,
    );
    assert!(msg.contains("AttributeUnsignedIntComponent"), "got: {msg}");
}

#[test]
fn accepts_application_specific_semantic_with_float() {
    // Same `_ID` semantic but stored as FLOAT — the extra forms allowed
    // for application-specific data. No §3.7.2.1 violation.
    decode_ok(
        r#"{ "POSITION": 0, "_ID": 1 }"#,
        r#", { "bufferView": 0, "componentType": 5126, "count": 1, "type": "SCALAR" }"#,
    );
}

#[test]
fn accepts_application_specific_semantic_with_unsigned_short() {
    // UNSIGNED_SHORT is a valid integer component type for an
    // application-specific semantic — only UNSIGNED_INT is barred.
    decode_ok(
        r#"{ "POSITION": 0, "_BATCHID": 1 }"#,
        r#", { "bufferView": 0, "componentType": 5123, "count": 1, "type": "SCALAR" }"#,
    );
}

#[test]
fn rejects_unsigned_int_on_morph_target_application_semantic() {
    // The rule also walks each morph target's attributes. Here the base
    // primitive is clean and the offending `_ID` UINT sits on a morph
    // target. A morphed POSITION with min/max keeps the morph-target
    // structural pass happy so §3.7.2.1 is the pass that fires.
    let b64 = "data:application/octet-stream;base64,\
        AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "uri": "{b64}", "byteLength": 64 }} ],
        "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 64 }} ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
               "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0] }},
            {{ "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
               "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0] }},
            {{ "bufferView": 0, "componentType": 5125, "count": 1, "type": "SCALAR" }}
        ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {{ "POSITION": 0 }},
            "targets": [ {{ "POSITION": 1, "_ID": 2 }} ]
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    );
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(json.as_bytes())
        .expect_err("morph-target document should have been rejected");
    let msg = format!("{err}");
    assert!(msg.contains("AttributeUnsignedIntComponent"), "got: {msg}");
}
