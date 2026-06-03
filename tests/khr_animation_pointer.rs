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
