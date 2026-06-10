//! `KHR_animation_pointer` extension — animation channels that drive
//! arbitrary mutable glTF properties via JSON Pointer (RFC 6901) per
//! `docs/3d/gltf/extensions/KHR_animation_pointer.md`. The decoder
//! siphons pointer-targeted channels (which the base spec would
//! discard because they don't bind to a node) into
//! `Scene3D::extras["KHR_animation_pointer"]` as
//! `{ "animations": [ { "animation": N, "name": "...", "channels": [...] } ] }`;
//! the encoder lifts them back into the typed `target.extensions
//! .KHR_animation_pointer` block and appends `KHR_animation_pointer`
//! to `extensionsUsed`. The §3.12 stack validator rejects documents
//! carrying the data block without the declaration
//! (`ExtensionStackUsedNotDeclared`) and adds three spec-explicit
//! per-channel rules: a `node` on a pointer channel is rejected
//! (`ExtensionStackAnimationPointerNode`); `target.path == "pointer"`
//! without the extension data is rejected
//! (`ExtensionStackAnimationPointerData`); duplicate pointer strings
//! within one animation are rejected
//! (`ExtensionStackAnimationPointerDuplicate`).

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};

/// Hand-built glTF JSON that drives `materials[0].pbrMetallicRoughness
/// .baseColorFactor` via the pointer extension. Two keyframes
/// (`t=0` and `t=1`) each carry a VEC4 colour.
fn baseline_pointer_doc() -> Vec<u8> {
    // Layout of the .bin payload (96 B total):
    //   off  0..8  : input  (2 × f32 keyframes — 0.0, 1.0)
    //   off  8..40 : output (2 × VEC4 — [1,0,0,1] then [0,1,0,1])
    let mut bin: Vec<u8> = Vec::new();
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&1.0f32.to_le_bytes());
    for f in [1.0f32, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0] {
        bin.extend_from_slice(&f.to_le_bytes());
    }
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let uri = format!("data:application/octet-stream;base64,{b64}");
    let buf_len = bin.len();
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_animation_pointer"],
            "buffers": [{{ "uri": "{uri}", "byteLength": {buf_len} }}],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": 0, "byteLength": 8 }},
                {{ "buffer": 0, "byteOffset": 8, "byteLength": 32 }}
            ],
            "accessors": [
                {{ "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0] }},
                {{ "bufferView": 1, "componentType": 5126, "count": 2, "type": "VEC4" }}
            ],
            "materials": [
                {{ "pbrMetallicRoughness": {{ "baseColorFactor": [1.0, 0.0, 0.0, 1.0] }} }}
            ],
            "animations": [
                {{
                    "channels": [
                        {{
                            "sampler": 0,
                            "target": {{
                                "path": "pointer",
                                "extensions": {{
                                    "KHR_animation_pointer": {{
                                        "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor"
                                    }}
                                }}
                            }}
                        }}
                    ],
                    "samplers": [
                        {{ "input": 0, "interpolation": "LINEAR", "output": 1 }}
                    ]
                }}
            ]
        }}"#
    );
    json.into_bytes()
}

#[test]
fn pointer_channel_decodes_into_extras_side_channel() {
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&baseline_pointer_doc()).unwrap();
    let v = scene
        .extras
        .get("KHR_animation_pointer")
        .expect("side-channel roster populated on decode");
    let anims = v
        .as_object()
        .and_then(|o| o.get("animations"))
        .and_then(|a| a.as_array())
        .expect("animations array present");
    assert_eq!(anims.len(), 1, "exactly one animation has pointer channels");
    let entry = anims[0].as_object().unwrap();
    assert_eq!(entry.get("animation").and_then(|x| x.as_u64()), Some(0));
    let channels = entry
        .get("channels")
        .and_then(|a| a.as_array())
        .expect("channels array");
    assert_eq!(channels.len(), 1);
    let ch = channels[0].as_object().unwrap();
    assert_eq!(
        ch.get("pointer").and_then(|p| p.as_str()),
        Some("/materials/0/pbrMetallicRoughness/baseColorFactor"),
        "pointer string round-trips"
    );
    assert_eq!(
        ch.get("interpolation").and_then(|p| p.as_str()),
        Some("LINEAR")
    );
    assert_eq!(ch.get("output_kind").and_then(|p| p.as_str()), Some("VEC4"));
    let input: Vec<f64> = ch
        .get("input")
        .and_then(|a| a.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(input, vec![0.0, 1.0]);
    let output: Vec<f64> = ch
        .get("output")
        .and_then(|a| a.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(output, vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn pointer_channel_roundtrips_through_glb() {
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&baseline_pointer_doc()).unwrap();
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let scene2 = GltfDecoder::new().decode(&glb).unwrap();
    let v = scene2
        .extras
        .get("KHR_animation_pointer")
        .expect("side-channel survives a glb round-trip");
    let anims = v
        .as_object()
        .and_then(|o| o.get("animations"))
        .and_then(|a| a.as_array())
        .unwrap();
    assert_eq!(anims.len(), 1);
    let entry = anims[0].as_object().unwrap();
    let ch = entry
        .get("channels")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_object())
        .unwrap();
    assert_eq!(
        ch.get("pointer").and_then(|p| p.as_str()),
        Some("/materials/0/pbrMetallicRoughness/baseColorFactor"),
    );
}

#[test]
fn pointer_channel_emits_extensions_used_on_encode() {
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&baseline_pointer_doc()).unwrap();
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_animation_pointer\""),
        "KHR_animation_pointer must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"path\":\"pointer\""),
        "target.path must be \"pointer\" on emit, got: {raw}"
    );
}

#[test]
fn pointer_channel_without_extensions_used_is_rejected() {
    // §3.12 — a channel carries the data block but the document does
    // NOT declare `KHR_animation_pointer` in `extensionsUsed`. Reject.
    let json = br#"{
        "asset": { "version": "2.0" },
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": {
                            "path": "pointer",
                            "extensions": {
                                "KHR_animation_pointer": { "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor" }
                            }
                        }
                    }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_animation_pointer"),
        "expected ExtensionStackUsedNotDeclared for KHR_animation_pointer, got {msg}"
    );
}

#[test]
fn pointer_channel_with_node_is_rejected() {
    // Per `docs/3d/gltf/extensions/KHR_animation_pointer.md`
    // §"Extension Usage": "The animation channel `node` property MUST
    // NOT be set."
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_animation_pointer"],
        "nodes": [{}],
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": {
                            "node": 0,
                            "path": "pointer",
                            "extensions": {
                                "KHR_animation_pointer": { "pointer": "/x" }
                            }
                        }
                    }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerNode"),
        "expected ExtensionStackAnimationPointerNode, got {msg}"
    );
}

#[test]
fn pointer_path_without_extension_data_is_rejected() {
    // Path is `"pointer"` but no extension data attached — spec
    // requires both to be present together.
    let json = br#"{
        "asset": { "version": "2.0" },
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": { "path": "pointer" }
                    }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerData"),
        "expected ExtensionStackAnimationPointerData, got {msg}"
    );
}

#[test]
fn pointer_data_without_pointer_path_is_rejected() {
    // Inverse — data block is attached but path is not `"pointer"`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_animation_pointer"],
        "nodes": [{}],
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": {
                            "node": 0,
                            "path": "translation",
                            "extensions": {
                                "KHR_animation_pointer": { "pointer": "/x" }
                            }
                        }
                    }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerPath"),
        "expected ExtensionStackAnimationPointerPath, got {msg}"
    );
}

#[test]
fn duplicate_pointer_within_one_animation_is_rejected() {
    // Spec §Operation: "different channels of the same animation MUST
    // NOT have identical pointers".
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_animation_pointer"],
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": {
                            "path": "pointer",
                            "extensions": { "KHR_animation_pointer": { "pointer": "/materials/0/emissiveFactor" } }
                        }
                    },
                    {
                        "sampler": 0,
                        "target": {
                            "path": "pointer",
                            "extensions": { "KHR_animation_pointer": { "pointer": "/materials/0/emissiveFactor" } }
                        }
                    }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerDuplicate"),
        "expected ExtensionStackAnimationPointerDuplicate, got {msg}"
    );
}

#[test]
fn malformed_pointer_string_is_rejected() {
    // RFC 6901 §3: a non-empty JSON Pointer MUST start with `/`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_animation_pointer"],
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": {
                            "path": "pointer",
                            "extensions": { "KHR_animation_pointer": { "pointer": "no-leading-slash" } }
                        }
                    }
                ],
                "samplers": [ { "input": 0, "output": 1 } ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerSyntax"),
        "expected ExtensionStackAnimationPointerSyntax, got {msg}"
    );
}

#[test]
fn pointer_channel_does_not_promote_to_typed_channels() {
    // Pointer-targeted channels intentionally do not appear in the
    // typed Animation::channels list (they don't bind to a node).
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&baseline_pointer_doc()).unwrap();
    assert_eq!(scene.animations.len(), 1);
    assert!(
        scene.animations[0].channels.is_empty(),
        "pointer channels stay in the extras side-channel only"
    );
}

/// Walk the `.glb` container and return its JSON chunk's payload bytes.
/// Matches the layout from glTF 2.0 spec §4 (12-byte file header,
/// then chunks of `length:u32, type:u32, payload`).
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}

// --------------------------------------------------------------------------
// r261: §"Output Accessor Component Types" — non-FLOAT output lanes.
//
// The spec branch for `float*` Object Model Data Types accepts FLOAT
// (used as-is), non-normalized integer (cast to float), and
// normalized integer (dequantised via the §3.6.2.2 equations) output
// accessors. r218 only carried the FLOAT lane; r261 lights up the
// remaining eight {componentType, normalized} combinations.
// --------------------------------------------------------------------------

use base64::Engine as _;
fn b64(bin: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bin)
}

/// Hand-build a pointer-targeted doc with a custom output accessor.
/// `output_bytes` is the raw .bin payload that the output accessor
/// will reference; `component_type` + `normalized` + `kind` + `count`
/// describe it to the glTF parser.
fn pointer_doc_with_output(
    output_bytes: &[u8],
    component_type: u32,
    normalized: bool,
    kind: &str,
    count: u32,
) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    // Input: 2 keyframes at t=0 and t=1.
    bin.extend_from_slice(&0.0f32.to_le_bytes());
    bin.extend_from_slice(&1.0f32.to_le_bytes());
    let output_offset = bin.len();
    bin.extend_from_slice(output_bytes);
    let total = bin.len();
    let output_len = output_bytes.len();
    let uri = format!("data:application/octet-stream;base64,{}", b64(&bin));
    let normalized_str = if normalized { "true" } else { "false" };
    format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_animation_pointer"],
            "buffers": [{{ "uri": "{uri}", "byteLength": {total} }}],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": 0, "byteLength": 8 }},
                {{ "buffer": 0, "byteOffset": {output_offset}, "byteLength": {output_len} }}
            ],
            "accessors": [
                {{ "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0] }},
                {{ "bufferView": 1, "componentType": {component_type}, "normalized": {normalized_str}, "count": {count}, "type": "{kind}" }}
            ],
            "materials": [
                {{ "pbrMetallicRoughness": {{ "baseColorFactor": [1.0, 0.0, 0.0, 1.0] }} }}
            ],
            "animations": [
                {{
                    "channels": [
                        {{
                            "sampler": 0,
                            "target": {{
                                "path": "pointer",
                                "extensions": {{
                                    "KHR_animation_pointer": {{
                                        "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor"
                                    }}
                                }}
                            }}
                        }}
                    ],
                    "samplers": [
                        {{ "input": 0, "interpolation": "LINEAR", "output": 1 }}
                    ]
                }}
            ]
        }}"#
    )
    .into_bytes()
}

fn pointer_channel_output(
    scene: &oxideav_mesh3d::Scene3D,
) -> (&serde_json::Map<String, serde_json::Value>, Vec<f32>) {
    let v = scene
        .extras
        .get("KHR_animation_pointer")
        .expect("side-channel populated");
    let anims = v
        .as_object()
        .and_then(|o| o.get("animations"))
        .and_then(|a| a.as_array())
        .unwrap();
    let ch = anims[0]
        .get("channels")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_object())
        .unwrap();
    let out: Vec<f32> = ch
        .get("output")
        .and_then(|a| a.as_array())
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap() as f32)
        .collect();
    (ch, out)
}

#[test]
fn pointer_output_unsigned_byte_normalized_dequantises() {
    // VEC4 UBYTE normalized — `f = c / 255` per spec §3.6.2.2.
    // Bytes [0, 128, 255, 255] decode to [0.0, 128/255, 1.0, 1.0]
    // (a fully-saturated channel + half-grey).
    let out_bytes = vec![0u8, 128, 255, 255, 64, 192, 32, 255];
    let doc = pointer_doc_with_output(
        &out_bytes, 5121, // UNSIGNED_BYTE
        true, "VEC4", 2,
    );
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (ch, out) = pointer_channel_output(&scene);
    assert_eq!(
        ch.get("output_component_type").and_then(|x| x.as_u64()),
        Some(5121)
    );
    assert_eq!(
        ch.get("output_normalized").and_then(|x| x.as_bool()),
        Some(true)
    );
    assert!((out[0] - 0.0).abs() < 1e-6);
    assert!((out[1] - 128.0 / 255.0).abs() < 1e-6);
    assert!((out[2] - 1.0).abs() < 1e-6);
    assert!((out[3] - 1.0).abs() < 1e-6);
    assert!((out[4] - 64.0 / 255.0).abs() < 1e-6);
}

#[test]
fn pointer_output_unsigned_short_normalized_dequantises() {
    // SCALAR USHORT normalized — `f = c / 65535`.
    let mut out_bytes = Vec::new();
    for c in [0u16, 32768, 65535] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5123, true, "SCALAR", 3);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out) = pointer_channel_output(&scene);
    assert!((out[0] - 0.0).abs() < 1e-6);
    assert!((out[1] - 32768.0 / 65535.0).abs() < 1e-6);
    assert!((out[2] - 1.0).abs() < 1e-6);
}

#[test]
fn pointer_output_signed_byte_normalized_dequantises() {
    // VEC2 BYTE normalized — `f = max(c / 127, -1)`. The -128 slot
    // is reserved per spec §3.6.2.2 and clamps to -1.
    let out_bytes = vec![
        127i8 as u8, // +1.0
        0u8,         // 0.0
        (-127i8) as u8,
        (-128i8) as u8, // -128 → max(-128/127, -1) = -1
    ];
    let doc = pointer_doc_with_output(&out_bytes, 5120, true, "VEC2", 2);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out) = pointer_channel_output(&scene);
    assert!((out[0] - 1.0).abs() < 1e-6);
    assert!((out[1] - 0.0).abs() < 1e-6);
    assert!((out[2] - (-1.0)).abs() < 1e-6);
    assert!(
        (out[3] - (-1.0)).abs() < 1e-6,
        "-128 reserved-slot clamps to -1"
    );
}

#[test]
fn pointer_output_signed_short_normalized_dequantises() {
    // SCALAR SHORT normalized — `f = max(c / 32767, -1)`.
    let mut out_bytes = Vec::new();
    for c in [32767i16, 0, -16384, -32768] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5122, true, "SCALAR", 4);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out) = pointer_channel_output(&scene);
    assert!((out[0] - 1.0).abs() < 1e-6);
    assert!((out[1] - 0.0).abs() < 1e-6);
    assert!((out[2] - (-16384.0 / 32767.0)).abs() < 1e-6);
    assert!((out[3] - (-1.0)).abs() < 1e-6);
}

#[test]
fn pointer_output_unsigned_byte_unnormalized_casts_to_float() {
    // Non-normalized integer: spec line 93 "converted to the equal
    // floating-point values, e.g. `1` to `1.0`".
    let out_bytes = vec![0u8, 1, 7, 255];
    let doc = pointer_doc_with_output(&out_bytes, 5121, false, "VEC4", 1);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (ch, out) = pointer_channel_output(&scene);
    assert_eq!(out, vec![0.0, 1.0, 7.0, 255.0]);
    assert_eq!(
        ch.get("output_normalized").and_then(|x| x.as_bool()),
        Some(false),
        "normalized flag round-trips even when false"
    );
}

#[test]
fn pointer_output_signed_short_unnormalized_casts_to_float() {
    // Non-normalized SHORT pours straight to f32 via i16 → f32.
    let mut out_bytes = Vec::new();
    for c in [-32768i16, -1, 0, 32767] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5122, false, "VEC4", 1);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out) = pointer_channel_output(&scene);
    assert_eq!(out, vec![-32768.0, -1.0, 0.0, 32767.0]);
}

#[test]
fn pointer_output_unsigned_int_unnormalized_casts_to_float() {
    // UINT (5125) is non-normalizable per spec §3.6.2.2 (no row);
    // we accept it only with normalized=false.
    let mut out_bytes = Vec::new();
    for c in [0u32, 1, 1_000_000] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5125, false, "SCALAR", 3);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out) = pointer_channel_output(&scene);
    assert_eq!(out, vec![0.0, 1.0, 1_000_000.0]);
}

#[test]
fn pointer_output_unsigned_int_normalized_is_rejected() {
    // Spec §3.6.2.2 doesn't define a normalized-UINT dequantisation
    // equation, so the decoder rejects the combination outright.
    let mut out_bytes = Vec::new();
    for c in [0u32, 1] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5125, true, "SCALAR", 2);
    let err = GltfDecoder::new().decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("UNSIGNED_INT") && msg.contains("normalized"),
        "expected UNSIGNED_INT + normalized rejection, got {msg}"
    );
}

#[test]
fn pointer_output_unsigned_byte_normalized_round_trips_through_glb() {
    // Decode → encode → decode preserves componentType=UBYTE +
    // normalized=true and stays bit-equal (within the half-ulp
    // round-trip error inherent to f = c / 255 → quantize → f').
    let out_bytes = vec![0u8, 128, 255, 64, 192, 32, 16, 200];
    let doc = pointer_doc_with_output(&out_bytes, 5121, true, "VEC4", 2);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out_before) = pointer_channel_output(&scene);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    // The emitted JSON MUST keep componentType=5121 + normalized=true
    // (not silently widen to FLOAT).
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"componentType\":5121"),
        "encoder must re-emit UBYTE componentType, got {raw}"
    );
    assert!(
        raw.contains("\"normalized\":true"),
        "encoder must re-emit normalized=true, got {raw}"
    );

    let scene2 = GltfDecoder::new().decode(&glb).unwrap();
    let (ch2, out_after) = pointer_channel_output(&scene2);
    assert_eq!(
        ch2.get("output_component_type").and_then(|x| x.as_u64()),
        Some(5121)
    );
    assert_eq!(
        ch2.get("output_normalized").and_then(|x| x.as_bool()),
        Some(true)
    );
    // Re-quantising an already-quantised value is idempotent for the
    // normalized-UBYTE codec, so the decoded float stream MUST match
    // exactly (bit-equal).
    assert_eq!(out_before.len(), out_after.len());
    for (a, b) in out_before.iter().zip(out_after.iter()) {
        assert!(
            (a - b).abs() < 1e-7,
            "round-trip drift {a} → {b} exceeds half-ulp"
        );
    }
}

#[test]
fn pointer_output_signed_short_normalized_round_trips_through_glb() {
    let mut out_bytes = Vec::new();
    for c in [32767i16, 0, -16384, -32767] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5122, true, "SCALAR", 4);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let (_, out_before) = pointer_channel_output(&scene);
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_json = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_json).unwrap();
    assert!(
        raw.contains("\"componentType\":5122"),
        "encoder must re-emit SHORT componentType, got {raw}"
    );
    let scene2 = GltfDecoder::new().decode(&glb).unwrap();
    let (_, out_after) = pointer_channel_output(&scene2);
    for (a, b) in out_before.iter().zip(out_after.iter()) {
        assert!((a - b).abs() < 1e-7, "drift {a} → {b}");
    }
}

#[test]
fn pointer_output_unsigned_int_unnormalized_round_trips_through_glb() {
    // SCALAR UINT — non-normalized cast lane; the encoder casts f32
    // back to u32 with NaN → 0 + clamp to u32 range.
    let mut out_bytes = Vec::new();
    for c in [0u32, 42, 65_535, 16_777_216] {
        out_bytes.extend_from_slice(&c.to_le_bytes());
    }
    let doc = pointer_doc_with_output(&out_bytes, 5125, false, "SCALAR", 4);
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_json = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_json).unwrap();
    assert!(
        raw.contains("\"componentType\":5125"),
        "encoder must re-emit UINT componentType, got {raw}"
    );
    let scene2 = GltfDecoder::new().decode(&glb).unwrap();
    let (_, out_after) = pointer_channel_output(&scene2);
    assert_eq!(out_after, vec![0.0, 42.0, 65_535.0, 16_777_216.0]);
}

#[test]
fn legacy_sidecar_without_component_type_keys_defaults_to_float() {
    // Documents authored by r218 (or hand-built sidecars) that omit
    // the new `output_component_type` + `output_normalized` keys MUST
    // continue to round-trip as FLOAT + normalized=false. Build a
    // Scene3D from scratch, drop a sidecar that only carries the
    // r218-era fields, and confirm encode picks FLOAT.
    use oxideav_mesh3d::Scene3D;
    use serde_json::{json, Value};
    let mut scene = Scene3D::new();
    let channel = json!({
        "channel": 0u32,
        "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor",
        "interpolation": "LINEAR",
        "input": [0.0, 1.0],
        "output_kind": "VEC4",
        "output": [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
    });
    let anim_entry = json!({
        "animation": 0u32,
        "channels": [channel],
    });
    let roster = json!({ "animations": [anim_entry] });
    scene.extras.insert("KHR_animation_pointer".into(), roster);
    // The encode loop only walks pointer rosters against extant
    // animations[], so add an empty animation slot for ai=0.
    scene.animations.push(oxideav_mesh3d::Animation::new(None));
    // A pointer-bearing document needs a `materials` slot so the
    // pointer's path can be plausible (the encoder doesn't resolve
    // it; the doc just has to be structurally complete enough to
    // round-trip). The Scene3D::new default already provides one.
    let _ = Value::Null;
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_json = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_json).unwrap();
    // FLOAT (5126) is the default componentType — legacy r218
    // sidecars MUST encode unchanged.
    assert!(
        raw.contains("\"componentType\":5126"),
        "legacy sidecar (no output_component_type) defaults to FLOAT, got {raw}"
    );
    // No `"normalized":true` should appear on the FLOAT output
    // accessor (normalized defaults to false on FLOAT).
    let scene2 = GltfDecoder::new().decode(&glb).unwrap();
    let (ch, out) = pointer_channel_output(&scene2);
    assert_eq!(
        ch.get("output_component_type").and_then(|x| x.as_u64()),
        Some(5126)
    );
    assert_eq!(out, vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
}

// --------------------------------------------------------------------------
// r269: Object-Model pointer-template registry — `bool` data-type lane.
//
// The staged extension specs declare one non-float* Object Model
// property: `/nodes/{}/extensions/KHR_node_visibility/visible` → `bool`
// (KHR_node_visibility.md §"Extending glTF 2.0 Asset Object Model").
// Per KHR_animation_pointer §"Output Accessor Component Types" a
// bool-typed channel's output accessor componentType MUST be unsigned
// byte (`0` → false, any other value → true), the §Operation data-type
// table pins `bool` → SCALAR, and the sampler MUST use STEP
// interpolation.
// --------------------------------------------------------------------------

const VISIBLE_POINTER: &str = "/nodes/0/extensions/KHR_node_visibility/visible";

/// Hand-build a pointer-targeted doc with full control over the
/// pointer string, sampler interpolation, and output accessor shape.
/// `interpolation` of `None` omits the key (spec default LINEAR).
fn pointer_doc_full(
    pointer: &str,
    interpolation: Option<&str>,
    output_bytes: &[u8],
    component_type: u32,
    kind: &str,
    count: u32,
) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    // Input: `count` keyframes at t = 0, 1, 2, ...
    for k in 0..count {
        bin.extend_from_slice(&(k as f32).to_le_bytes());
    }
    let output_offset = bin.len();
    bin.extend_from_slice(output_bytes);
    let total = bin.len();
    let input_len = output_offset;
    let output_len = output_bytes.len();
    let max_t = (count - 1) as f32;
    let uri = format!("data:application/octet-stream;base64,{}", b64(&bin));
    let interp = interpolation
        .map(|s| format!("\"interpolation\": \"{s}\", "))
        .unwrap_or_default();
    format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_animation_pointer"],
            "nodes": [{{}}],
            "buffers": [{{ "uri": "{uri}", "byteLength": {total} }}],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": 0, "byteLength": {input_len} }},
                {{ "buffer": 0, "byteOffset": {output_offset}, "byteLength": {output_len} }}
            ],
            "accessors": [
                {{ "bufferView": 0, "componentType": 5126, "count": {count}, "type": "SCALAR", "min": [0.0], "max": [{max_t}] }},
                {{ "bufferView": 1, "componentType": {component_type}, "count": {count}, "type": "{kind}" }}
            ],
            "animations": [
                {{
                    "channels": [
                        {{
                            "sampler": 0,
                            "target": {{
                                "path": "pointer",
                                "extensions": {{
                                    "KHR_animation_pointer": {{ "pointer": "{pointer}" }}
                                }}
                            }}
                        }}
                    ],
                    "samplers": [
                        {{ "input": 0, {interp}"output": 1 }}
                    ]
                }}
            ]
        }}"#
    )
    .into_bytes()
}

fn pointer_channel_obj(
    scene: &oxideav_mesh3d::Scene3D,
) -> &serde_json::Map<String, serde_json::Value> {
    scene
        .extras
        .get("KHR_animation_pointer")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("animations"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|e| e.get("channels"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_object())
        .expect("pointer channel present in side-channel roster")
}

#[test]
fn bool_pointer_output_decodes_to_booleans() {
    // UBYTE SCALAR STEP, bytes [0, 1, 7] — `0` → false, any other
    // value → true per §"Output Accessor Component Types".
    let doc = pointer_doc_full(
        VISIBLE_POINTER,
        Some("STEP"),
        &[0u8, 1, 7],
        5121,
        "SCALAR",
        3,
    );
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let ch = pointer_channel_obj(&scene);
    assert_eq!(
        ch.get("output_data_type").and_then(|x| x.as_str()),
        Some("bool"),
        "registry hit recorded in the sidecar"
    );
    assert_eq!(
        ch.get("output_component_type").and_then(|x| x.as_u64()),
        Some(5121)
    );
    let out: Vec<bool> = ch
        .get("output")
        .and_then(|a| a.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_bool().expect("bool lane surfaces JSON booleans"))
        .collect();
    assert_eq!(out, vec![false, true, true]);
}

#[test]
fn bool_pointer_with_float_output_is_rejected() {
    // componentType MUST be unsigned byte for a bool-typed property.
    let mut out_bytes = Vec::new();
    for f in [0.0f32, 1.0] {
        out_bytes.extend_from_slice(&f.to_le_bytes());
    }
    let doc = pointer_doc_full(VISIBLE_POINTER, Some("STEP"), &out_bytes, 5126, "SCALAR", 2);
    let err = GltfDecoder::new().decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerBoolComponentType"),
        "expected ExtensionStackAnimationPointerBoolComponentType, got {msg}"
    );
}

#[test]
fn bool_pointer_with_non_scalar_output_is_rejected() {
    // §Operation data-type table: `bool` → SCALAR.
    let doc = pointer_doc_full(
        VISIBLE_POINTER,
        Some("STEP"),
        &[0u8, 1, 1, 0],
        5121,
        "VEC2",
        2,
    );
    let err = GltfDecoder::new().decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerBoolType"),
        "expected ExtensionStackAnimationPointerBoolType, got {msg}"
    );
}

#[test]
fn bool_pointer_with_linear_interpolation_is_rejected() {
    // "Animation samplers used with `int` or `bool` Object Model Data
    // Types MUST use STEP interpolation."
    let doc = pointer_doc_full(
        VISIBLE_POINTER,
        Some("LINEAR"),
        &[0u8, 1],
        5121,
        "SCALAR",
        2,
    );
    let err = GltfDecoder::new().decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerBoolInterpolation"),
        "expected ExtensionStackAnimationPointerBoolInterpolation, got {msg}"
    );
}

#[test]
fn bool_pointer_with_default_interpolation_is_rejected() {
    // An absent `interpolation` key defaults to LINEAR (spec §3.11),
    // which the bool lane MUST refuse just like an explicit LINEAR.
    let doc = pointer_doc_full(VISIBLE_POINTER, None, &[0u8, 1], 5121, "SCALAR", 2);
    let err = GltfDecoder::new().decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackAnimationPointerBoolInterpolation"),
        "expected ExtensionStackAnimationPointerBoolInterpolation, got {msg}"
    );
}

#[test]
fn bool_pointer_round_trips_through_glb() {
    // Decode → encode → decode. The emitted JSON keeps the mandatory
    // unsigned-byte componentType + STEP interpolation; truthy source
    // bytes (7) canonicalise to 1 but stay `true`.
    let doc = pointer_doc_full(
        VISIBLE_POINTER,
        Some("STEP"),
        &[0u8, 7, 1],
        5121,
        "SCALAR",
        3,
    );
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"componentType\":5121"),
        "encoder must emit UBYTE componentType on the bool lane, got {raw}"
    );
    assert!(
        raw.contains("\"interpolation\":\"STEP\""),
        "encoder must emit STEP interpolation on the bool lane, got {raw}"
    );
    let scene2 = GltfDecoder::new().decode(&glb).unwrap();
    let ch = pointer_channel_obj(&scene2);
    assert_eq!(
        ch.get("output_data_type").and_then(|x| x.as_str()),
        Some("bool")
    );
    let out: Vec<bool> = ch
        .get("output")
        .and_then(|a| a.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    assert_eq!(out, vec![false, true, true]);
}

#[test]
fn unregistered_pointer_stays_on_float_lane() {
    // A pointer with no registry row (here a core material factor)
    // keeps the float* branch: numeric sidecar output and no
    // `output_data_type` key, even with a UBYTE STEP sampler.
    let doc = pointer_doc_full(
        "/materials/0/pbrMetallicRoughness/metallicFactor",
        Some("STEP"),
        &[0u8, 1, 7],
        5121,
        "SCALAR",
        3,
    );
    let scene = GltfDecoder::new().decode(&doc).unwrap();
    let ch = pointer_channel_obj(&scene);
    assert!(
        ch.get("output_data_type").is_none(),
        "float* lane omits the output_data_type key"
    );
    let out: Vec<f64> = ch
        .get("output")
        .and_then(|a| a.as_array())
        .unwrap()
        .iter()
        .map(|v| v.as_f64().expect("float lane surfaces JSON numbers"))
        .collect();
    assert_eq!(out, vec![0.0, 1.0, 7.0]);
}

#[test]
fn bool_sidecar_with_linear_interpolation_refuses_to_encode() {
    // A hand-authored sidecar that pairs the bool lane with a LINEAR
    // sampler violates the STEP MUST and is refused at encode time.
    use oxideav_mesh3d::Scene3D;
    use serde_json::json;
    let mut scene = Scene3D::new();
    scene.extras.insert(
        "KHR_animation_pointer".into(),
        json!({
            "animations": [{
                "animation": 0u32,
                "channels": [{
                    "channel": 0u32,
                    "pointer": VISIBLE_POINTER,
                    "interpolation": "LINEAR",
                    "input": [0.0, 1.0],
                    "output_kind": "SCALAR",
                    "output_data_type": "bool",
                    "output": [true, false],
                }],
            }],
        }),
    );
    scene.animations.push(oxideav_mesh3d::Animation::new(None));
    let err = GltfEncoder::new().encode(&scene).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("STEP"),
        "expected the STEP-interpolation MUST to surface, got {msg}"
    );
}
