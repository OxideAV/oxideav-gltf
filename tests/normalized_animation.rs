//! Normalised-integer animation output accessors per glTF 2.0 §3.11.
//!
//! Spec lets ROTATION (VEC4) and MORPH_WEIGHTS (SCALAR) sampler outputs
//! use any of `BYTE / UBYTE / SHORT / USHORT` component types when
//! `normalized: true`. The decoder dequantises via the §3.6.2.2
//! equations:
//!
//! - i8  : f = max(c / 127.0, -1.0)
//! - u8  : f = c / 255.0
//! - i16 : f = max(c / 32767.0, -1.0)
//! - u16 : f = c / 65535.0
//!
//! r3 only adds DECODE — the encoder still emits FLOAT. These tests
//! hand-craft glTF JSON with normalised integers and verify the
//! dequantised values match the spec equations within a tight epsilon.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::{AnimationProperty, AnimationValues, Mesh3DDecoder};

fn build_glb_or_json_with_animation(
    component_type: u32,
    bytes_per_component: usize,
    output_count: u32,
    output_type: &str,
    output_data: Vec<u8>,
    path: &str,
) -> String {
    // Layout the binary buffer:
    //   [0..pos_len) : 9 floats POSITION (3 VEC3) so the mesh is well-formed
    //   [pos_len..) : keyframe times (SCALAR FLOAT) — 2 keyframes
    //   then the output payload
    let mut bin: Vec<u8> = Vec::new();
    let positions = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions.iter() {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_byte_len = bin.len();

    // Two keyframes — `(0.0, 1.0)`. min/max required.
    let key_offset = bin.len();
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&1.0f32.to_le_bytes());
    let key_byte_len = bin.len() - key_offset;

    // Pad to alignment of the integer width.
    while bin.len() % bytes_per_component.max(4) != 0 {
        bin.push(0);
    }
    let out_offset = bin.len();
    bin.extend_from_slice(&output_data);
    let out_byte_len = bin.len() - out_offset;

    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let total = bin.len();

    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": {pos_byte_len} }},
            {{ "buffer": 0, "byteOffset": {key_offset}, "byteLength": {key_byte_len} }},
            {{ "buffer": 0, "byteOffset": {out_offset}, "byteLength": {out_byte_len} }}
        ],
        "accessors": [
            {{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
            }},
            {{
                "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR",
                "min": [0.0], "max": [1.0]
            }},
            {{
                "bufferView": 2, "componentType": {component_type},
                "count": {output_count}, "type": "{output_type}",
                "normalized": true
            }}
        ],
        "meshes": [
            {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ],
        "scene": 0,
        "animations": [
            {{
                "channels": [
                    {{
                        "sampler": 0,
                        "target": {{ "node": 0, "path": "{path}" }}
                    }}
                ],
                "samplers": [
                    {{ "input": 1, "output": 2 }}
                ]
            }}
        ]
    }}"#,
        component_type = component_type,
        pos_byte_len = pos_byte_len,
        key_offset = key_offset,
        key_byte_len = key_byte_len,
        out_offset = out_offset,
        out_byte_len = out_byte_len,
        output_count = output_count,
        output_type = output_type,
        path = path,
        total = total,
        b64 = b64,
    )
}

#[test]
fn ubyte_normalized_morph_weights_roundtrip() {
    // 4 morph weights, encoded as u8 normalised. Bytes 255 / 0 / 128 / 64
    // dequantise to 1.0 / 0.0 / 0.501... / 0.251...
    let json = build_glb_or_json_with_animation(
        5121, // UBYTE
        1,
        4,
        "SCALAR",
        vec![255u8, 0, 128, 64],
        "weights",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();
    let ch = &scene.animations[0].channels[0];
    assert_eq!(ch.target.property, AnimationProperty::MorphWeights);
    match &ch.sampler.values {
        AnimationValues::Scalar(v) => {
            assert_eq!(v.len(), 4);
            assert!((v[0] - 1.0).abs() < 1e-6, "v[0]={}", v[0]);
            assert!((v[1] - 0.0).abs() < 1e-6, "v[1]={}", v[1]);
            assert!((v[2] - (128.0 / 255.0)).abs() < 1e-6, "v[2]={}", v[2]);
            assert!((v[3] - (64.0 / 255.0)).abs() < 1e-6, "v[3]={}", v[3]);
        }
        other => panic!("expected Scalar, got {other:?}"),
    }
}

#[test]
fn ibyte_normalized_rotation_roundtrip() {
    // 1 rotation quaternion encoded as i8 normalised. Bytes
    // (0, 0, 90, 90) -> (0.0, 0.0, ~0.708, ~0.708).
    // Spec equation: f = max(c / 127, -1).
    let json = build_glb_or_json_with_animation(
        5120, // BYTE
        1,
        1,
        "VEC4",
        vec![0u8, 0, 90, 90],
        "rotation",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();
    let ch = &scene.animations[0].channels[0];
    assert_eq!(ch.target.property, AnimationProperty::Rotation);
    match &ch.sampler.values {
        AnimationValues::Quat(v) => {
            assert_eq!(v.len(), 1);
            assert!(v[0][0].abs() < 1e-6);
            assert!(v[0][1].abs() < 1e-6);
            assert!((v[0][2] - (90.0 / 127.0)).abs() < 1e-6);
            assert!((v[0][3] - (90.0 / 127.0)).abs() < 1e-6);
        }
        other => panic!("expected Quat, got {other:?}"),
    }
}

#[test]
fn ushort_normalized_morph_weights_roundtrip() {
    // 2 morph weights as u16 normalised. Little-endian:
    //   0xFFFF -> 1.0
    //   0x0000 -> 0.0
    let json = build_glb_or_json_with_animation(
        5123, // UNSIGNED_SHORT
        2,
        2,
        "SCALAR",
        vec![0xFF, 0xFF, 0x00, 0x00],
        "weights",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();
    let ch = &scene.animations[0].channels[0];
    match &ch.sampler.values {
        AnimationValues::Scalar(v) => {
            assert_eq!(v.len(), 2);
            assert!((v[0] - 1.0).abs() < 1e-6);
            assert!((v[1] - 0.0).abs() < 1e-6);
        }
        other => panic!("expected Scalar, got {other:?}"),
    }
}

#[test]
fn ishort_normalized_rotation_roundtrip() {
    // 1 quat as i16 normalised; 32767 -> 1.0 (clamped through max).
    // Bytes le: 0x7FFF, 0x8001 (=-32767 -> -1), 0x0000, 0x7FFF
    let json = build_glb_or_json_with_animation(
        5122, // SHORT
        2,
        1,
        "VEC4",
        vec![0xFF, 0x7F, 0x01, 0x80, 0x00, 0x00, 0xFF, 0x7F],
        "rotation",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();
    let ch = &scene.animations[0].channels[0];
    match &ch.sampler.values {
        AnimationValues::Quat(v) => {
            assert_eq!(v.len(), 1);
            assert!((v[0][0] - 1.0).abs() < 1e-6);
            assert!((v[0][1] - -1.0).abs() < 1e-6);
            assert!(v[0][2].abs() < 1e-6);
            assert!((v[0][3] - 1.0).abs() < 1e-6);
        }
        other => panic!("expected Quat, got {other:?}"),
    }
}

#[test]
fn translation_rejects_normalized_int() {
    // Spec restricts TRANSLATION/SCALE outputs to FLOAT; loading a
    // u8-normalized translation accessor must be a hard error.
    let json = build_glb_or_json_with_animation(
        5121,
        1,
        2,
        "VEC3",
        vec![10u8, 20, 30, 40, 50, 60],
        "translation",
    );
    let mut dec = GltfDecoder::new();
    let result = dec.decode(json.as_bytes());
    assert!(
        result.is_err(),
        "translation must reject non-FLOAT, got {result:?}"
    );
}

#[test]
fn missing_normalized_flag_on_int_output_errors() {
    // Same byte stream as the ubyte morph-weights test but the
    // accessor is missing `"normalized": true` — spec requires that
    // flag for any non-FLOAT animation output. We sniff the absence
    // and fail rather than silently mis-decode the integers as raw
    // sample counts.
    use base64::Engine as _;
    let mut bin: Vec<u8> = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let key_offset = bin.len();
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&1.0f32.to_le_bytes());
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let out_offset = bin.len();
    bin.extend_from_slice(&[1u8, 2, 3, 4]);
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {key_offset}, "byteLength": 8 }},
            {{ "buffer": 0, "byteOffset": {out_offset}, "byteLength": 4 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min":[0,0,0], "max":[1,1,0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min":[0], "max":[1] }},
            {{ "bufferView": 2, "componentType": 5121, "count": 4, "type": "SCALAR" }}
        ],
        "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0,
        "animations": [
            {{ "channels": [ {{ "sampler": 0, "target": {{ "node": 0, "path": "weights" }} }} ],
               "samplers": [ {{ "input": 1, "output": 2 }} ] }}
        ]
    }}"#
    );
    let mut dec = GltfDecoder::new();
    assert!(dec.decode(json.as_bytes()).is_err());
}
