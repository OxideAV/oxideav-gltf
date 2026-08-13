//! Morph targets per glTF 2.0 §3.7.2.2.
//!
//! Each `mesh.primitives[i].targets[t]` is an attribute → accessor map
//! whose elements are vertex deltas (POSITION / NORMAL / TANGENT only,
//! VEC3 FLOAT) added to the base attribute weighted by `mesh.weights`
//! (or `node.weights`) per the formula:
//!
//! ```text
//! mesh.primitives[i].attribute =
//!   primitives[i].attribute
//!     + sum_t weight[t] * primitives[i].targets[t].attribute
//! ```
//!
//! The decoder fills the typed `oxideav_mesh3d::Primitive::targets`
//! field (`MorphTarget { position, normal, tangent }`) and the typed
//! `Mesh::weights` default-weight vector. Attributes the typed model
//! has no slot for (TEXCOORD_n / COLOR_n) ride the
//! `primitive.extras["__morph_targets"]` sidecar, index-aligned with
//! the typed list. The encoder merges both sources back into the JSON
//! `targets` array (and still accepts the pre-typed sidecar shape as
//! a legacy input for hand-authored scenes).

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};
use serde_json::json;

fn build_morph_doc(targets_json: &str, mesh_weights: Option<&str>) -> Vec<u8> {
    // Build a binary buffer with:
    //   bv0 = 3 base positions (9 floats = 36 bytes)
    //   then one VEC3 FLOAT array per accessor referenced by the
    //   targets JSON. We take the simpler route here and hard-code 3
    //   accessor blobs (POSITION_DELTA × however many targets) at
    //   known byte offsets — caller of this helper builds tests with
    //   the relevant target shapes.
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Three "delta" blobs, each 36 bytes = 3 vec3 floats.
    // Target 0 POSITION: (0.1, 0, 0) per vertex
    let off1 = bin.len();
    for _ in 0..3 {
        for &c in &[0.1f32, 0.0, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Target 1 POSITION: (0, 0.2, 0) per vertex
    let off2 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 0.2, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Target 2 POSITION: (0, 0, 0.3) per vertex
    let off3 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 0.0, 0.3] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // Target 3 NORMAL_0 delta: (0, 1, 0) per vertex
    let off4 = bin.len();
    for _ in 0..3 {
        for &c in &[0.0f32, 1.0, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let total = bin.len();
    let mw = mesh_weights.unwrap_or("");
    let mw_field = if mw.is_empty() {
        String::new()
    } else {
        format!(", \"weights\": {mw}")
    };
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off1}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off2}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off3}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {off4}, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.1, 0.0, 0.0], "max": [0.1, 0.0, 0.0] }},
            {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.2, 0.0], "max": [0.0, 0.2, 0.0] }},
            {{ "bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.3], "max": [0.0, 0.0, 0.3] }},
            {{ "bufferView": 4, "componentType": 5126, "count": 3, "type": "VEC3" }},
            {{ "bufferView": 5, "componentType": 5126, "count": 3, "type": "VEC3" }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{
                        "attributes": {{ "POSITION": 0, "NORMAL": 5 }},
                        "targets": {targets_json}
                    }}
                ]
                {mw_field}
            }}
        ],
        "nodes": [ {{ "mesh": 0 }} ],
        "scenes": [ {{ "nodes": [0] }} ],
        "scene": 0
    }}"#
    );
    json.into_bytes()
}

#[test]
fn one_target_position_round_trip() {
    // Single morph target: POSITION_0 -> accessor 1 (0.1,0,0 deltas).
    let bytes = build_morph_doc(r#"[ { "POSITION": 1 } ]"#, None);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).unwrap();

    let prim = &scene.meshes[0].primitives[0];
    assert_eq!(prim.targets.len(), 1, "one typed morph target");
    let pos = prim.targets[0]
        .position
        .as_ref()
        .expect("typed POSITION deltas");
    assert_eq!(pos.len(), 3);
    assert!((pos[0][0] - 0.1).abs() < 1e-6);
    assert!(prim.targets[0].normal.is_none());
    assert!(prim.targets[0].tangent.is_none());
    // A pure POSITION/NORMAL/TANGENT morph lives entirely in the typed
    // field — no residual sidecar.
    assert!(
        !prim.extras.contains_key("__morph_targets"),
        "no residual __morph_targets sidecar for typed-only attributes"
    );

    // Re-encode → decode and verify the deltas survive the round trip.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let scene2 = dec.decode(&glb).unwrap();
    assert_eq!(
        scene.meshes[0].primitives[0].targets, scene2.meshes[0].primitives[0].targets,
        "typed morph targets survive the round trip"
    );
}

#[test]
fn four_targets_with_mesh_weights() {
    // 4 morph targets with default mesh.weights = [0.0, 0.5, 0.0, 0.25].
    let targets = r#"[
        { "POSITION": 1 },
        { "POSITION": 2 },
        { "POSITION": 3 },
        { "POSITION": 1 }
    ]"#;
    let bytes = build_morph_doc(targets, Some("[0.0, 0.5, 0.0, 0.25]"));
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).unwrap();

    // Typed `Mesh::weights` carries the default morph weights.
    assert_eq!(scene.meshes[0].weights, vec![0.0, 0.5, 0.0, 0.25]);
    // The pre-typed sidecar is no longer emitted.
    assert!(!scene.meshes[0].primitives[0]
        .extras
        .contains_key("__mesh_weights"));

    assert_eq!(scene.meshes[0].primitives[0].targets.len(), 4);

    // Re-encode to .glb and pull the JSON chunk to verify
    // mesh.weights + primitive.targets are emitted at the right paths.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let mesh = &v["meshes"][0];
    assert_eq!(mesh["weights"], json!([0.0, 0.5, 0.0, 0.25]));
    let prim = &mesh["primitives"][0];
    let targets_out = prim["targets"].as_array().unwrap();
    assert_eq!(targets_out.len(), 4);
    for t in targets_out {
        assert!(t.as_object().unwrap().contains_key("POSITION"));
    }
}

#[test]
fn mixed_position_and_normal_target() {
    // One target with both POSITION_0 and NORMAL_0 attributes.
    let targets = r#"[ { "POSITION": 1, "NORMAL": 4 } ]"#;
    let bytes = build_morph_doc(targets, None);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&bytes).unwrap();
    let prim = &scene.meshes[0].primitives[0];
    assert_eq!(prim.targets.len(), 1);
    let tgt = &prim.targets[0];
    // POSITION delta first vertex was (0.1, 0, 0).
    let pos = tgt.position.as_ref().expect("typed POSITION deltas");
    assert!((pos[0][0] - 0.1).abs() < 1e-6);
    // NORMAL delta first vertex was (0, 1, 0).
    let nrm = tgt.normal.as_ref().expect("typed NORMAL deltas");
    assert!((nrm[0][1] - 1.0).abs() < 1e-6);

    // Round-trip through encoder.
    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let scene2 = dec.decode(&glb).unwrap();
    assert_eq!(
        prim.targets, scene2.meshes[0].primitives[0].targets,
        "typed morph targets survive the round trip"
    );
}

#[test]
fn legacy_sidecar_scene_still_encodes() {
    // Pre-typed callers hand-author `__morph_targets` +
    // `__mesh_weights` sidecars with an empty typed `targets` field.
    // The encoder must keep accepting that shape: decode a plain
    // (no-morph) document, inject the legacy sidecars, and verify the
    // re-encoded JSON carries `targets` + `mesh.weights`.
    let bytes = build_morph_doc(r#"[]"#, None);
    let mut dec = GltfDecoder::new();
    let mut scene = dec.decode(&bytes).unwrap();
    assert!(scene.meshes[0].primitives[0].targets.is_empty());

    let prim = &mut scene.meshes[0].primitives[0];
    prim.extras.insert(
        "__morph_targets".to_owned(),
        json!([ { "POSITION": [[0.1, 0.0, 0.0], [0.1, 0.0, 0.0], [0.1, 0.0, 0.0]] } ]),
    );
    prim.extras
        .insert("__mesh_weights".to_owned(), json!([0.75]));

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    let mesh = &v["meshes"][0];
    assert_eq!(mesh["weights"], json!([0.75]));
    let targets_out = mesh["primitives"][0]["targets"].as_array().unwrap();
    assert_eq!(targets_out.len(), 1);
    assert!(targets_out[0].as_object().unwrap().contains_key("POSITION"));

    // And the re-decoded scene surfaces them through the typed fields.
    let scene2 = dec.decode(&glb).unwrap();
    assert_eq!(scene2.meshes[0].weights, vec![0.75]);
    let pos = scene2.meshes[0].primitives[0].targets[0]
        .position
        .as_ref()
        .expect("typed POSITION deltas after legacy round trip");
    assert!((pos[0][0] - 0.1).abs() < 1e-6);
}

#[test]
fn typed_mesh_weights_take_precedence_over_legacy_sidecar() {
    // When a caller sets BOTH the typed `Mesh::weights` and the legacy
    // `__mesh_weights` sidecar, the typed field is authoritative.
    let bytes = build_morph_doc(r#"[ { "POSITION": 1 } ]"#, Some("[0.5]"));
    let mut dec = GltfDecoder::new();
    let mut scene = dec.decode(&bytes).unwrap();
    assert_eq!(scene.meshes[0].weights, vec![0.5]);
    scene.meshes[0].primitives[0]
        .extras
        .insert("__mesh_weights".to_owned(), json!([0.125]));

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let json_chunk = {
        let n = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        glb[20..20 + n].to_vec()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_chunk).unwrap();
    assert_eq!(v["meshes"][0]["weights"], json!([0.5]));
}

#[test]
fn typed_and_sidecar_target_count_mismatch_rejected() {
    // A sidecar that disagrees with the typed `targets` length is a
    // caller bug — the encoder refuses rather than guessing an
    // alignment.
    let bytes = build_morph_doc(r#"[ { "POSITION": 1 } ]"#, None);
    let mut dec = GltfDecoder::new();
    let mut scene = dec.decode(&bytes).unwrap();
    assert_eq!(scene.meshes[0].primitives[0].targets.len(), 1);
    scene.meshes[0].primitives[0].extras.insert(
        "__morph_targets".to_owned(),
        json!([
            { "TEXCOORD_0": [[0.1, 0.0], [0.1, 0.0], [0.1, 0.0]] },
            { "TEXCOORD_0": [[0.2, 0.0], [0.2, 0.0], [0.2, 0.0]] }
        ]),
    );

    let mut enc = GltfEncoder::new();
    let err = enc.encode(&scene).unwrap_err();
    assert!(
        format!("{err}").contains("MorphTargetSidecarCount"),
        "expected MorphTargetSidecarCount, got: {err}"
    );
}
