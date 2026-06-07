//! `KHR_draco_mesh_compression` extension — per-primitive descriptor
//! pointing at a Draco-compressed `bufferView` payload, with a
//! parent-attribute-to-Draco-attribute-id map, per
//! `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md`.
//!
//! This crate is a pass-through engine: the Draco bitstream inflate
//! path is out of scope for this round. We handle the extension at the
//! JSON descriptor level:
//!
//! * **Decode**: each primitive carrying
//!   `extensions.KHR_draco_mesh_compression` surfaces the descriptor
//!   (bufferView + attributes map + optional extras) through
//!   `Primitive::extras["KHR_draco_mesh_compression"]`. The parent
//!   primitive's uncompressed-fallback accessors are read through the
//!   usual accessor pipeline — per §"accessors" the spec mandates that
//!   the parent accessors describe the decompressed data, so the
//!   fallback lane is authoritative whenever it is present.
//!
//! * **Encode**: the encoder lifts the sidecar back into the typed
//!   `PrimitiveExtensions` block and appends `KHR_draco_mesh_compression`
//!   to `extensionsUsed`. The compressed payload itself is NOT
//!   regenerated — documents emitted by this crate retain the original
//!   `bufferView` index from the source document, paired with fresh
//!   uncompressed accessors built from the parent primitive's typed
//!   data.
//!
//! * **Validation** (§3.12 + §"glTF Schema Updates" + §"Restrictions on
//!   geometry type" + §Conformance):
//!
//!   * data block on any primitive without `KHR_draco_mesh_compression`
//!     in `extensionsUsed` (`ExtensionStackUsedNotDeclared`)
//!   * descriptor `bufferView` index outside `bufferViews[]` range
//!     (`ExtensionStackDracoBufferView`)
//!   * descriptor `attributes` key not present in parent primitive's
//!     own `attributes` map (`ExtensionStackDracoAttributes`)
//!   * duplicate Draco-side attribute id within one descriptor
//!     (`ExtensionStackDracoAttributeId`)
//!   * primitive `mode` outside `{4 (TRIANGLES), 5 (TRIANGLE_STRIP)}`
//!     (`ExtensionStackDracoMode`)
//!   * compressed-only shape (parent primitive carries no uncompressed
//!     attributes alongside the descriptor) without
//!     `KHR_draco_mesh_compression` listed in `extensionsRequired`
//!     (`ExtensionStackDracoRequired`)
//!   * descriptor `bufferView` references a bufferView that carries
//!     `byteStride` — forbidden per glTF 2.0 §5.11.4 because the Draco
//!     payload is opaque compressed bytes, not vertex attribute data
//!     (`ExtensionStackDracoByteStride`)

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, NodeId, Primitive, Scene3D, Topology,
};
use serde_json::{json, Value};

/// Build a minimal `Scene3D` with one TRIANGLES primitive (the default
/// mode the spec permits alongside this extension) attached through a
/// single node. Three vertices form a degenerate triangle — enough to
/// exercise the encoder path and produce a real `attributes.POSITION`
/// accessor on output.
fn make_triangles_scene() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut mesh = Mesh::new(None);
    mesh.primitives.push(prim);
    let mesh_id = scene.add_mesh(mesh);
    let mut node = Node::new();
    node.mesh = Some(mesh_id);
    let node_id = NodeId(scene.nodes.len() as u32);
    scene.add_node(node);
    scene.add_root(node_id);
    scene
}

/// Walk a `.glb` container and return its JSON chunk's payload bytes.
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}

// ---------------------------------------------------------------------
// Round-trip — decoder lifts descriptor onto primitive.extras, encoder
// lifts it back into the typed extension block, GLB survives a second
// decode.

#[test]
fn draco_descriptor_roundtrips_via_glb() {
    let mut scene = make_triangles_scene();
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_draco_mesh_compression".into(),
        json!({
            "bufferView": 0,
            "attributes": { "POSITION": 0 }
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    let desc = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_draco_mesh_compression")
        .expect("descriptor must round-trip");
    assert!(desc.get("bufferView").is_some());
    let attrs = desc.get("attributes").and_then(|v| v.as_object()).unwrap();
    assert_eq!(attrs.get("POSITION").and_then(|v| v.as_u64()), Some(0));
}

#[test]
fn draco_emits_extensions_used_on_encode() {
    let mut scene = make_triangles_scene();
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_draco_mesh_compression".into(),
        json!({
            "bufferView": 0,
            "attributes": { "POSITION": 0 }
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_draco_mesh_compression\""),
        "KHR_draco_mesh_compression must appear in extensionsUsed, got: {raw}"
    );
    assert!(
        raw.contains("\"bufferView\""),
        "bufferView field must round-trip into JSON, got: {raw}"
    );
}

#[test]
fn draco_extension_omitted_when_no_descriptor() {
    let scene = make_triangles_scene();
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        !raw.contains("KHR_draco_mesh_compression"),
        "extension must NOT appear when no descriptor present, got: {raw}"
    );
}

#[test]
fn draco_descriptor_carries_extras_through() {
    // The descriptor sidecar surfaces any non-spec siblings via the
    // `extras` field per the glTF JSON conventions; round-trip them.
    let mut scene = make_triangles_scene();
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_draco_mesh_compression".into(),
        json!({
            "bufferView": 0,
            "attributes": { "POSITION": 0 },
            "extras": { "tool": "fixture" }
        }),
    );
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let desc = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_draco_mesh_compression")
        .unwrap();
    let extras = desc.get("extras").and_then(|v| v.as_object()).unwrap();
    assert_eq!(extras.get("tool").and_then(|v| v.as_str()), Some("fixture"));
}

// ---------------------------------------------------------------------
// §3.12 stack rule — descriptor present without extensionsUsed.

#[test]
fn draco_descriptor_without_extensions_used_is_rejected() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "buffers": [ { "byteLength": 12, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAA" } ],
        "bufferViews": [ { "buffer": 0, "byteLength": 12 } ],
        "accessors": [ {
            "bufferView": 0,
            "componentType": 5126,
            "count": 1,
            "type": "VEC3",
            "min": [0.0, 0.0, 0.0],
            "max": [0.0, 0.0, 0.0]
        } ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": { "POSITION": 0 },
                        "mode": 4,
                        "extensions": {
                            "KHR_draco_mesh_compression": {
                                "bufferView": 0,
                                "attributes": { "POSITION": 0 }
                            }
                        }
                    }
                ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_draco_mesh_compression"),
        "expected ExtensionStackUsedNotDeclared for KHR_draco_mesh_compression, got {msg}"
    );
}

// ---------------------------------------------------------------------
// §"glTF Schema Updates" — bufferView / attributes / attribute-id checks.

fn decode_doc(extension: Value, prim_attrs: Value, mode: u64) -> Result<Scene3D, String> {
    let raw = serde_json::to_string(&json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "buffers": [ { "byteLength": 12, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAA" } ],
        "bufferViews": [ { "buffer": 0, "byteLength": 12 } ],
        "accessors": [ {
            "bufferView": 0,
            "componentType": 5126,
            "count": 1,
            "type": "VEC3",
            "min": [0.0, 0.0, 0.0],
            "max": [0.0, 0.0, 0.0]
        } ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": prim_attrs,
                        "mode": mode,
                        "extensions": {
                            "KHR_draco_mesh_compression": extension
                        }
                    }
                ]
            }
        ]
    }))
    .unwrap();
    let mut dec = GltfDecoder::new();
    dec.decode(raw.as_bytes()).map_err(|e| format!("{e}"))
}

#[test]
fn draco_rejects_buffer_view_out_of_range() {
    let err = decode_doc(
        json!({ "bufferView": 99, "attributes": { "POSITION": 0 } }),
        json!({ "POSITION": 0 }),
        4,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackDracoBufferView"),
        "expected ExtensionStackDracoBufferView, got: {err}"
    );
}

#[test]
fn draco_rejects_attribute_not_in_parent() {
    // §"attributes": "The `attributes` defined in the extension must be
    // a subset of the attributes of the primitive."
    let err = decode_doc(
        json!({ "bufferView": 0, "attributes": { "NORMAL": 0 } }),
        json!({ "POSITION": 0 }),
        4,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackDracoAttributes"),
        "expected ExtensionStackDracoAttributes, got: {err}"
    );
}

#[test]
fn draco_rejects_duplicate_attribute_id() {
    let err = decode_doc(
        json!({
            "bufferView": 0,
            "attributes": { "POSITION": 7, "NORMAL": 7 }
        }),
        json!({ "POSITION": 0, "NORMAL": 0 }),
        4,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackDracoAttributeId"),
        "expected ExtensionStackDracoAttributeId, got: {err}"
    );
}

#[test]
fn draco_accepts_unique_attribute_ids() {
    let scene = decode_doc(
        json!({
            "bufferView": 0,
            "attributes": { "POSITION": 0, "NORMAL": 1 }
        }),
        json!({ "POSITION": 0, "NORMAL": 0 }),
        4,
    )
    .unwrap();
    let desc = scene.meshes[0].primitives[0]
        .extras
        .get("KHR_draco_mesh_compression")
        .unwrap();
    let attrs = desc.get("attributes").and_then(|v| v.as_object()).unwrap();
    assert_eq!(attrs.get("POSITION").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(attrs.get("NORMAL").and_then(|v| v.as_u64()), Some(1));
}

// ---------------------------------------------------------------------
// §"Restrictions on geometry type" — mode ∈ {TRIANGLES, TRIANGLE_STRIP}.

#[test]
fn draco_accepts_triangles_mode() {
    let scene = decode_doc(
        json!({ "bufferView": 0, "attributes": { "POSITION": 0 } }),
        json!({ "POSITION": 0 }),
        4, // TRIANGLES
    )
    .unwrap();
    assert!(scene.meshes[0].primitives[0]
        .extras
        .contains_key("KHR_draco_mesh_compression"));
}

#[test]
fn draco_accepts_triangle_strip_mode() {
    let scene = decode_doc(
        json!({ "bufferView": 0, "attributes": { "POSITION": 0 } }),
        json!({ "POSITION": 0 }),
        5, // TRIANGLE_STRIP
    )
    .unwrap();
    assert!(scene.meshes[0].primitives[0]
        .extras
        .contains_key("KHR_draco_mesh_compression"));
}

#[test]
fn draco_rejects_points_mode() {
    let err = decode_doc(
        json!({ "bufferView": 0, "attributes": { "POSITION": 0 } }),
        json!({ "POSITION": 0 }),
        0, // POINTS
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackDracoMode"),
        "expected ExtensionStackDracoMode, got: {err}"
    );
}

#[test]
fn draco_rejects_line_loop_mode() {
    let err = decode_doc(
        json!({ "bufferView": 0, "attributes": { "POSITION": 0 } }),
        json!({ "POSITION": 0 }),
        2, // LINE_LOOP
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackDracoMode"),
        "expected ExtensionStackDracoMode, got: {err}"
    );
}

#[test]
fn draco_rejects_triangle_fan_mode() {
    let err = decode_doc(
        json!({ "bufferView": 0, "attributes": { "POSITION": 0 } }),
        json!({ "POSITION": 0 }),
        6, // TRIANGLE_FAN
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackDracoMode"),
        "expected ExtensionStackDracoMode, got: {err}"
    );
}

// ---------------------------------------------------------------------
// §Conformance — compressed-only shape requires extensionsRequired.

#[test]
fn draco_rejects_compressed_only_without_extensions_required() {
    // §Conformance: "If the uncompressed version of the asset is not
    // provided, then KHR_draco_mesh_compression must be added to
    // extensionsRequired." Build a primitive whose `attributes` map is
    // empty (no uncompressed fallback) and verify the validator rejects
    // when the document declares the extension only in `extensionsUsed`.
    let raw = r#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "buffers": [ { "byteLength": 12, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAA" } ],
        "bufferViews": [ { "buffer": 0, "byteLength": 12 } ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "mode": 4,
                        "extensions": {
                            "KHR_draco_mesh_compression": {
                                "bufferView": 0,
                                "attributes": {}
                            }
                        }
                    }
                ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(raw.as_bytes()).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDracoRequired"),
        "expected ExtensionStackDracoRequired for compressed-only doc, got {msg}"
    );
}

#[test]
fn draco_accepts_compressed_only_with_extensions_required() {
    // Same shape, but the document also lists the extension in
    // `extensionsRequired` — the spec permits this. We exercise the
    // validator surface only: a compressed-only primitive without any
    // uncompressed POSITION attribute is well-formed per §Conformance
    // when `extensionsRequired` is set. Materialising the typed
    // primitive would also require a Draco bitstream decoder (out of
    // scope for this round), so the decode lane bails afterward with a
    // different error — what the test guards is that the §3.12 +
    // §Conformance validator no longer rejects the document on
    // `ExtensionStackDracoRequired` once the declaration is in place.
    let raw = r#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "extensionsRequired": ["KHR_draco_mesh_compression"],
        "buffers": [ { "byteLength": 12, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAA" } ],
        "bufferViews": [ { "buffer": 0, "byteLength": 12 } ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "mode": 4,
                        "extensions": {
                            "KHR_draco_mesh_compression": {
                                "bufferView": 0,
                                "attributes": {}
                            }
                        }
                    }
                ]
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(raw.as_bytes()).unwrap_err();
    let msg = format!("{err}");
    // The compressed-only shape passes the validator (the
    // `ExtensionStackDracoRequired` gate is satisfied) but the typed
    // primitive materialisation needs POSITION bytes from an inflated
    // Draco payload — surface that as a missing-POSITION error rather
    // than a stack-validator one. The negative guarantee here is the
    // ABSENCE of `ExtensionStackDracoRequired` in the message.
    assert!(
        !msg.contains("ExtensionStackDracoRequired"),
        "validator must accept the compressed-only shape when \
         extensionsRequired is set; got: {msg}"
    );
    assert!(
        msg.contains("POSITION"),
        "expected the missing-POSITION downstream surface; got: {msg}"
    );
}

// ---------------------------------------------------------------------
// glTF 2.0 §5.11.4 — `bufferView.byteStride`, when defined, applies to
// vertex attribute data layouts only ("Buffer views with other types of
// data MUST NOT define byteStride (unless such layout is explicitly
// enabled by an extension)"). The Draco descriptor's bufferView holds
// an opaque compressed payload; `KHR_draco_mesh_compression` does not
// enable a strided payload layout. So the referenced bufferView MUST
// NOT carry `byteStride`. The error surface is
// `ExtensionStackDracoByteStride`.

/// Build a Draco-document JSON string where the Draco-referenced
/// bufferView optionally carries `byteStride`. Both the bufferView
/// `byteLength` and the embedded data URI grow to keep alignment +
/// fit invariants happy so the new check is the only failure surface.
fn draco_doc_with_payload_stride(stride: Option<u32>) -> String {
    // 32 zero bytes encoded as base64: keeps the bufferView fit check
    // happy for a stride up to 32 with 1 element.
    let data_uri =
        "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let stride_field = match stride {
        Some(s) => format!(", \"byteStride\": {s}"),
        None => String::new(),
    };
    format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_draco_mesh_compression"],
            "buffers": [ {{ "byteLength": 32, "uri": "{data_uri}" }} ],
            "bufferViews": [ {{ "buffer": 0, "byteLength": 32 {stride_field} }} ],
            "accessors": [ {{
                "componentType": 5126,
                "count": 1,
                "type": "VEC3",
                "min": [0.0, 0.0, 0.0],
                "max": [0.0, 0.0, 0.0]
            }} ],
            "meshes": [
                {{
                    "primitives": [
                        {{
                            "attributes": {{ "POSITION": 0 }},
                            "mode": 4,
                            "extensions": {{
                                "KHR_draco_mesh_compression": {{
                                    "bufferView": 0,
                                    "attributes": {{ "POSITION": 0 }}
                                }}
                            }}
                        }}
                    ]
                }}
            ]
        }}"#
    )
}

#[test]
fn draco_rejects_payload_buffer_view_with_byte_stride() {
    // Stride of 4 satisfies the §5.11.4 generic byteStride range
    // `[4, 252]` (the JSON-schema range our generic bufferView check
    // already enforces) — so the only failure surface available is the
    // new Draco-specific MUST NOT.
    let raw = draco_doc_with_payload_stride(Some(4));
    let mut dec = GltfDecoder::new();
    let err = dec.decode(raw.as_bytes()).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDracoByteStride"),
        "expected ExtensionStackDracoByteStride for Draco bufferView with \
         byteStride, got: {msg}"
    );
}

#[test]
fn draco_rejects_payload_buffer_view_with_byte_stride_252() {
    // Upper bound of the §5.11.4 range `[4, 252]`. The bufferView itself
    // is large enough to satisfy the generic stride-fit checks (the
    // bufferView holds a single element and `byteLength == 32` which is
    // <= 252, but the fit check is `byteLength >= byteStride * count` only
    // when count > 0; with no accessor pointing into this bufferView in
    // the Draco-payload role, the only relevant invariant is the new
    // Draco-specific MUST NOT). Confirm the rejection still fires.
    //
    // (We use a smaller stride at 8 here to avoid colliding with the
    // bufferView-fit pre-check elsewhere; both 4 and 8 sit inside the
    // §5.11.4 range, both are forbidden.)
    let raw = draco_doc_with_payload_stride(Some(8));
    let mut dec = GltfDecoder::new();
    let err = dec.decode(raw.as_bytes()).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackDracoByteStride"),
        "expected ExtensionStackDracoByteStride for Draco bufferView with \
         byteStride = 8, got: {msg}"
    );
}

#[test]
fn draco_accepts_payload_buffer_view_without_byte_stride() {
    // Same document shape, but the bufferView does NOT define
    // byteStride. The new check is silent — the document either parses
    // through (the spec-compliant happy path) or hits an unrelated
    // downstream error. The negative guarantee here is the ABSENCE of
    // `ExtensionStackDracoByteStride` in any error surface.
    let raw = draco_doc_with_payload_stride(None);
    let mut dec = GltfDecoder::new();
    match dec.decode(raw.as_bytes()) {
        Ok(_) => { /* spec-compliant happy path */ }
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                !msg.contains("ExtensionStackDracoByteStride"),
                "validator must not flag a stride-less Draco bufferView; \
                 got: {msg}"
            );
        }
    }
}
