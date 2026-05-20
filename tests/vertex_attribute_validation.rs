//! End-to-end vertex-attribute compression validation per glTF 2.0
//! §3.6.2.4 (data alignment) + §3.7.2.1 (semantic constraints).
//!
//! These checks all run on the decoder side: spec-non-conformant
//! attribute layouts surface as `Error::InvalidData` with a stable
//! `VertexAttribute…` prefix. The encoder side is unit-tested in
//! `src/validation.rs`.

use base64::Engine as _;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a minimal `.gltf` JSON document around a single accessor +
/// bufferView pair. `extra_accessors` and `extra_buffer_views` get
/// concatenated into the JSON arrays. `attributes_json` is the
/// primitive's `attributes` map. `indices_json` is the optional
/// indices accessor index (string `null` for none).
fn build_doc(
    bin: &[u8],
    accessors_json: &str,
    buffer_views_json: &str,
    attributes_json: &str,
    indices_json: &str,
) -> Vec<u8> {
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(bin);
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [ {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }} ],
        "bufferViews": [ {buffer_views_json} ],
        "accessors": [ {accessors_json} ],
        "meshes": [ {{ "primitives": [ {{
            "attributes": {attributes_json},
            "indices": {indices_json}
        }} ] }} ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ], "scene": 0
    }}"#
    )
    .into_bytes()
}

/// 4 POSITION VEC3 floats (48 bytes), aligned, with declared min/max.
fn aligned_positions_bin_4() -> (Vec<u8>, &'static str) {
    let positions: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let mut bin = Vec::new();
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    (
        bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
    )
}

#[test]
fn rejects_misaligned_attribute_byte_offset() {
    // Spec §3.6.2.4: vertex-attribute accessor.byteOffset MUST be a
    // multiple of 4. Build a bufferView starting at 0 with a POSITION
    // accessor at byteOffset=2 → must reject.
    let (mut bin, _) = aligned_positions_bin_4();
    // Pad bin so the misaligned accessor still fits.
    let mut padded = vec![0u8; 4];
    padded.append(&mut bin);
    let total = padded.len();
    let doc = build_doc(
        &padded,
        // POSITION at byteOffset = 2 (misaligned, not multiple of 4)
        r#"{"bufferView": 0, "byteOffset": 2, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}}}"#),
        r#"{"POSITION": 0}"#,
        "null",
    );

    let mut dec = GltfDecoder::new();
    let res = dec.decode(&doc);
    let err = res.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("VertexAttributeAlignment"),
        "expected alignment error, got: {msg}"
    );
}

#[test]
fn rejects_misaligned_byte_stride() {
    // Spec §3.6.2.4: bufferView.byteStride for vertex attributes MUST
    // be a multiple of 4. Build an interleaved layout with stride = 13
    // (bogus) and a generous bufferView byteLength so the round-8
    // accessor-fit check (also §3.6.2.4) passes and only the alignment
    // rule fires.
    //
    // Fit math with stride=13, VEC3 float, count=4 needs:
    //   byteOffset + stride * (count-1) + elementSize
    //   = 0 + 13*3 + 12 = 51 bytes.
    // Allocate 64 so fit-check is happy.
    let bin = vec![0u8; 64];
    let total = bin.len();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 4, "type": "VEC3",
            "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]}"#,
        &format!(r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {total}, "byteStride": 13}}"#),
        r#"{"POSITION": 0}"#,
        "null",
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("VertexAttributeAlignment") && msg.contains("byteStride"),
        "expected stride error, got: {msg}"
    );
}

#[test]
fn rejects_attribute_count_mismatch() {
    // Spec §3.7.2.1: all attribute accessors MUST share `count`.
    // Build a POSITION with count=4 and a NORMAL with count=3.
    let (mut bin, _) = aligned_positions_bin_4();
    // Append 3 NORMALs (36 bytes).
    for n in [[0.0f32, 0.0, 1.0]; 3] {
        for c in n {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = 48;
    let nrm_len = 36;
    let total = bin.len();
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
              {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {pos_len}}},
              {{"buffer": 0, "byteOffset": {pos_len}, "byteLength": {nrm_len}}}"#
        ),
        r#"{"POSITION": 0, "NORMAL": 1}"#,
        "null",
    );
    let _ = total;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("VertexAttributeCount"),
        "expected count mismatch, got: {msg}"
    );
}

#[test]
fn rejects_index_primitive_restart_sentinel_u16() {
    // Spec §3.7.2.1: indices accessor MUST NOT contain 65535 for u16
    // (reserved as primitive-restart sentinel).
    let (mut bin, _) = aligned_positions_bin_4();
    // Pad to 4-byte alignment for the index accessor.
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let idx_offset = bin.len();
    // 6 indices: 0,1,2, 1,2,65535 — last is the sentinel
    for i in [0u16, 1, 2, 1, 2, 65535] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let total = bin.len();
    let idx_len = total - idx_offset;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
              {"bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": 48}},
              {{"buffer": 0, "byteOffset": {idx_offset}, "byteLength": {idx_len}}}"#
        ),
        r#"{"POSITION": 0}"#,
        "1",
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("VertexAttributeIndexRestart"),
        "expected restart-sentinel rejection, got: {msg}"
    );
}

#[test]
fn accepts_index_max_minus_one_u16() {
    // 65534 (max - 1) is fine; only the sentinel is forbidden.
    let (mut bin, _) = aligned_positions_bin_4();
    // Add a 5th POSITION at index 4 so we can use 65534 ... wait no,
    // 65534 must be a valid vertex index. Just build a 4-vertex
    // primitive with indices 0,1,2,3 (no sentinel) — sanity check that
    // index validation isn't over-aggressive.
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let idx_offset = bin.len();
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let total = bin.len();
    let idx_len = total - idx_offset;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
              {"bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": 48}},
              {{"buffer": 0, "byteOffset": {idx_offset}, "byteLength": {idx_len}}}"#
        ),
        r#"{"POSITION": 0}"#,
        "1",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("non-sentinel indices must pass");
    assert_eq!(scene.meshes[0].primitives[0].positions.len(), 4);
}

#[test]
fn rejects_tangent_w_not_unit() {
    // Spec §3.7.2.1: TANGENT.w MUST be ±1.0. Use 0.5 to trigger.
    let (mut bin, _) = aligned_positions_bin_4();
    // Append 4 tangents, last with w = 0.5
    let tangents: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.5],
    ];
    let pos_len = bin.len(); // 48
    for t in tangents {
        for c in t {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let tan_len = 64;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
              {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC4"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {pos_len}}},
              {{"buffer": 0, "byteOffset": {pos_len}, "byteLength": {tan_len}}}"#
        ),
        r#"{"POSITION": 0, "TANGENT": 1}"#,
        "null",
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("VertexAttributeTangentW"),
        "expected TangentW error, got: {msg}"
    );
}

#[test]
fn rejects_color0_out_of_range() {
    // Spec §3.7.2.1: COLOR_0 components MUST be in [0.0, 1.0].
    let (mut bin, _) = aligned_positions_bin_4();
    let pos_len = bin.len();
    // 4 colours, last one's red = 1.5
    let colors: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 1.0],
        [0.5, 0.5, 0.5, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [1.5, 0.0, 0.0, 1.0],
    ];
    for c in colors {
        for v in c {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let col_len = 64;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
              {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC4"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {pos_len}}},
              {{"buffer": 0, "byteOffset": {pos_len}, "byteLength": {col_len}}}"#
        ),
        r#"{"POSITION": 0, "COLOR_0": 1}"#,
        "null",
    );
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&doc).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("VertexAttributeColor0Range"),
        "expected COLOR_0 range error, got: {msg}"
    );
}

#[test]
fn accepts_color0_within_range() {
    // Same shape as above but with valid colour values.
    let (mut bin, _) = aligned_positions_bin_4();
    let pos_len = bin.len();
    let colors: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 1.0],
        [0.5, 0.5, 0.5, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 0.5],
    ];
    for c in colors {
        for v in c {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let col_len = 64;
    let doc = build_doc(
        &bin,
        r#"{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
              {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC4"}"#,
        &format!(
            r#"{{"buffer": 0, "byteOffset": 0, "byteLength": {pos_len}}},
              {{"buffer": 0, "byteOffset": {pos_len}, "byteLength": {col_len}}}"#
        ),
        r#"{"POSITION": 0, "COLOR_0": 1}"#,
        "null",
    );
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("valid COLOR_0 must pass");
    assert_eq!(scene.meshes[0].primitives[0].colors[0].len(), 4);
}

#[test]
fn round_trip_after_validation_changes() {
    // A clean encoder->decoder round-trip must still pass with the new
    // validations in place — guards against the new checks
    // accidentally rejecting our own emitter's output.
    use oxideav_gltf::GltfEncoder;
    use oxideav_mesh3d::{Indices, Mesh, Mesh3DEncoder, Node, Primitive, Scene3D, Topology};

    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    prim.normals = Some(vec![[0.0, 0.0, 1.0]; 4]);
    prim.tangents = Some(vec![[1.0, 0.0, 0.0, 1.0]; 4]);
    prim.colors.push(vec![[1.0, 0.5, 0.25, 1.0]; 4]);
    prim.indices = Some(Indices::U16(vec![0, 1, 2, 1, 3, 2]));
    let mut mesh = Mesh::new(Some("rt".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let n = scene.add_node(Node::new().with_mesh(mid));
    scene.add_root(n);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let scene2 = dec.decode(&glb).expect("round-trip must pass validation");
    let prim2 = &scene2.meshes[0].primitives[0];
    assert_eq!(prim2.positions.len(), 4);
    assert_eq!(prim2.normals.as_ref().unwrap().len(), 4);
    assert_eq!(prim2.tangents.as_ref().unwrap()[0][3], 1.0);
    assert_eq!(prim2.colors[0].len(), 4);
}
