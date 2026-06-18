//! End-to-end decode validation for `KHR_mesh_quantization` per
//! `docs/3d/gltf/extensions/KHR_mesh_quantization.md`.
//!
//! The extension widens the allowed vertex-attribute component types
//! from `FLOAT` to 8-/16-bit signed/unsigned integers. When the
//! accessor is `normalized`, the decoder dequantizes via the spec's
//! int-to-float table; otherwise integers are cast directly to `f32`.
//! These tests construct quantized `.gltf` documents and assert the
//! decoded `Primitive` floats match the spec equations bit-for-bit
//! (within f32 rounding).

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a minimal `.gltf` document around the supplied JSON fragments.
/// `extensions_used_json` is the `extensionsUsed` array body
/// (e.g. `"KHR_mesh_quantization"`), or empty for none.
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
            "mode": 0,
            "attributes": {attributes_json}
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

#[test]
fn decodes_short_normalized_position() {
    // POSITION VEC3 SHORT normalized: f = max(c / 32767.0, -1.0).
    // 4 vertices, 6 bytes each (3 * i16), 4-byte aligned → stride 8.
    // Use stride=8 buffer view (last 2 bytes padding per vertex).
    let verts: [[i16; 3]; 4] = [
        [32767, 0, -32767],   // -> [1.0, 0.0, -1.0]
        [-32768, 16384, 0],   // -> [-1.0 (clamped), 0.5000..., 0.0]
        [0, 0, 0],            // -> [0.0, 0.0, 0.0]
        [16383, -16384, 100], // -> [~0.5, ~-0.5, ~0.003]
    ];
    let mut bin = Vec::new();
    for v in verts {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        bin.extend_from_slice(&[0u8, 0]); // pad to stride 8
    }
    let total = bin.len();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5122, "count": 4, "type": "VEC3", "normalized": true}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}, "byteStride": 8}}"#),
        r#"{"POSITION": 0}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("quantized POSITION must decode");
    let pos = &scene.meshes[0].primitives[0].positions;
    assert_eq!(pos.len(), 4);
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(near(pos[0][0], 1.0), "{:?}", pos[0]);
    assert!(near(pos[0][2], -1.0), "{:?}", pos[0]);
    // -32768 / 32767 = -1.0078 → clamped to -1.0 by max(.., -1.0).
    assert!(near(pos[1][0], -1.0), "{:?}", pos[1]);
    assert!(near(pos[1][1], 16384.0 / 32767.0), "{:?}", pos[1]);
    assert!(near(pos[2][0], 0.0), "{:?}", pos[2]);
    assert!(near(pos[3][2], 100.0 / 32767.0), "{:?}", pos[3]);
}

#[test]
fn decodes_byte_normalized_normal() {
    // NORMAL VEC3 BYTE normalized: f = max(c / 127.0, -1.0).
    // 3 bytes per element, padded to stride 4.
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
        bin.push(0); // pad to stride 4
    }
    let total = bin.len();
    let nrm_len = total - nrm_off;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 0.0, 0.0]},
              {"bufferView": 1, "componentType": 5120, "count": 2, "type": "VEC3", "normalized": true}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {nrm_off}}},
              {{"buffer": 0, "byteOffset": {nrm_off}, "byteLength": {nrm_len}, "byteStride": 4}}"#
        ),
        r#"{"POSITION": 0, "NORMAL": 1}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("quantized NORMAL must decode");
    let nrm = scene.meshes[0].primitives[0].normals.as_ref().unwrap();
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(near(nrm[0][1], 1.0), "{:?}", nrm[0]); // 127/127
    assert!(near(nrm[1][0], -1.0), "{:?}", nrm[1]); // -127/127
}

#[test]
fn decodes_unnormalized_short_texcoord() {
    // TEXCOORD_0 VEC2 SHORT (NOT normalized): cast directly to f32.
    // Spec: "unnormalized integer 2 corresponds to 2.0 in UV space."
    let positions: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let uv_off = bin.len();
    // VEC2 SHORT = 4 bytes, already 4-aligned (no pad needed).
    let uvs: [[i16; 2]; 2] = [[2, 5], [-3, 1000]];
    for uv in uvs {
        for c in uv {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let uv_len = total - uv_off;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 0.0, 0.0]},
              {"bufferView": 1, "componentType": 5122, "count": 2, "type": "VEC2"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {uv_off}}},
              {{"buffer": 0, "byteOffset": {uv_off}, "byteLength": {uv_len}}}"#
        ),
        r#"{"POSITION": 0, "TEXCOORD_0": 1}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("unnormalized TEXCOORD must decode");
    let uv = &scene.meshes[0].primitives[0].uvs[0];
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(near(uv[0][0], 2.0) && near(uv[0][1], 5.0), "{:?}", uv[0]);
    assert!(
        near(uv[1][0], -3.0) && near(uv[1][1], 1000.0),
        "{:?}",
        uv[1]
    );
}

#[test]
fn base_spec_normalized_texcoord_without_extension() {
    // UNSIGNED_BYTE normalized TEXCOORD is allowed by the base spec
    // §3.7.2.1 *without* the extension. f = c / 255.0.
    let positions: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let uv_off = bin.len();
    // VEC2 UBYTE = 2 bytes, padded to stride 4.
    let uvs: [[u8; 2]; 2] = [[255, 0], [128, 64]];
    for uv in uvs {
        bin.extend_from_slice(&uv);
        bin.extend_from_slice(&[0u8, 0]);
    }
    let total = bin.len();
    let uv_len = total - uv_off;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 0.0, 0.0]},
              {"bufferView": 1, "componentType": 5121, "count": 2, "type": "VEC2", "normalized": true}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {uv_off}}},
              {{"buffer": 0, "byteOffset": {uv_off}, "byteLength": {uv_len}, "byteStride": 4}}"#
        ),
        r#"{"POSITION": 0, "TEXCOORD_0": 1}"#,
        "", // no extensionsUsed — base spec allows UBYTE normalized
    );

    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(&doc)
        .expect("base-spec UBYTE TEXCOORD must decode");
    let uv = &scene.meshes[0].primitives[0].uvs[0];
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    assert!(near(uv[0][0], 1.0), "{:?}", uv[0]); // 255/255
    assert!(near(uv[1][1], 64.0 / 255.0), "{:?}", uv[1]);
}

#[test]
fn rejects_quantized_position_without_extension() {
    // SHORT-normalized POSITION requires KHR_mesh_quantization in
    // extensionsUsed; omitting it must be rejected.
    let verts: [[i16; 3]; 2] = [[32767, 0, 0], [0, 0, 0]];
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
        r#"{"bufferView": 0, "componentType": 5122, "count": 2, "type": "VEC3", "normalized": true}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}, "byteStride": 8}}"#),
        r#"{"POSITION": 0}"#,
        "", // extension NOT declared
    );

    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("KHR_mesh_quantization") && msg.contains("extensionsUsed"),
        "expected extension-required rejection, got: {msg}"
    );
}

#[test]
fn quantized_position_records_attr_quant_sentinel() {
    // A quantized attribute should stash its (componentType, normalized)
    // form under the `__attr_quant` extras sentinel so the encoder can
    // round-trip it. Verify the sentinel is present and well-formed.
    let verts: [[i16; 3]; 2] = [[32767, 0, 0], [0, 0, 0]];
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
        r#"{"bufferView": 0, "componentType": 5122, "count": 2, "type": "VEC3", "normalized": true}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}, "byteStride": 8}}"#),
        r#"{"POSITION": 0}"#,
        r#""KHR_mesh_quantization""#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("decode");
    let prim = &scene.meshes[0].primitives[0];
    let sentinel = prim
        .extras
        .get("__attr_quant")
        .expect("__attr_quant sentinel present for quantized primitive");
    let pos = sentinel
        .as_object()
        .and_then(|o| o.get("POSITION"))
        .and_then(|p| p.as_object())
        .expect("POSITION entry");
    assert_eq!(
        pos.get("componentType").and_then(|c| c.as_u64()),
        Some(5122)
    );
    assert_eq!(pos.get("normalized").and_then(|n| n.as_bool()), Some(true));
}

#[test]
fn float_primitive_has_no_attr_quant_sentinel() {
    // A plain all-FLOAT primitive must NOT gain the sentinel — keeps
    // output identical to pre-quantization behaviour.
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
    let prim = &scene.meshes[0].primitives[0];
    assert!(
        !prim.extras.contains_key("__attr_quant"),
        "plain FLOAT primitive must not carry __attr_quant"
    );
}
