//! Quantized morph-target attribute decode + encode round-trip per
//! `docs/3d/gltf/extensions/KHR_mesh_quantization.md` §Extending Morph
//! Target Attributes.
//!
//! The §Extending Morph Target Attributes table extends the base
//! §3.7.2.2 morph attribute set with 8-bit / 16-bit integer storage:
//!
//! | Name       | Type | Component Type(s)                                |
//! |------------|------|--------------------------------------------------|
//! | POSITION   | VEC3 | byte / byte normalized / short / short normalized |
//! | NORMAL     | VEC3 | byte normalized / short normalized                |
//! | TANGENT    | VEC3 | byte normalized / short normalized                |
//! | TEXCOORD_n | VEC2 | byte / short                                      |
//!
//! Quantised data must be aligned to 4-byte element boundaries
//! (§Extending Mesh Attributes alignment rule covers morph data too —
//! the morph table's note re-states it). The decoder dequantises via
//! the spec equations and records the original (componentType,
//! normalized) tuple under the per-primitive `__morph_attr_quant`
//! sentinel; the encoder re-emits the morph deltas with the same
//! storage form when re-encoding the scene.

use base64::Engine as _;
use oxideav_gltf::{json_encoder, GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};

/// Construct a small glTF document carrying one mesh with a single
/// primitive that has one morph target whose POSITION attribute is
/// stored as SHORT/normalized=true. POSITION base attribute stays
/// FLOAT so we exercise the morph-only quantization path.
fn build_short_normalized_morph_position_doc() -> Vec<u8> {
    // bin layout:
    //   0..36     : 3 base POSITION vec3 floats
    //   36..52    : 3 morph POSITION SHORT-normalized VEC3 elements,
    //               padded to 8-byte stride (6 bytes data + 2 pad)
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_off = 0;
    let morph_off = bin.len();
    // morph POSITION SHORT-normalized deltas (i16 values): one slot
    // per vertex. Each element is 3 i16 (6 bytes) padded to 8 bytes
    // (the spec-mandated 4-byte vertex stride => 8 for VEC3 SHORT).
    let morph_deltas: [[i16; 3]; 3] = [
        [32767, 0, 0],      // +1.0 along X (max positive normalized SHORT)
        [-32767, 16383, 0], // -1.0 along X, ~0.5 along Y
        [0, -32768, 100],   // 0, clamped-to-(-1.0) along Y, tiny Z
    ];
    for v in morph_deltas {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]); // pad to 8-byte stride
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "extensionsUsed": [ "KHR_mesh_quantization" ],
        "extensionsRequired": [ "KHR_mesh_quantization" ],
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": {pos_off}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {morph_off}, "byteLength": 24, "byteStride": 8 }}
        ],
        "accessors": [
            {{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
            }},
            {{
                "bufferView": 1, "componentType": 5122, "count": 3, "type": "VEC3",
                "normalized": true,
                "min": [-32767, -32768, 0], "max": [32767, 16383, 100]
            }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{
                        "attributes": {{ "POSITION": 0 }},
                        "targets": [ {{ "POSITION": 1 }} ]
                    }}
                ]
            }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    );
    json.into_bytes()
}

#[test]
fn decode_short_normalized_morph_position_dequantises_via_spec_equation() {
    let doc = build_short_normalized_morph_position_doc();
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode quantized morph POSITION");

    // Pull the morph-target sentinel: the delta values must come back
    // as f32 in [-1, 1] range per §Decoding Quantized Data:
    //   f = max(c / 32767.0, -1.0)
    let mt = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .expect("__morph_targets sentinel populated");
    let targets = mt.as_array().expect("targets array");
    assert_eq!(targets.len(), 1, "one morph target");
    let pos = targets[0]
        .as_object()
        .and_then(|o| o.get("POSITION"))
        .and_then(|v| v.as_array())
        .expect("POSITION delta array");
    assert_eq!(pos.len(), 3);

    // Vertex 0: [32767, 0, 0] -> [1.0, 0.0, 0.0]
    let v0 = pos[0].as_array().unwrap();
    assert!((v0[0].as_f64().unwrap() - 1.0).abs() < 1.0 / 32767.0);
    assert!(v0[1].as_f64().unwrap().abs() < 1e-6);
    assert!(v0[2].as_f64().unwrap().abs() < 1e-6);

    // Vertex 1: [-32767, 16383, 0] -> [-1.0, ~0.5, 0.0]
    let v1 = pos[1].as_array().unwrap();
    assert!((v1[0].as_f64().unwrap() + 1.0).abs() < 1.0 / 32767.0);
    assert!((v1[1].as_f64().unwrap() - (16383.0 / 32767.0)).abs() < 1e-6);

    // Vertex 2: [0, -32768, 100] -> [0.0, -1.0 (clamped), ~0.003]
    let v2 = pos[2].as_array().unwrap();
    assert!(v2[0].as_f64().unwrap().abs() < 1e-6);
    assert!(
        (v2[1].as_f64().unwrap() + 1.0).abs() < 1e-6,
        "-32768 clamps to -1.0 per the spec equation"
    );

    // The per-primitive `__morph_attr_quant` sentinel must record the
    // storage form so the encoder can round-trip the SHORT/normalized
    // shape.
    let mq = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_attr_quant")
        .expect("__morph_attr_quant sentinel populated");
    let per_target = mq
        .as_object()
        .and_then(|o| o.get("0"))
        .and_then(|v| v.as_object())
        .expect("per-target entry");
    let entry = per_target
        .get("POSITION")
        .and_then(|v| v.as_object())
        .expect("per-attribute entry");
    assert_eq!(
        entry.get("componentType").and_then(|v| v.as_u64()),
        Some(5122)
    );
    assert_eq!(
        entry.get("normalized").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn quantized_morph_position_round_trip_preserves_extension_declaration() {
    let doc = build_short_normalized_morph_position_doc();
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode");

    // Re-encode the scene as JSON and verify:
    //   * extensionsUsed AND extensionsRequired list KHR_mesh_quantization
    //     (the extension is mandatory once any quantized attribute or
    //     quantized morph delta surfaces — §Overview "the extension is
    //     not optional").
    //   * The morph-target POSITION accessor stays SHORT/normalized=true
    //     (the encoder didn't promote it to FLOAT).
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON");

    let used = json["extensionsUsed"]
        .as_array()
        .expect("extensionsUsed array");
    let required = json["extensionsRequired"]
        .as_array()
        .expect("extensionsRequired array — quantized morph data is not optional");
    assert!(
        used.iter().any(|v| v == "KHR_mesh_quantization"),
        "extensionsUsed lacks KHR_mesh_quantization: {used:?}"
    );
    assert!(
        required.iter().any(|v| v == "KHR_mesh_quantization"),
        "extensionsRequired lacks KHR_mesh_quantization: {required:?}"
    );

    // The morph-target POSITION accessor index lives at
    // meshes[0].primitives[0].targets[0].POSITION; resolve it and
    // check the (componentType, normalized) pair survived.
    let prim = &json["meshes"][0]["primitives"][0];
    let morph_pos_acc_idx = prim["targets"][0]["POSITION"]
        .as_u64()
        .expect("morph POSITION accessor index") as usize;
    let acc = &json["accessors"][morph_pos_acc_idx];
    assert_eq!(acc["componentType"].as_u64(), Some(5122), "SHORT preserved");
    assert_eq!(
        acc["normalized"].as_bool(),
        Some(true),
        "normalized preserved"
    );
    assert_eq!(acc["type"].as_str(), Some("VEC3"));
    assert_eq!(acc["count"].as_u64(), Some(3));
}

#[test]
fn quantized_morph_position_byte_round_trip_via_glb() {
    // BYTE-normalized morph POSITION VEC3 — exercises the 4-byte
    // padding rule (3 raw bytes + 1 pad per element).
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let morph_off = bin.len();
    // 3 elements × (3 i8 + 1 pad) = 12 bytes, stride 4.
    let morph_deltas: [[i8; 3]; 3] = [[127, 0, 0], [-127, 64, 0], [0, -127, 32]];
    for v in morph_deltas {
        for c in v {
            bin.push(c as u8);
        }
        bin.push(0); // pad to 4
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "extensionsUsed": [ "KHR_mesh_quantization" ],
        "extensionsRequired": [ "KHR_mesh_quantization" ],
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {morph_off}, "byteLength": 12, "byteStride": 4 }}
        ],
        "accessors": [
            {{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
            }},
            {{
                "bufferView": 1, "componentType": 5120, "count": 3, "type": "VEC3",
                "normalized": true,
                "min": [-127, -127, 0], "max": [127, 64, 32]
            }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{
                        "attributes": {{ "POSITION": 0 }},
                        "targets": [ {{ "POSITION": 1 }} ]
                    }}
                ]
            }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes();

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode BYTE-normalized morph");

    // Verify the decoder dequantised correctly:
    //   [127, 0, 0] -> [1.0, 0.0, 0.0] (f = max(c/127, -1))
    let mt = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .unwrap()
        .as_array()
        .unwrap();
    let v0 = mt[0].as_object().unwrap()["POSITION"].as_array().unwrap()[0]
        .as_array()
        .unwrap();
    assert!((v0[0].as_f64().unwrap() - 1.0).abs() < 1e-6);

    // Round-trip through GLB and confirm the morph accessor still
    // carries BYTE-normalized.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode .glb");
    let scene2 = dec.decode(&glb).expect("re-decode .glb");
    let mq2 = scene2.meshes[0].primitives[0]
        .extras
        .get("__morph_attr_quant")
        .expect("morph quant sentinel re-decoded");
    let entry = mq2
        .as_object()
        .and_then(|o| o.get("0"))
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("POSITION"))
        .and_then(|v| v.as_object())
        .expect("POSITION entry");
    assert_eq!(
        entry.get("componentType").and_then(|v| v.as_u64()),
        Some(5120)
    );
    assert_eq!(
        entry.get("normalized").and_then(|v| v.as_bool()),
        Some(true)
    );

    // Re-decoded deltas should match the first decode to within the
    // BYTE-normalized precision floor (1/127).
    let mt2 = scene2.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .unwrap()
        .as_array()
        .unwrap();
    let v0a = mt[0].as_object().unwrap()["POSITION"].as_array().unwrap();
    let v0b = mt2[0].as_object().unwrap()["POSITION"].as_array().unwrap();
    assert_eq!(v0a.len(), v0b.len());
    for (a, b) in v0a.iter().zip(v0b.iter()) {
        let ca = a.as_array().unwrap();
        let cb = b.as_array().unwrap();
        for (x, y) in ca.iter().zip(cb.iter()) {
            let dx = (x.as_f64().unwrap() - y.as_f64().unwrap()).abs();
            assert!(dx < 1.0 / 127.0 + 1e-6, "delta drift {dx} > 1/127");
        }
    }
}

#[test]
fn quantized_morph_short_normalized_normal_and_tangent_round_trip() {
    // SHORT-normalized morph NORMAL (VEC3) + morph TANGENT (VEC3 — per
    // §Extending Morph Target Attributes the morph-target TANGENT is
    // VEC3, NOT VEC4 like the base attribute).
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let nrm_off = bin.len();
    // Three VEC3 SHORT-normalized normals, 8-byte stride (6+2 pad).
    let nrm_deltas: [[i16; 3]; 3] = [[0, 32767, 0], [0, 0, 32767], [-32767, 0, 0]];
    for v in nrm_deltas {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]);
    }
    let tan_off = bin.len();
    let tan_deltas: [[i16; 3]; 3] = [[32767, 0, 0], [0, 32767, 0], [0, 0, 32767]];
    for v in tan_deltas {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]);
    }
    // Base TANGENT (VEC4 float) — §3.7.2.2 requires that each morphed
    // attribute has an original attribute of the same name in the
    // primitive; the morph deltas above target NORMAL + TANGENT.
    let base_tan_off = bin.len();
    let base_tangents: [[f32; 4]; 3] = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];
    for v in base_tangents {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "extensionsUsed": [ "KHR_mesh_quantization" ],
        "extensionsRequired": [ "KHR_mesh_quantization" ],
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {nrm_off}, "byteLength": 24, "byteStride": 8 }},
            {{ "buffer": 0, "byteOffset": {tan_off}, "byteLength": 24, "byteStride": 8 }},
            {{ "buffer": 0, "byteOffset": {base_tan_off}, "byteLength": 48 }}
        ],
        "accessors": [
            {{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
            }},
            {{
                "bufferView": 1, "componentType": 5122, "count": 3, "type": "VEC3",
                "normalized": true
            }},
            {{
                "bufferView": 2, "componentType": 5122, "count": 3, "type": "VEC3",
                "normalized": true
            }},
            {{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"
            }},
            {{
                "bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC4"
            }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{
                        "attributes": {{ "POSITION": 0, "NORMAL": 3, "TANGENT": 4 }},
                        "targets": [ {{ "NORMAL": 1, "TANGENT": 2 }} ]
                    }}
                ]
            }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes();

    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(&doc)
        .expect("decode quantized morph NORMAL+TANGENT");

    // Verify the per-target quant sentinel carries both NORMAL and
    // TANGENT, both SHORT-normalized.
    let mq = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_attr_quant")
        .expect("__morph_attr_quant sentinel");
    let per_target = mq
        .as_object()
        .unwrap()
        .get("0")
        .unwrap()
        .as_object()
        .unwrap();
    assert!(per_target.contains_key("NORMAL"));
    assert!(per_target.contains_key("TANGENT"));

    // Round-trip and confirm the morph TANGENT accessor in the
    // re-encoded JSON is VEC3 (NOT VEC4 — the morph spec specifically
    // strips the W handedness from the TANGENT delta).
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode JSON");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON");
    let prim = &json["meshes"][0]["primitives"][0];
    let morph_tan_idx = prim["targets"][0]["TANGENT"].as_u64().unwrap() as usize;
    let tan_acc = &json["accessors"][morph_tan_idx];
    assert_eq!(tan_acc["type"].as_str(), Some("VEC3"));
    assert_eq!(tan_acc["componentType"].as_u64(), Some(5122));
    assert_eq!(tan_acc["normalized"].as_bool(), Some(true));
}

#[test]
fn quantized_morph_rejected_when_extension_not_declared() {
    // SHORT-normalized morph POSITION but no `KHR_mesh_quantization`
    // in extensionsUsed → the decoder MUST refuse.
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let morph_off = bin.len();
    for _ in 0..3 {
        for c in [0i16, 0, 0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]);
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {morph_off}, "byteLength": 24, "byteStride": 8 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5122, "count": 3, "type": "VEC3", "normalized": true, "min": [0, 0, 0], "max": [0, 0, 0] }}
        ],
        "meshes": [
            {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }}, "targets": [ {{ "POSITION": 1 }} ] }} ] }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes();

    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc)
        .expect_err("decoder must refuse undeclared quantized morph");
    let msg = format!("{err}");
    assert!(
        msg.contains("KHR_mesh_quantization")
            || msg.contains("not in extensionsUsed")
            || msg.contains("MorphTargetAttributeComponent"),
        "error should call out the undeclared quantized morph storage form, got: {msg}"
    );
}

#[test]
fn quantized_morph_texcoord_round_trip() {
    // BYTE morph TEXCOORD_0 VEC2 — exercises the §Extending Morph
    // Target Attributes TEXCOORD row. Unnormalized BYTE so the
    // dequantizer casts straight to f32.
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let morph_off = bin.len();
    // VEC2 BYTE: 2 bytes per element padded to 4-byte stride.
    let morph_uv: [[i8; 2]; 3] = [[5, 0], [0, 10], [-3, 7]];
    for v in morph_uv {
        for c in v {
            bin.push(c as u8);
        }
        bin.extend_from_slice(&[0u8, 0]); // pad to 4-byte stride
    }
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let doc = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "extensionsUsed": [ "KHR_mesh_quantization" ],
        "extensionsRequired": [ "KHR_mesh_quantization" ],
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {morph_off}, "byteLength": 12, "byteStride": 4 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5120, "count": 3, "type": "VEC2", "normalized": false }},
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC2" }}
        ],
        "meshes": [
            {{ "primitives": [ {{ "attributes": {{ "POSITION": 0, "TEXCOORD_0": 2 }}, "targets": [ {{ "TEXCOORD_0": 1 }} ] }} ] }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes();

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode quantized morph TEXCOORD_0");

    let mt = scene.meshes[0].primitives[0]
        .extras
        .get("__morph_targets")
        .unwrap()
        .as_array()
        .unwrap();
    let tex = mt[0].as_object().unwrap()["TEXCOORD_0"].as_array().unwrap();
    assert_eq!(tex.len(), 3);
    // Unnormalized BYTE: f == c per spec.
    let v0 = tex[0].as_array().unwrap();
    assert!((v0[0].as_f64().unwrap() - 5.0).abs() < 1e-6);
    let v2 = tex[2].as_array().unwrap();
    assert!((v2[0].as_f64().unwrap() + 3.0).abs() < 1e-6);

    // Encode → confirm the morph TEXCOORD_0 accessor in the re-emit
    // is VEC2 BYTE, NOT FLOAT.
    let mut enc = json_encoder();
    let bytes = enc.encode(&scene).expect("encode JSON");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("parse JSON");
    let prim = &json["meshes"][0]["primitives"][0];
    let morph_tex_idx = prim["targets"][0]["TEXCOORD_0"].as_u64().unwrap() as usize;
    let tex_acc = &json["accessors"][morph_tex_idx];
    assert_eq!(tex_acc["type"].as_str(), Some("VEC2"));
    assert_eq!(tex_acc["componentType"].as_u64(), Some(5120));
    assert!(!tex_acc["normalized"].as_bool().unwrap_or(false));
}
