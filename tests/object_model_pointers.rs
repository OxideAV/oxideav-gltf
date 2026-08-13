//! Object Model registry rules for `KHR_animation_pointer` channels,
//! per `docs/3d/gltf/ObjectModel.md` (core mutable + read-only
//! pointer tables) and
//! `docs/3d/gltf/extensions/KHR_animation_pointer.md` §Operation:
//!
//! * "The property being animated MUST be mutable as defined by the
//!   glTF 2.0 Asset Object Model" —
//!   `ExtensionStackAnimationPointerReadOnly`.
//! * "The JSON Pointer MUST point to a property defined in the
//!   asset" — `ExtensionStackAnimationPointerIndex` (array index out
//!   of range) / `ExtensionStackAnimationPointerUndefined` (the
//!   Object-Model-documented undefined shapes: `/nodes/{}/weights`
//!   without a morphed mesh, weights element index past the target
//!   count, rotation/scale of a matrix-form node).
//! * "The output accessor MUST be compatible with the animated
//!   property data type" —
//!   `ExtensionStackAnimationPointerAccessorType`.
//! * The `float[]` row `/nodes/{}/weights` animates the whole
//!   morph-weight array, so its per-keyframe output element count is
//!   the morph-target count (`AnimationSamplerOutputCount` sizing).

use base64::Engine as _;
use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};

/// One mesh with 3 vertices and 2 morph targets, instantiated by
/// node 0 (TRS form by default); an animation with one
/// pointer-targeted channel. `out_kind` / `out_values` describe the
/// sampler's FLOAT output accessor; two keyframes (t = 0, 1).
fn pointer_doc(pointer: &str, out_kind: &str, out_values: &[f32], node_matrix: bool) -> Vec<u8> {
    let mut bin = Vec::new();
    // positions (accessor 2)
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for v in positions {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // morph delta (accessor 3, shared by both targets)
    let delta_off = bin.len();
    for _ in 0..3 {
        for &c in &[0.1f32, 0.0, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // input keyframes (accessor 0)
    let input_off = bin.len();
    for t in [0.0f32, 1.0] {
        bin.extend_from_slice(&t.to_le_bytes());
    }
    // output (accessor 1)
    let out_off = bin.len();
    for v in out_values {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let out_bytes = out_values.len() * 4;
    let comps = match out_kind {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        other => panic!("unhandled kind {other}"),
    };
    let out_count = out_values.len() / comps;
    let total = bin.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let node_transform = if node_matrix {
        r#""matrix": [2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0],"#
    } else {
        ""
    };
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "extensionsUsed": ["KHR_animation_pointer"],
        "buffers": [
            {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
        ],
        "bufferViews": [
            {{ "buffer": 0, "byteOffset": {input_off}, "byteLength": 8 }},
            {{ "buffer": 0, "byteOffset": {out_off}, "byteLength": {out_bytes} }},
            {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": {delta_off}, "byteLength": 36 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": {out_count}, "type": "{out_kind}" }},
            {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }},
            {{ "bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.1, 0.0, 0.0], "max": [0.1, 0.0, 0.0] }}
        ],
        "meshes": [
            {{
                "primitives": [
                    {{
                        "attributes": {{ "POSITION": 2 }},
                        "targets": [ {{ "POSITION": 3 }}, {{ "POSITION": 3 }} ]
                    }}
                ]
            }}
        ],
        "nodes": [ {{ "mesh": 0, {node_transform} "name": "morphed" }} ],
        "scenes": [ {{ "nodes": [0] }} ],
        "scene": 0,
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
                    {{ "input": 0, "interpolation": "LINEAR", "output": 1 }}
                ]
            }}
        ]
    }}"#
    )
    .into_bytes()
}

fn expect_err(doc: &[u8], needle: &str) {
    let mut dec = GltfDecoder::new();
    let err = dec.decode(doc).expect_err("decode must fail");
    let msg = format!("{err}");
    assert!(msg.contains(needle), "expected {needle}, got: {msg}");
}

#[test]
fn node_weights_float_array_pointer_round_trips() {
    // `/nodes/{}/weights` is `float[]` — 2 morph targets × 2 keyframes
    // = 4 SCALAR output elements. Previously the pointer lane assumed
    // 1 element/keyframe and false-rejected this conformant shape.
    let doc = pointer_doc("/nodes/0/weights", "SCALAR", &[0.0, 0.0, 1.0, 0.5], false);
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(&doc).expect("float[] weights pointer decodes");
    let ch = scene.extras["KHR_animation_pointer"]["animations"][0]["channels"][0].clone();
    assert_eq!(ch["pointer"], "/nodes/0/weights");
    let out: Vec<f64> = ch["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(out, vec![0.0, 0.0, 1.0, 0.5]);

    // Round-trip: the channel re-encodes and re-decodes intact.
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let scene2 = dec.decode(&glb).expect("re-decode");
    let ch2 = scene2.extras["KHR_animation_pointer"]["animations"][0]["channels"][0].clone();
    assert_eq!(ch2["pointer"], "/nodes/0/weights");
    assert_eq!(ch2["output"], ch["output"]);
}

#[test]
fn node_weights_scalar_element_pointer_accepted() {
    // `/nodes/{}/weights/{}` is a plain `float` — 1 element/keyframe.
    let doc = pointer_doc("/nodes/0/weights/1", "SCALAR", &[0.0, 1.0], false);
    let mut dec = GltfDecoder::new();
    dec.decode(&doc).expect("weights element pointer decodes");
}

#[test]
fn node_weights_wrong_output_count_rejected() {
    // 2 targets but only 1 SCALAR element per keyframe — the float[]
    // sizing rule catches the mismatch.
    let doc = pointer_doc("/nodes/0/weights", "SCALAR", &[0.0, 1.0], false);
    expect_err(&doc, "AnimationSamplerOutputCount");
}

#[test]
fn node_weights_element_index_past_target_count_rejected() {
    let doc = pointer_doc("/nodes/0/weights/2", "SCALAR", &[0.0, 1.0], false);
    expect_err(&doc, "ExtensionStackAnimationPointerUndefined");
}

#[test]
fn read_only_pointer_rejected() {
    // `/nodes/{}/matrix` is a read-only runtime property in the core
    // Object Model table.
    let doc = pointer_doc(
        "/nodes/0/matrix",
        "MAT4",
        &[0.0; 32], // 2 keyframes × 16
        false,
    );
    expect_err(&doc, "ExtensionStackAnimationPointerReadOnly");
}

#[test]
fn read_only_length_pointer_rejected() {
    let doc = pointer_doc("/nodes.length", "SCALAR", &[0.0, 1.0], false);
    expect_err(&doc, "ExtensionStackAnimationPointerReadOnly");
}

#[test]
fn registered_pointer_with_wrong_accessor_kind_rejected() {
    // `/nodes/{}/translation` is `float3` → VEC3; a SCALAR output is
    // incompatible per the §Operation data-type table.
    let doc = pointer_doc("/nodes/0/translation", "SCALAR", &[0.0, 1.0], false);
    expect_err(&doc, "ExtensionStackAnimationPointerAccessorType");
}

#[test]
fn registered_pointer_with_matching_accessor_kind_accepted() {
    let doc = pointer_doc(
        "/nodes/0/translation",
        "VEC3",
        &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
        false,
    );
    let mut dec = GltfDecoder::new();
    dec.decode(&doc).expect("VEC3 translation pointer decodes");
}

#[test]
fn out_of_range_collection_index_rejected() {
    // The asset defines 1 node; `/nodes/7/translation` dangles.
    let doc = pointer_doc(
        "/nodes/7/translation",
        "VEC3",
        &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
        false,
    );
    expect_err(&doc, "ExtensionStackAnimationPointerIndex");
}

#[test]
fn out_of_range_material_index_rejected() {
    // No materials at all — `/materials/0/alphaCutoff` cannot resolve.
    let doc = pointer_doc("/materials/0/alphaCutoff", "SCALAR", &[0.0, 1.0], false);
    expect_err(&doc, "ExtensionStackAnimationPointerIndex");
}

#[test]
fn weights_pointer_without_morphed_mesh_rejected() {
    // Node 0 has a mesh with targets in the fixture; point at a node
    // index that exists but... the fixture only has one node, so
    // build a doc whose pointer targets weights of a node with no
    // mesh: hand-roll it.
    let doc = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_animation_pointer"],
        "buffers": [
            { "byteLength": 16, "uri": "data:application/octet-stream;base64,AAAAAAAAgD8AAAAAAACAPw==" }
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 8 },
            { "buffer": 0, "byteOffset": 8, "byteLength": 8 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0] },
            { "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR" }
        ],
        "nodes": [ { "name": "meshless" } ],
        "scenes": [ { "nodes": [0] } ],
        "scene": 0,
        "animations": [
            {
                "channels": [
                    {
                        "sampler": 0,
                        "target": {
                            "path": "pointer",
                            "extensions": {
                                "KHR_animation_pointer": { "pointer": "/nodes/0/weights" }
                            }
                        }
                    }
                ],
                "samplers": [ { "input": 0, "interpolation": "LINEAR", "output": 1 } ]
            }
        ]
    }"#;
    expect_err(doc, "ExtensionStackAnimationPointerUndefined");
}

#[test]
fn rotation_pointer_on_matrix_form_node_rejected() {
    // ObjectModel §"Core Pointers": rotation/scale pointers are
    // undefined for a node that uses the static `matrix` form.
    let doc = pointer_doc(
        "/nodes/0/rotation",
        "VEC4",
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        true,
    );
    expect_err(&doc, "ExtensionStackAnimationPointerUndefined");
}

#[test]
fn translation_pointer_on_matrix_form_node_accepted() {
    // "the translation pointer is always defined" — matrix form does
    // not invalidate it.
    let doc = pointer_doc(
        "/nodes/0/translation",
        "VEC3",
        &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
        true,
    );
    let mut dec = GltfDecoder::new();
    dec.decode(&doc)
        .expect("translation pointer on matrix-form node decodes");
}
