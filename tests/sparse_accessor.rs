//! Sparse accessor decode per glTF 2.0 §3.6.2.3.
//!
//! The encoder always emits dense storage (round-tripping a sparse
//! accessor that was just decoded yields a dense second pass — that's
//! spec-legal, since sparse is purely a storage optimisation).
//!
//! What this test verifies: a hand-crafted glTF JSON whose POSITION
//! accessor uses `sparse` to override two of the four base elements
//! decodes to the substituted values.

use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

fn pack_f32_le(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn pack_u16_le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

#[test]
fn sparse_position_overrides_two_of_four() {
    // Build the binary buffer manually:
    //   [0..48]   : 4 base VEC3 positions (12 bytes each):
    //               (0,0,0), (1,0,0), (0,1,0), (1,1,0)
    //   [48..52]  : 2 sparse indices (u16): 1, 3
    //   [52..76]  : 2 sparse VEC3 values: (10,0,0), (10,1,0)
    let mut bin: Vec<u8> = Vec::new();
    let base = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    for v in &base {
        for c in v {
            pack_f32_le(&mut bin, *c);
        }
    }
    assert_eq!(bin.len(), 48);
    // Sparse indices.
    pack_u16_le(&mut bin, 1);
    pack_u16_le(&mut bin, 3);
    // Pad to 4-byte align before the float values (4-byte floats are
    // already aligned at offset 52).
    assert_eq!(bin.len(), 52);
    // Sparse values.
    pack_f32_le(&mut bin, 10.0);
    pack_f32_le(&mut bin, 0.0);
    pack_f32_le(&mut bin, 0.0);
    pack_f32_le(&mut bin, 10.0);
    pack_f32_le(&mut bin, 1.0);
    pack_f32_le(&mut bin, 0.0);
    assert_eq!(bin.len(), 76);

    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "buffers": [
                {{
                    "byteLength": {len},
                    "uri": "data:application/octet-stream;base64,{b64}"
                }}
            ],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": 0,  "byteLength": 48 }},
                {{ "buffer": 0, "byteOffset": 48, "byteLength": 4  }},
                {{ "buffer": 0, "byteOffset": 52, "byteLength": 24 }}
            ],
            "accessors": [
                {{
                    "bufferView": 0,
                    "componentType": 5126,
                    "count": 4,
                    "type": "VEC3",
                    "sparse": {{
                        "count": 2,
                        "indices": {{ "bufferView": 1, "componentType": 5123 }},
                        "values":  {{ "bufferView": 2 }}
                    }}
                }}
            ],
            "meshes": [
                {{
                    "primitives": [
                        {{ "mode": 0, "attributes": {{ "POSITION": 0 }} }}
                    ]
                }}
            ],
            "nodes": [ {{ "mesh": 0 }} ],
            "scenes": [ {{ "nodes": [0] }} ],
            "scene": 0
        }}"#,
        len = bin.len(),
        b64 = b64,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();
    let p = &scene.meshes[0].primitives[0];
    assert_eq!(p.positions.len(), 4);
    // Index 0 unchanged (base value).
    assert_eq!(p.positions[0], [0.0, 0.0, 0.0]);
    // Index 1 overridden by sparse value 0.
    assert_eq!(p.positions[1], [10.0, 0.0, 0.0]);
    // Index 2 unchanged (base value).
    assert_eq!(p.positions[2], [0.0, 1.0, 0.0]);
    // Index 3 overridden by sparse value 1.
    assert_eq!(p.positions[3], [10.0, 1.0, 0.0]);
}

#[test]
fn sparse_with_no_base_buffer_view_initialises_to_zero() {
    // Spec §3.6.2.3 explicitly allows omitting the base bufferView —
    // the array is then initialised to zero before sparse overrides apply.
    let mut bin: Vec<u8> = Vec::new();
    // 3 sparse indices @ u16 => 6 bytes.
    for i in [0u16, 2, 4] {
        pack_u16_le(&mut bin, i);
    }
    // Pad to 4 (already at 6 bytes; pad to 8).
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let val_off = bin.len();
    // 3 VEC3 values: (1,0,0), (2,0,0), (3,0,0)
    for x in [1.0f32, 2.0, 3.0] {
        for c in [x, 0.0, 0.0] {
            pack_f32_le(&mut bin, c);
        }
    }
    let total = bin.len();
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
    let json = format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "buffers": [
                {{ "byteLength": {total}, "uri": "data:application/octet-stream;base64,{b64}" }}
            ],
            "bufferViews": [
                {{ "buffer": 0, "byteOffset": 0,        "byteLength": 6 }},
                {{ "buffer": 0, "byteOffset": {val_off},"byteLength": 36 }}
            ],
            "accessors": [
                {{
                    "componentType": 5126,
                    "count": 5,
                    "type": "VEC3",
                    "sparse": {{
                        "count": 3,
                        "indices": {{ "bufferView": 0, "componentType": 5123 }},
                        "values":  {{ "bufferView": 1 }}
                    }}
                }}
            ],
            "meshes": [
                {{ "primitives": [ {{ "mode": 0, "attributes": {{ "POSITION": 0 }} }} ] }}
            ],
            "nodes": [ {{ "mesh": 0 }} ],
            "scenes": [ {{ "nodes": [0] }} ],
            "scene": 0
        }}"#,
    );

    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json.as_bytes()).unwrap();
    let p = &scene.meshes[0].primitives[0];
    assert_eq!(p.positions.len(), 5);
    assert_eq!(p.positions[0], [1.0, 0.0, 0.0]);
    assert_eq!(p.positions[1], [0.0, 0.0, 0.0]); // implicit zero
    assert_eq!(p.positions[2], [2.0, 0.0, 0.0]);
    assert_eq!(p.positions[3], [0.0, 0.0, 0.0]); // implicit zero
    assert_eq!(p.positions[4], [3.0, 0.0, 0.0]);
}
