//! `KHR_meshopt_compression` extension — per-bufferView compression
//! descriptors + per-buffer `{ "fallback": true }` placeholder
//! markers per
//! `docs/3d/gltf/extensions/KHR_meshopt_compression.md`.
//!
//! The meshopt Appendix A (Bitstream) + Appendix B (Filters) decoder
//! is implemented in `meshopt.rs`; the extension is handled at both the
//! JSON descriptor level and the bitstream level:
//!
//! * **Decode**: every bufferView carrying
//!   `extensions.KHR_meshopt_compression` is captured under
//!   `Scene3D::extras["KHR_meshopt_compression"].bufferViews["<bvi>"]`
//!   as the full extension JSON object, AND its compressed source range
//!   is inflated into the parent bufferView's region so the standard
//!   accessor pipeline reads the real attribute / index data. Every
//!   buffer marked with `extensions.KHR_meshopt_compression.fallback =
//!   true` is captured under `…fallbackBuffers` as an array of buffer
//!   indices. A fallback buffer that has no URI and is not the GLB
//!   binary chunk is materialised as a zero-filled byte vector of the
//!   declared `byteLength` and then overwritten by the inflated bytes.
//!
//! * **Encode**: the encoder builds fresh bufferViews against an
//!   uncompressed packed BIN, so the descriptors are NOT round-tripped
//!   into the emitted document. The sidecar is stripped from
//!   `scene.extras` so the written `extras` field is clean. Documents
//!   round-tripped through this crate are always uncompressed
//!   (the compression is a load-time concern only).
//!
//! * **Validation** (§3.12 + §"JSON schema updates" + §"Fallback
//!   buffers"): all of the following are rejected by `validate_root`
//!   with stable `ExtensionStack…` error prefixes:
//!
//!   * data block on any bufferView/buffer without
//!     `KHR_meshopt_compression` in `extensionsUsed`
//!     (`ExtensionStackUsedNotDeclared`)
//!   * uri-less fallback buffer without `KHR_meshopt_compression` in
//!     `extensionsRequired` (`ExtensionStackMeshoptRequired`)
//!   * `mode` not in `{ATTRIBUTES, TRIANGLES, INDICES}`
//!     (`ExtensionStackMeshoptMode`)
//!   * `filter` not in `{NONE, OCTAHEDRAL, QUATERNION, EXPONENTIAL,
//!     COLOR}` (`ExtensionStackMeshoptFilter`)
//!   * `parent.byteLength != byteStride * count`
//!     (`ExtensionStackMeshoptLayout`)
//!   * per-mode byteStride invariants
//!     (`ExtensionStackMeshoptStride`)
//!   * per-mode count invariant (TRIANGLES) — count divisible by 3
//!     (`ExtensionStackMeshoptCount`)
//!   * filter constraints on TRIANGLES / INDICES — filter must be
//!     `"NONE"` or omitted (`ExtensionStackMeshoptFilter`)
//!   * filter-specific byteStride constraints
//!     (`ExtensionStackMeshoptFilter`)
//!   * `extension.buffer` out of range
//!     (`ExtensionStackMeshoptBuffer`)
//!   * extension compressed range overruns source buffer
//!     (`ExtensionStackMeshoptRange`)
//!   * fallback buffer referenced by a bufferView WITHOUT the
//!     extension (`ExtensionStackMeshoptFallbackRef`)
//!   * extension's own `buffer` is itself a fallback buffer
//!     (`ExtensionStackMeshoptFallbackSource`)

use base64::Engine;
use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder, Scene3D};
use serde_json::Value;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A valid v0 ATTRIBUTES stream of `count` elements, `byte_stride`-wide,
/// in which every byte delta is zero — so every decoded element equals
/// the `baseline` carried in the tail block. Layout per
/// `docs/3d/gltf/extensions/KHR_meshopt_compression.md` §"Mode 0":
/// header `0xa0`, then for each byte position one header byte selecting
/// delta-encoding-mode 0 (all-zero, no data bytes) for the single group,
/// then the `byte_stride`-byte baseline tail.
fn meshopt_attr_v0_constant(count: usize, byte_stride: usize, baseline: &[u8]) -> Vec<u8> {
    assert_eq!(baseline.len(), byte_stride);
    let group_count = count.div_ceil(16);
    let header_bytes = group_count.div_ceil(4);
    let mut s = vec![0xa0u8];
    for _ in 0..byte_stride {
        // header bits: all groups hb=0 → v0 delta-mode 0 (all-zero).
        s.extend(std::iter::repeat(0u8).take(header_bytes));
    }
    s.extend_from_slice(baseline);
    s
}

/// Build a tiny meshopt-tagged glTF JSON document. `bufferView[0]`
/// decompresses to `count` elements of `byte_stride` bytes; the source
/// buffer (index 1) carries the supplied `compressed` payload. Buffer 0
/// is the GLB-style fallback (`fallback: true`, no URI).
fn meshopt_doc_with_payload(
    mode: &str,
    filter: Option<&str>,
    count: usize,
    byte_stride: usize,
    compressed: &[u8],
) -> Vec<u8> {
    let filter_field = match filter {
        Some(f) => format!(r#", "filter": "{f}""#),
        None => String::new(),
    };
    let uncompressed_len = count * byte_stride;
    let comp_len = compressed.len();
    let comp_b64 = b64(compressed);
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_meshopt_compression"],
            "extensionsRequired": ["KHR_meshopt_compression"],
            "buffers": [
                {{
                    "byteLength": {uncompressed_len},
                    "extensions": {{
                        "KHR_meshopt_compression": {{ "fallback": true }}
                    }}
                }},
                {{
                    "uri": "data:application/octet-stream;base64,{comp_b64}",
                    "byteLength": {comp_len}
                }}
            ],
            "bufferViews": [
                {{
                    "buffer": 0,
                    "byteOffset": 0,
                    "byteLength": {uncompressed_len},
                    "byteStride": {byte_stride},
                    "extensions": {{
                        "KHR_meshopt_compression": {{
                            "buffer": 1,
                            "byteOffset": 0,
                            "byteLength": {comp_len},
                            "byteStride": {byte_stride},
                            "count": {count},
                            "mode": "{mode}"{filter_field}
                        }}
                    }}
                }}
            ]
        }}"#
    );
    json.into_bytes()
}

/// Convenience: the legacy "ATTRIBUTES, byteStride 12, count 4" doc with
/// a valid all-zero v0 payload (baseline = ascending bytes).
fn meshopt_doc_with_fallback(mode: &str, filter: Option<&str>) -> Vec<u8> {
    let baseline: Vec<u8> = (0u8..12).collect();
    let comp = meshopt_attr_v0_constant(4, 12, &baseline);
    meshopt_doc_with_payload(mode, filter, 4, 12, &comp)
}

// --- decode + side-channel capture ----------------------------------

#[test]
fn meshopt_descriptor_lifts_into_extras() {
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(&meshopt_doc_with_fallback("ATTRIBUTES", None))
        .expect("doc must decode");
    let v = scene
        .extras
        .get("KHR_meshopt_compression")
        .expect("sidecar lifted into Scene3D::extras");
    let obj = v.as_object().expect("sidecar is JSON object");
    let bvs = obj
        .get("bufferViews")
        .and_then(|x| x.as_object())
        .expect("bufferViews map present");
    let descriptor = bvs.get("0").expect("bufferView[0] descriptor present");
    assert_eq!(descriptor["buffer"].as_u64(), Some(1));
    assert_eq!(descriptor["byteStride"].as_u64(), Some(12));
    assert_eq!(descriptor["count"].as_u64(), Some(4));
    assert_eq!(descriptor["mode"].as_str(), Some("ATTRIBUTES"));
    let fbs = obj
        .get("fallbackBuffers")
        .and_then(|x| x.as_array())
        .expect("fallbackBuffers array present");
    assert_eq!(fbs.len(), 1);
    assert_eq!(fbs[0].as_u64(), Some(0));
}

#[test]
fn meshopt_descriptor_carries_filter_when_present() {
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(&meshopt_doc_with_fallback("ATTRIBUTES", Some("OCTAHEDRAL")))
        .expect_err("OCTAHEDRAL with byteStride 12 must be rejected");
    let _ = scene;
}

#[test]
fn meshopt_descriptor_carries_octahedral_filter_with_valid_stride() {
    // byteStride 8 (which OCTAHEDRAL allows): four i16 components per
    // element. Baseline encodes one valid octahedral vector: x=0, y=0,
    // input[2]=32767 (K=16 representation of 1.0), w=0. All-zero deltas
    // replicate it across all 4 elements. Per §"Filter 1: octahedral"
    // this decodes to the +Z unit vector (0, 0, 32767).
    let mut baseline = Vec::new();
    baseline.extend_from_slice(&0i16.to_le_bytes()); // x
    baseline.extend_from_slice(&0i16.to_le_bytes()); // y
    baseline.extend_from_slice(&32767i16.to_le_bytes()); // input[2] = 1.0
    baseline.extend_from_slice(&0i16.to_le_bytes()); // w (passthrough)
    let comp = meshopt_attr_v0_constant(4, 8, &baseline);
    let doc = meshopt_doc_with_payload("ATTRIBUTES", Some("OCTAHEDRAL"), 4, 8, &comp);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("OCTAHEDRAL byteStride=8 is valid");
    let v = scene.extras.get("KHR_meshopt_compression").unwrap();
    let bv0 = &v["bufferViews"]["0"];
    assert_eq!(bv0["filter"].as_str(), Some("OCTAHEDRAL"));
}

#[test]
fn fallback_buffer_materialises_as_zero_padding() {
    // Document parses without panic even though the fallback buffer has
    // no URI and no actual bytes. The bufferView's 48-byte declared
    // range is backed by the zero-filled placeholder we synthesize.
    let mut dec = GltfDecoder::new();
    let _scene = dec
        .decode(&meshopt_doc_with_fallback("ATTRIBUTES", None))
        .expect("uri-less fallback buffer must be materialised by decode");
}

// --- end-to-end bitstream inflation (Appendix A / B) ----------------

#[test]
fn attributes_inflate_feeds_position_accessor() {
    // A POSITION VEC3 FLOAT accessor (byteStride 12, count 3) backed by
    // an ATTRIBUTES-compressed bufferView. The v0 stream carries
    // all-zero deltas and a baseline of three f32s, so every vertex
    // decodes to the same position. Per
    // `docs/3d/gltf/extensions/KHR_meshopt_compression.md` §"Mode 0"
    // the inflated bytes flow through the normal accessor pipeline.
    let mut baseline = Vec::new();
    baseline.extend_from_slice(&1.5f32.to_le_bytes());
    baseline.extend_from_slice(&(-2.0f32).to_le_bytes());
    baseline.extend_from_slice(&3.25f32.to_le_bytes());
    let comp = meshopt_attr_v0_constant(3, 12, &baseline);
    let comp_b64 = b64(&comp);
    let comp_len = comp.len();
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_meshopt_compression"],
            "extensionsRequired": ["KHR_meshopt_compression"],
            "buffers": [
                {{ "byteLength": 36, "extensions": {{ "KHR_meshopt_compression": {{ "fallback": true }} }} }},
                {{ "uri": "data:application/octet-stream;base64,{comp_b64}", "byteLength": {comp_len} }}
            ],
            "bufferViews": [
                {{
                    "buffer": 0, "byteOffset": 0, "byteLength": 36, "byteStride": 12,
                    "extensions": {{ "KHR_meshopt_compression": {{
                        "buffer": 1, "byteOffset": 0, "byteLength": {comp_len},
                        "byteStride": 12, "count": 3, "mode": "ATTRIBUTES" }} }}
                }}
            ],
            "accessors": [
                {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }}
            ],
            "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
            "nodes": [ {{ "mesh": 0 }} ],
            "scenes": [ {{ "nodes": [0] }} ],
            "scene": 0
        }}"#
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).expect("inflate + decode");
    let p = &scene.meshes[0].primitives[0];
    assert_eq!(p.positions.len(), 3);
    for v in &p.positions {
        assert_eq!(*v, [1.5, -2.0, 3.25]);
    }
}

#[test]
fn indices_inflate_feeds_index_accessor() {
    // A SCALAR UNSIGNED_SHORT index accessor backed by an
    // INDICES-compressed bufferView producing [0, 1, 2] (a single
    // triangle). Per §"Mode 2: indices": header 0xd1, three varint
    // deltas (baseline bit 0, +0 / +1 / +1), 4-byte tail padding.
    let mut comp = vec![0xd1u8];
    comp.push(0x00); // index 0
    comp.push(0x04); // +1 → 1
    comp.push(0x04); // +1 → 2
    comp.extend_from_slice(&[0, 0, 0, 0]); // tail padding
    let comp_b64 = b64(&comp);
    let comp_len = comp.len();
    // POSITION needs at least one valid accessor for a renderable
    // primitive; reuse a trivial inline buffer for positions.
    let pos: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let pos_b64 = b64(&pos);
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_meshopt_compression"],
            "extensionsRequired": ["KHR_meshopt_compression"],
            "buffers": [
                {{ "byteLength": 6, "extensions": {{ "KHR_meshopt_compression": {{ "fallback": true }} }} }},
                {{ "uri": "data:application/octet-stream;base64,{comp_b64}", "byteLength": {comp_len} }},
                {{ "uri": "data:application/octet-stream;base64,{pos_b64}", "byteLength": 36 }}
            ],
            "bufferViews": [
                {{
                    "buffer": 0, "byteOffset": 0, "byteLength": 6,
                    "extensions": {{ "KHR_meshopt_compression": {{
                        "buffer": 1, "byteOffset": 0, "byteLength": {comp_len},
                        "byteStride": 2, "count": 3, "mode": "INDICES" }} }}
                }},
                {{ "buffer": 2, "byteOffset": 0, "byteLength": 36 }}
            ],
            "accessors": [
                {{ "bufferView": 0, "componentType": 5123, "count": 3, "type": "SCALAR" }},
                {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }}
            ],
            "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 1 }}, "indices": 0 }} ] }} ],
            "nodes": [ {{ "mesh": 0 }} ],
            "scenes": [ {{ "nodes": [0] }} ],
            "scene": 0
        }}"#
    );
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(json.as_bytes())
        .expect("inflate INDICES + decode");
    let p = &scene.meshes[0].primitives[0];
    let tris = p.triangle_indices();
    assert_eq!(tris, vec![[0, 1, 2]]);
}

// --- encode strips sidecar (pass-through) ---------------------------

#[test]
fn round_trip_drops_descriptor_sidecar_from_extras() {
    // Build a scene manually with the sidecar populated, then encode.
    // We expect the emitted JSON's scene.extras to NOT carry the
    // sidecar — the encoder always writes uncompressed.
    let mut scene = Scene3D::new();
    let mut top = serde_json::Map::new();
    let mut bvs = serde_json::Map::new();
    bvs.insert(
        "0".to_owned(),
        serde_json::json!({
            "buffer": 1,
            "byteLength": 24,
            "byteStride": 12,
            "count": 4,
            "mode": "ATTRIBUTES"
        }),
    );
    top.insert("bufferViews".to_owned(), Value::Object(bvs));
    top.insert(
        "fallbackBuffers".to_owned(),
        Value::Array(vec![Value::from(0u32)]),
    );
    scene
        .extras
        .insert("KHR_meshopt_compression".to_owned(), Value::Object(top));
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).expect("encode succeeds");
    let json_payload = extract_json_chunk(&glb);
    let v: Value = serde_json::from_slice(&json_payload).unwrap();
    let scenes = v["scenes"].as_array().unwrap();
    assert!(
        scenes[0]
            .get("extras")
            .and_then(|x| x.get("KHR_meshopt_compression"))
            .is_none(),
        "sidecar must be stripped on encode"
    );
    // Document does NOT declare the extension on encode either, since
    // the encoded data is uncompressed.
    let used = v.get("extensionsUsed").and_then(|x| x.as_array());
    if let Some(used) = used {
        assert!(
            !used.iter().any(|s| s == "KHR_meshopt_compression"),
            "encoder must not declare the extension when emitting uncompressed bufferViews"
        );
    }
}

// --- §3.12 stack rejection ------------------------------------------

#[test]
fn rejects_data_block_without_extensions_used_declaration() {
    // Same doc but drop both arrays.
    let json = r#"{
        "asset": { "version": "2.0" },
        "buffers": [
            { "uri": "data:application/octet-stream;base64,AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4v", "byteLength": 48 },
            { "uri": "data:application/octet-stream;base64,AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGw==", "byteLength": 28 }
        ],
        "bufferViews": [
            {
                "buffer": 0, "byteOffset": 0, "byteLength": 48, "byteStride": 12,
                "extensions": {
                    "KHR_meshopt_compression": {
                        "buffer": 1, "byteOffset": 0, "byteLength": 24,
                        "byteStride": 12, "count": 4, "mode": "ATTRIBUTES"
                    }
                }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(json.as_bytes())
        .expect_err("data block without extensionsUsed must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_meshopt_compression"),
        "{msg}"
    );
}

#[test]
fn rejects_uriless_fallback_buffer_without_extensions_required() {
    // Doc declares used but not required, and the fallback buffer has
    // no URI — spec §"Fallback buffers" mandates required.
    let json = r#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_meshopt_compression"],
        "buffers": [
            { "byteLength": 48, "extensions": { "KHR_meshopt_compression": { "fallback": true } } },
            { "uri": "data:application/octet-stream;base64,AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGw==", "byteLength": 28 }
        ],
        "bufferViews": [
            {
                "buffer": 0, "byteOffset": 0, "byteLength": 48, "byteStride": 12,
                "extensions": {
                    "KHR_meshopt_compression": {
                        "buffer": 1, "byteOffset": 0, "byteLength": 24,
                        "byteStride": 12, "count": 4, "mode": "ATTRIBUTES"
                    }
                }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(json.as_bytes())
        .expect_err("uri-less fallback without extensionsRequired must reject");
    let msg = format!("{err}");
    assert!(msg.contains("ExtensionStackMeshoptRequired"), "{msg}");
}

// --- per-rule invariant rejection -----------------------------------

fn doc_with_descriptor(descriptor: &str, parent_byte_length: u32, parent_stride: u32) -> Vec<u8> {
    // Plain (no fallback) variant — the source buffer has real bytes
    // and the parent bufferView lives in a non-fallback buffer.
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_meshopt_compression"],
            "buffers": [
                {{ "uri": "data:application/octet-stream;base64,AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4v", "byteLength": 48 }},
                {{ "uri": "data:application/octet-stream;base64,AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGw==", "byteLength": 28 }}
            ],
            "bufferViews": [
                {{
                    "buffer": 0, "byteOffset": 0, "byteLength": {parent_byte_length}, "byteStride": {parent_stride},
                    "extensions": {{ "KHR_meshopt_compression": {descriptor} }}
                }}
            ]
        }}"#
    );
    json.into_bytes()
}

#[test]
fn rejects_unknown_mode() {
    let desc =
        r#"{ "buffer": 1, "byteLength": 16, "byteStride": 4, "count": 4, "mode": "SOMETHING" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 16, 4))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptMode"));
}

#[test]
fn rejects_unknown_filter() {
    let desc = r#"{ "buffer": 1, "byteLength": 16, "byteStride": 4, "count": 4, "mode": "ATTRIBUTES", "filter": "RAINBOW" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 16, 4))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptFilter"));
}

#[test]
fn rejects_parent_layout_mismatch() {
    // byteStride * count = 4 * 4 = 16, parent claims 32.
    let desc =
        r#"{ "buffer": 1, "byteLength": 16, "byteStride": 4, "count": 4, "mode": "ATTRIBUTES" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 32, 4))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptLayout"));
}

#[test]
fn rejects_attributes_stride_too_small() {
    // ATTRIBUTES requires byteStride divisible by 4 and >= 4.
    let desc =
        r#"{ "buffer": 1, "byteLength": 8, "byteStride": 2, "count": 4, "mode": "ATTRIBUTES" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 8, 2))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptStride"));
}

#[test]
fn rejects_attributes_stride_too_big() {
    // ATTRIBUTES requires byteStride <= 256.
    let desc = r#"{ "buffer": 1, "byteLength": 1024, "byteStride": 260, "count": 4, "mode": "ATTRIBUTES" }"#;
    // parent.byteLength matches descriptor.byteStride * count to isolate the stride check.
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 1040, 260))
        .expect_err("rejects");
    // The parent_byte_length 1040 != 260*4 = 1040 actually matches; but byteStride 260 fails first.
    assert!(
        format!("{err}").contains("ExtensionStackMeshoptStride")
            || format!("{err}").contains("ExtensionStackMeshoptLayout")
    );
}

#[test]
fn rejects_triangles_count_not_divisible_by_three() {
    let desc =
        r#"{ "buffer": 1, "byteLength": 8, "byteStride": 2, "count": 4, "mode": "TRIANGLES" }"#;
    // 2 * 4 = 8 matches parent.
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 8, 2))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptCount"));
}

#[test]
fn rejects_triangles_with_non_none_filter() {
    let desc = r#"{ "buffer": 1, "byteLength": 12, "byteStride": 2, "count": 6, "mode": "TRIANGLES", "filter": "EXPONENTIAL" }"#;
    // 2 * 6 = 12.
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 12, 2))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptFilter"));
}

#[test]
fn rejects_indices_bad_stride() {
    let desc =
        r#"{ "buffer": 1, "byteLength": 6, "byteStride": 6, "count": 1, "mode": "INDICES" }"#;
    // 6 * 1 = 6.
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 6, 6))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptStride"));
}

#[test]
fn rejects_quaternion_with_wrong_stride() {
    // QUATERNION requires byteStride == 8.
    let desc = r#"{ "buffer": 1, "byteLength": 16, "byteStride": 4, "count": 4, "mode": "ATTRIBUTES", "filter": "QUATERNION" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 16, 4))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptFilter"));
}

#[test]
fn rejects_exponential_with_non_multiple_of_four_stride() {
    let desc = r#"{ "buffer": 1, "byteLength": 18, "byteStride": 6, "count": 3, "mode": "ATTRIBUTES", "filter": "EXPONENTIAL" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 18, 6))
        .expect_err("rejects");
    // Either stride-check (ATTRIBUTES needs %4==0) or filter check fires.
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackMeshoptStride") || msg.contains("ExtensionStackMeshoptFilter")
    );
}

#[test]
fn rejects_color_with_invalid_stride() {
    // COLOR requires byteStride ∈ {4, 8}.
    let desc = r#"{ "buffer": 1, "byteLength": 48, "byteStride": 12, "count": 4, "mode": "ATTRIBUTES", "filter": "COLOR" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 48, 12))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptFilter"));
}

#[test]
fn rejects_buffer_index_out_of_range() {
    let desc =
        r#"{ "buffer": 99, "byteLength": 16, "byteStride": 4, "count": 4, "mode": "ATTRIBUTES" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 16, 4))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptBuffer"));
}

#[test]
fn rejects_range_overrun_in_source_buffer() {
    // Source buffer is 28 bytes; descriptor claims byteLength 256.
    let desc = r#"{ "buffer": 1, "byteOffset": 8, "byteLength": 256, "byteStride": 4, "count": 64, "mode": "ATTRIBUTES" }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(&doc_with_descriptor(desc, 256, 4))
        .expect_err("rejects");
    assert!(format!("{err}").contains("ExtensionStackMeshoptRange"));
}

#[test]
fn rejects_fallback_buffer_referenced_without_extension() {
    // bufferView[1] points at the fallback buffer (0) but doesn't carry
    // KHR_meshopt_compression — spec §"Fallback buffers" forbids that.
    let json = r#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_meshopt_compression"],
        "extensionsRequired": ["KHR_meshopt_compression"],
        "buffers": [
            { "byteLength": 96, "extensions": { "KHR_meshopt_compression": { "fallback": true } } },
            { "uri": "data:application/octet-stream;base64,AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGw==", "byteLength": 28 }
        ],
        "bufferViews": [
            {
                "buffer": 0, "byteOffset": 0, "byteLength": 48, "byteStride": 12,
                "extensions": {
                    "KHR_meshopt_compression": {
                        "buffer": 1, "byteOffset": 0, "byteLength": 24,
                        "byteStride": 12, "count": 4, "mode": "ATTRIBUTES"
                    }
                }
            },
            { "buffer": 0, "byteOffset": 48, "byteLength": 48 }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(json.as_bytes())
        .expect_err("plain bufferView on fallback must reject");
    assert!(format!("{err}").contains("ExtensionStackMeshoptFallbackRef"));
}

#[test]
fn rejects_descriptor_buffer_pointing_at_fallback() {
    // The extension's `buffer` MUST NOT be a fallback buffer.
    let json = r#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_meshopt_compression"],
        "extensionsRequired": ["KHR_meshopt_compression"],
        "buffers": [
            { "byteLength": 48, "extensions": { "KHR_meshopt_compression": { "fallback": true } } },
            { "byteLength": 48, "extensions": { "KHR_meshopt_compression": { "fallback": true } } }
        ],
        "bufferViews": [
            {
                "buffer": 0, "byteOffset": 0, "byteLength": 48, "byteStride": 12,
                "extensions": {
                    "KHR_meshopt_compression": {
                        "buffer": 1, "byteOffset": 0, "byteLength": 24,
                        "byteStride": 12, "count": 4, "mode": "ATTRIBUTES"
                    }
                }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec
        .decode(json.as_bytes())
        .expect_err("extension.buffer == fallback must reject");
    assert!(format!("{err}").contains("ExtensionStackMeshoptFallbackSource"));
}

// --- bare doc without the extension stays unaffected ----------------

#[test]
fn doc_without_extension_does_not_grow_sidecar() {
    // Smallest sane doc — empty asset block only.
    let json = r#"{
        "asset": { "version": "2.0" },
        "scenes": [ { "nodes": [] } ]
    }"#;
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).expect("plain doc decodes");
    assert!(
        !scene.extras.contains_key("KHR_meshopt_compression"),
        "no descriptor → no sidecar"
    );
}

fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}
