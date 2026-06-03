//! End-to-end encode validation for `KHR_mesh_quantization` per
//! `docs/3d/gltf/extensions/KHR_mesh_quantization.md` §Encoding
//! Quantized Data.
//!
//! The decode side stashes each attribute's storage form
//! (`componentType` + `normalized`) under the per-primitive
//! `__attr_quant` extras sentinel. The encoder reads the sentinel
//! back and re-emits each attribute with the original integer
//! width + normalisation flag, padding to the spec-mandated 4-byte
//! element stride.
//!
//! The encoder MUST also declare `KHR_mesh_quantization` in BOTH
//! `extensionsUsed` AND `extensionsRequired` per the extension's
//! §Overview ("files that use the extension must specify it in
//! extensionsRequired array - the extension is not optional").

use base64::Engine as _;
use oxideav_gltf::{json_encoder, GltfDecoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};

fn build_doc(
    bin: &[u8],
    accessors_json: &str,
    buffer_views_json: &str,
    attributes_json: &str,
    extensions_used_json: &str,
) -> Vec<u8> {
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(bin);
    let ext_used = if extensions_used_json.is_empty() {
        String::new()
    } else {
        format!(r#""extensionsUsed": [ {extensions_used_json} ],"#)
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        {ext_used}
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [ {buffer_views_json} ],
        "accessors": [ {accessors_json} ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {attributes_json}
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

/// Build a tiny SHORT-normalized POSITION VEC3 document, decode it,
/// re-encode it as JSON, and assert that the encoded form declares
/// `KHR_mesh_quantization` in both `extensionsUsed` AND
/// `extensionsRequired`, and that the POSITION accessor carries the
/// SHORT/normalized=true storage form (i.e. the encoder re-quantised
/// rather than promoting to FLOAT).
#[test]
fn round_trip_short_normalized_position_declares_extension() {
    let verts: [[i16; 3]; 4] = [
        [32767, 0, -32767],
        [-32767, 16384, 0],
        [0, 0, 0],
        [16383, -16384, 100],
    ];
    let mut bin = Vec::new();
    for v in verts {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]); // stride 8
    }
    let total = bin.len();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5122, "count": 4, "type": "VEC3", "normalized": true,
                "min": [-32767, -16384, -32767], "max": [32767, 16384, 100]}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}, "byteStride": 8}}"#),
        r#"{"POSITION": 0}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode quantized POSITION");

    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode quantized scene");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse encoded JSON");

    // extensionsUsed AND extensionsRequired MUST both list the extension.
    let used = json["extensionsUsed"]
        .as_array()
        .expect("extensionsUsed array");
    let required = json["extensionsRequired"]
        .as_array()
        .expect("extensionsRequired array — KHR_mesh_quantization is not optional");
    assert!(
        used.iter().any(|v| v == "KHR_mesh_quantization"),
        "extensionsUsed lacks KHR_mesh_quantization: {used:?}"
    );
    assert!(
        required.iter().any(|v| v == "KHR_mesh_quantization"),
        "extensionsRequired lacks KHR_mesh_quantization: {required:?}"
    );

    // The POSITION accessor MUST be SHORT/normalized=true (the
    // round-tripped storage form), NOT FLOAT.
    let acc = &json["accessors"][0];
    assert_eq!(acc["componentType"].as_u64(), Some(5122));
    assert_eq!(acc["normalized"].as_bool(), Some(true));
    assert_eq!(acc["type"].as_str(), Some("VEC3"));
    assert_eq!(acc["count"].as_u64(), Some(4));

    // Spec §Extending Mesh Attributes Implementation Note: for
    // quantized data, min/max also carry quantised integer values.
    // 16383 / 32767 dequantises back to ~0.5 — verify min/max are
    // integer-valued.
    let min = acc["min"].as_array().expect("min present");
    let max = acc["max"].as_array().expect("max present");
    assert_eq!(min.len(), 3);
    assert_eq!(max.len(), 3);
    for c in min.iter().chain(max.iter()) {
        let f = c.as_f64().expect("numeric");
        assert!(
            f.fract() == 0.0,
            "quantised bound must be integer-valued, got {f}"
        );
        assert!((-32768.0..=32767.0).contains(&f), "out of i16 range: {f}");
    }
}

/// Re-decoding the encoder's output reproduces the original f32
/// positions to within the spec dequantisation precision
/// (`1.0 / 32767.0` for SHORT-normalized data).
#[test]
fn round_trip_short_normalized_position_values_match() {
    let verts: [[i16; 3]; 3] = [[32767, 0, -32767], [0, 0, 0], [16383, -16384, 1000]];
    let mut bin = Vec::new();
    for v in verts {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]);
    }
    let total = bin.len();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5122, "count": 3, "type": "VEC3", "normalized": true,
                "min": [0, -16384, -32767], "max": [32767, 0, 1000]}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}, "byteStride": 8}}"#),
        r#"{"POSITION": 0}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene1 = dec.decode(&doc).expect("decode quantized");
    let pos1 = scene1.meshes[0].primitives[0].positions.clone();

    let mut enc = json_encoder();
    let bytes = enc.encode(&scene1).expect("encode");
    let mut dec2 = GltfDecoder::new();
    let scene2 = dec2.decode(&bytes).expect("decode round-trip");
    let pos2 = &scene2.meshes[0].primitives[0].positions;

    assert_eq!(pos1.len(), pos2.len());
    // SHORT-normalized precision is `1.0 / 32767.0` ≈ 3.05e-5; allow
    // 2× that to absorb f32 rounding through the encode equation.
    let tol = 2.0 / 32767.0;
    for (a, b) in pos1.iter().zip(pos2.iter()) {
        for c in 0..3 {
            assert!(
                (a[c] - b[c]).abs() < tol,
                "round-trip mismatch axis {c}: {} vs {} (tol {})",
                a[c],
                b[c],
                tol
            );
        }
    }
}

/// BYTE-normalized NORMAL + TEXCOORD round-trip through the encoder
/// retain their original component types.
#[test]
fn round_trip_byte_normalized_normal_and_texcoord() {
    let positions: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let nrm_off = bin.len();
    let normals: [[i8; 3]; 2] = [[0, 127, 0], [-127, 0, 0]];
    for n in normals {
        for c in n {
            bin.push(c as u8);
        }
        bin.push(0); // stride 4
    }
    let uv_off = bin.len();
    let uvs: [[u8; 2]; 2] = [[255, 0], [128, 64]];
    for uv in uvs {
        bin.extend_from_slice(&uv);
        bin.extend_from_slice(&[0u8, 0]); // stride 4
    }
    let total = bin.len();
    let nrm_len = uv_off - nrm_off;
    let uv_len = total - uv_off;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 0.0, 0.0]},
              {"bufferView": 1, "componentType": 5120, "count": 2, "type": "VEC3", "normalized": true},
              {"bufferView": 2, "componentType": 5121, "count": 2, "type": "VEC2", "normalized": true}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {nrm_off}}},
              {{"buffer": 0, "byteOffset": {nrm_off}, "byteLength": {nrm_len}, "byteStride": 4}},
              {{"buffer": 0, "byteOffset": {uv_off}, "byteLength": {uv_len}, "byteStride": 4}}"#
        ),
        r#"{"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode");
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    let accs = json["accessors"].as_array().expect("accessors");

    // Find the NORMAL + TEXCOORD_0 accessors by their attribute names
    // (POSITION can be index 0; the rest are appended in encoder
    // order). Walk the accessor array and verify the BYTE / UBYTE
    // entries are present.
    let mut saw_byte_normal = false;
    let mut saw_ubyte_texcoord = false;
    for acc in accs {
        match (
            acc["componentType"].as_u64(),
            acc["type"].as_str(),
            acc["normalized"].as_bool().unwrap_or(false),
        ) {
            (Some(5120), Some("VEC3"), true) => saw_byte_normal = true,
            (Some(5121), Some("VEC2"), true) => saw_ubyte_texcoord = true,
            _ => {}
        }
    }
    assert!(saw_byte_normal, "BYTE NORMAL accessor not emitted");
    assert!(saw_ubyte_texcoord, "UBYTE TEXCOORD_0 accessor not emitted");

    // Round-trip: decoded normal values must match the source within
    // the BYTE-normalized precision (1 / 127).
    let mut dec2 = GltfDecoder::new();
    let scene2 = dec2.decode(&bytes).expect("decode round-trip");
    let nrm = scene2.meshes[0].primitives[0]
        .normals
        .as_ref()
        .expect("normals");
    let tol = 2.0 / 127.0;
    assert!((nrm[0][1] - 1.0).abs() < tol, "{:?}", nrm[0]); // 127/127
    assert!((nrm[1][0] - -1.0).abs() < tol, "{:?}", nrm[1]); // -127/127
}

/// A plain all-FLOAT scene MUST NOT gain `extensionsRequired` —
/// quantisation is opt-in via the `__attr_quant` sentinel.
#[test]
fn float_only_scene_does_not_declare_extension() {
    let positions: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
            "min": [0.0, 0.0, 0.0], "max": [1.0, 0.0, 0.0]}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}}}"#),
        r#"{"POSITION": 0}"#,
        "",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode");
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    let used = json["extensionsUsed"].as_array();
    let required = json["extensionsRequired"].as_array();
    let has = |opt: Option<&Vec<serde_json::Value>>| {
        opt.map(|a| a.iter().any(|v| v == "KHR_mesh_quantization"))
            .unwrap_or(false)
    };
    assert!(
        !has(used) && !has(required),
        "FLOAT-only scene must not declare KHR_mesh_quantization: used={used:?} required={required:?}"
    );

    // The POSITION accessor stays FLOAT.
    let acc = &json["accessors"][0];
    assert_eq!(acc["componentType"].as_u64(), Some(5126));
}

/// BYTE-normalized TANGENT VEC4 (the only TANGENT entry the extension
/// table allows) round-trips through the encoder.
#[test]
fn round_trip_byte_normalized_tangent() {
    let positions: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let tan_off = bin.len();
    // VEC4 BYTE = 4 bytes, already aligned.
    let tangents: [[i8; 4]; 2] = [[0, 0, 127, 127], [127, 0, 0, -127]];
    for t in tangents {
        for c in t {
            bin.push(c as u8);
        }
    }
    let total = bin.len();
    let tan_len = total - tan_off;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 0.0, 0.0]},
              {"bufferView": 1, "componentType": 5120, "count": 2, "type": "VEC4", "normalized": true}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {tan_off}}},
              {{"buffer": 0, "byteOffset": {tan_off}, "byteLength": {tan_len}, "byteStride": 4}}"#
        ),
        r#"{"POSITION": 0, "TANGENT": 1}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode");
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");

    // TANGENT accessor MUST be BYTE/normalized=true.
    let accs = json["accessors"].as_array().expect("accessors");
    let saw = accs.iter().any(|a| {
        a["componentType"].as_u64() == Some(5120)
            && a["type"].as_str() == Some("VEC4")
            && a["normalized"].as_bool() == Some(true)
    });
    assert!(saw, "BYTE-normalized TANGENT VEC4 accessor not emitted");

    // Extension declared.
    let req = json["extensionsRequired"].as_array().expect("required");
    assert!(req.iter().any(|v| v == "KHR_mesh_quantization"));
}
