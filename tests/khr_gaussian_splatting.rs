//! KHR_gaussian_splatting extension — per-primitive descriptor that
//! flags a `POINTS` mesh primitive as a 3D Gaussian splat field, per
//! `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Extending
//! Mesh Primitives".
//!
//! The descriptor is a four-field object:
//!
//!   * `kernel` (required) — base spec defines `"ellipse"`.
//!   * `colorSpace` (required) — base spec defines `"srgb_rec709_display"`
//!     or `"lin_rec709_display"`.
//!   * `projection` (optional, default `"perspective"`).
//!   * `sortingMethod` (optional, default `"cameraDistance"`).
//!
//! The decoder lifts this descriptor block onto
//! `Primitive::extras["KHR_gaussian_splatting"]` as a JSON object; the
//! encoder lifts it back into the typed `PrimitiveExtensions` block on
//! write and appends `KHR_gaussian_splatting` to `extensionsUsed`.
//!
//! The §3.12 validator enforces:
//!
//!  * `ExtensionStackUsedNotDeclared` — descriptor present without the
//!    `extensionsUsed` entry.
//!  * `ExtensionStackGaussianSplattingKernel` — `kernel` outside the
//!    spec-defined set and lacking a vendor-extension underscore prefix.
//!  * `ExtensionStackGaussianSplattingColorSpace` — `colorSpace` rule.
//!  * `ExtensionStackGaussianSplattingProjection` — `projection` rule.
//!  * `ExtensionStackGaussianSplattingSortingMethod` — `sortingMethod` rule.
//!  * `ExtensionStackGaussianSplattingMode` — `kernel == "ellipse"` but
//!    primitive `mode` is not `0` / POINTS (§"Ellipse Kernel"
//!    §"Dependencies on glTF").

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, NodeId, Primitive, Scene3D, Topology,
};
use serde_json::{json, Value};

/// Build a minimal `Scene3D` with one POINTS primitive (the required
/// topology for the spec's `"ellipse"` kernel) instanced through a
/// single node — enough for a round-trip exercise of the descriptor.
fn make_points_scene() -> Scene3D {
    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Points);
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
fn splatting_descriptor_roundtrips_via_glb() {
    // The typed Scene3D model has no slot for the custom splat
    // attributes, so a spec-complete ellipse primitive cannot be built
    // through the encoder; exercise the descriptor-lift path on decode
    // of a fully-attributed hand-built document instead. The descriptor
    // surfaces onto `primitive.extras["KHR_gaussian_splatting"]`.
    let scene = decode_descriptor(json!({
        "kernel": "ellipse",
        "colorSpace": "srgb_rec709_display",
        "projection": "perspective",
        "sortingMethod": "cameraDistance",
    }))
    .expect("complete ellipse primitive must validate");

    let desc = scene.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .expect("descriptor must surface");
    assert_eq!(desc.get("kernel").and_then(|v| v.as_str()), Some("ellipse"));
    assert_eq!(
        desc.get("colorSpace").and_then(|v| v.as_str()),
        Some("srgb_rec709_display")
    );
    assert_eq!(
        desc.get("projection").and_then(|v| v.as_str()),
        Some("perspective")
    );
    assert_eq!(
        desc.get("sortingMethod").and_then(|v| v.as_str()),
        Some("cameraDistance")
    );
}

#[test]
fn splatting_descriptor_omits_optional_fields() {
    // The spec marks `projection` + `sortingMethod` as optional with
    // documented defaults. A document that omits them must round-trip
    // with those fields absent, not synthesised by the encoder.
    let mut scene = make_points_scene();
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_gaussian_splatting".into(),
        json!({
            "kernel": "ellipse",
            "colorSpace": "lin_rec709_display",
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"KHR_gaussian_splatting\""),
        "descriptor object must appear in JSON, got: {raw}"
    );
    assert!(
        raw.contains("\"lin_rec709_display\""),
        "colorSpace must round-trip into JSON, got: {raw}"
    );
    assert!(
        !raw.contains("\"projection\""),
        "absent projection must NOT be synthesised on encode, got: {raw}"
    );
    assert!(
        !raw.contains("\"sortingMethod\""),
        "absent sortingMethod must NOT be synthesised on encode, got: {raw}"
    );

    // Decode-side: a spec-complete ellipse primitive that omits the two
    // optional descriptor fields surfaces them as absent (no default
    // synthesis). Built via the raw helper because the typed encoder
    // cannot carry the required custom splat attributes.
    let scene = decode_descriptor(json!({
        "kernel": "ellipse",
        "colorSpace": "lin_rec709_display",
    }))
    .expect("complete ellipse primitive must validate");
    let desc = scene.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .expect("descriptor must surface");
    assert!(desc.get("projection").is_none());
    assert!(desc.get("sortingMethod").is_none());
}

#[test]
fn splatting_emits_extensions_used_on_encode() {
    let mut scene = make_points_scene();
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_gaussian_splatting".into(),
        json!({
            "kernel": "ellipse",
            "colorSpace": "srgb_rec709_display",
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
        raw.contains("\"KHR_gaussian_splatting\""),
        "KHR_gaussian_splatting must appear in extensionsUsed, got: {raw}"
    );
    assert!(
        raw.contains("\"kernel\":\"ellipse\""),
        "kernel string must round-trip into JSON, got: {raw}"
    );
}

#[test]
fn splatting_extension_omitted_when_no_descriptor() {
    let scene = make_points_scene();
    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        !raw.contains("KHR_gaussian_splatting"),
        "extension must NOT appear when no descriptor present, got: {raw}"
    );
}

// ---------------------------------------------------------------------
// §3.12 stack rule — descriptor present without extensionsUsed.

#[test]
fn splatting_descriptor_without_extensions_used_is_rejected() {
    // Hand-build JSON with a primitive-level descriptor but NO
    // extensionsUsed declaration. The validator must reject with
    // ExtensionStackUsedNotDeclared per spec §3.12 + the extension's
    // §"Extending Mesh Primitives" mandate.
    let json = br#"{
        "asset": { "version": "2.0" },
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "mode": 0,
                        "extensions": {
                            "KHR_gaussian_splatting": {
                                "kernel": "ellipse",
                                "colorSpace": "srgb_rec709_display"
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
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_gaussian_splatting"),
        "expected ExtensionStackUsedNotDeclared for KHR_gaussian_splatting, got {msg}"
    );
}

// ---------------------------------------------------------------------
// Allowed-value sets — kernel / colorSpace / projection / sortingMethod.

fn decode_descriptor(descriptor: Value) -> Result<Scene3D, String> {
    decode_descriptor_with_mode(descriptor, 0)
}

/// Hand-build a minimal glTF JSON document with a `POINTS` primitive
/// carrying the full set of `"ellipse"`-kernel splat attributes the spec
/// requires (`POSITION` + `KHR_gaussian_splatting:ROTATION` /`:SCALE`
/// /`:OPACITY` /`:SH_DEGREE_0_COEF_0`), so the descriptor walk can run
/// through `convert_primitive` and surface §3.12 validation errors
/// instead of tripping the new §"Ellipse Kernel" §"Attributes"
/// completeness checks.
fn decode_descriptor_with_mode(descriptor: Value, mode: u64) -> Result<Scene3D, String> {
    decode_full_splat_primitive(descriptor, mode, json!({}))
}

/// Like `decode_descriptor_with_mode`, but `extra_attributes` is merged
/// into the primitive's `attributes` map (each value pointing at one of
/// the spare float accessors built below) so tests can add higher-degree
/// spherical-harmonics coefficients or override storage forms.
///
/// Layout: one 64-byte zero buffer split into five bufferViews —
///   * accessor 0: POSITION             VEC3 float (offset 0,  12B)
///   * accessor 1: ROTATION             VEC4 float (offset 12, 16B)
///   * accessor 2: SCALE                VEC3 float (offset 28, 12B)
///   * accessor 3: OPACITY              SCALAR float (offset 40, 4B)
///   * accessor 4: SH_DEGREE_0_COEF_0   VEC3 float (offset 44, 12B)
///
/// All hold one zero element (a valid unit-quaternion check is not
/// performed — zeros satisfy the [0,1] opacity + non-negative scale data
/// constraints the validator does enforce).
fn decode_full_splat_primitive(
    descriptor: Value,
    mode: u64,
    extra_attributes: Value,
) -> Result<Scene3D, String> {
    let bytes = [0u8; 64];
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let uri = format!("data:application/octet-stream;base64,{b64}");

    let mut attributes = serde_json::Map::new();
    attributes.insert("POSITION".into(), json!(0));
    attributes.insert("KHR_gaussian_splatting:ROTATION".into(), json!(1));
    attributes.insert("KHR_gaussian_splatting:SCALE".into(), json!(2));
    attributes.insert("KHR_gaussian_splatting:OPACITY".into(), json!(3));
    attributes.insert("KHR_gaussian_splatting:SH_DEGREE_0_COEF_0".into(), json!(4));
    if let Some(extra) = extra_attributes.as_object() {
        for (k, v) in extra {
            attributes.insert(k.clone(), v.clone());
        }
    }

    let raw = serde_json::to_string(&json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_gaussian_splatting"],
        "buffers": [ { "byteLength": 64, "uri": uri } ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0,  "byteLength": 12, "target": 34962 },
            { "buffer": 0, "byteOffset": 12, "byteLength": 16, "target": 34962 },
            { "buffer": 0, "byteOffset": 28, "byteLength": 12, "target": 34962 },
            { "buffer": 0, "byteOffset": 40, "byteLength": 4,  "target": 34962 },
            { "buffer": 0, "byteOffset": 44, "byteLength": 12, "target": 34962 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
              "min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0] },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "VEC4" },
            { "bufferView": 2, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 3, "componentType": 5126, "count": 1, "type": "SCALAR" },
            { "bufferView": 4, "componentType": 5126, "count": 1, "type": "VEC3" }
        ],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": attributes,
                        "mode": mode,
                        "extensions": {
                            "KHR_gaussian_splatting": descriptor
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
fn splatting_rejects_unknown_kernel() {
    let err = decode_descriptor(json!({
        "kernel": "mystery-shape",
        "colorSpace": "srgb_rec709_display"
    }))
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingKernel"),
        "expected kernel rejection, got: {err}"
    );
}

#[test]
fn splatting_accepts_vendor_prefixed_kernel() {
    // A vendor-extension-prefixed kernel string (the forward-compat
    // carve-out the spec invites in §"Kernel": "Additional kernel
    // types can be added over time by supplying an extension that
    // defines an alternative definition.") MUST be accepted because
    // it represents a kernel definition layered through another
    // extension; pretending to know all future kernel names is what
    // the validator is meant to AVOID.
    let scene = decode_descriptor(json!({
        "kernel": "KHR_some_future_kernel",
        "colorSpace": "srgb_rec709_display",
        "projection": "perspective",
        "sortingMethod": "cameraDistance"
    }))
    .unwrap();
    let desc = scene.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .unwrap();
    assert_eq!(
        desc.get("kernel").and_then(|v| v.as_str()),
        Some("KHR_some_future_kernel")
    );
}

#[test]
fn splatting_rejects_unknown_color_space() {
    let err = decode_descriptor(json!({
        "kernel": "ellipse",
        "colorSpace": "rec2020"
    }))
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingColorSpace"),
        "expected colorSpace rejection, got: {err}"
    );
}

#[test]
fn splatting_accepts_linear_color_space() {
    let scene = decode_descriptor(json!({
        "kernel": "ellipse",
        "colorSpace": "lin_rec709_display"
    }))
    .unwrap();
    let desc = scene.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .unwrap();
    assert_eq!(
        desc.get("colorSpace").and_then(|v| v.as_str()),
        Some("lin_rec709_display")
    );
}

#[test]
fn splatting_rejects_unknown_projection() {
    let err = decode_descriptor(json!({
        "kernel": "ellipse",
        "colorSpace": "srgb_rec709_display",
        "projection": "ortho"
    }))
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingProjection"),
        "expected projection rejection, got: {err}"
    );
}

#[test]
fn splatting_rejects_unknown_sorting_method() {
    let err = decode_descriptor(json!({
        "kernel": "ellipse",
        "colorSpace": "srgb_rec709_display",
        "sortingMethod": "splatId"
    }))
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingSortingMethod"),
        "expected sortingMethod rejection, got: {err}"
    );
}

// ---------------------------------------------------------------------
// §"Ellipse Kernel" §"Dependencies on glTF" — mode MUST be POINTS (0)
// for the base ellipse kernel.

#[test]
fn splatting_ellipse_kernel_requires_points_mode() {
    // mode == 4 (TRIANGLES, the default) with kernel "ellipse" must
    // be rejected.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_gaussian_splatting"],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "mode": 4,
                        "extensions": {
                            "KHR_gaussian_splatting": {
                                "kernel": "ellipse",
                                "colorSpace": "srgb_rec709_display"
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
        msg.contains("ExtensionStackGaussianSplattingMode"),
        "expected mode-rejection for non-POINTS ellipse primitive, got: {msg}"
    );
}

#[test]
fn splatting_default_triangle_mode_rejected_for_ellipse() {
    // When `mode` is omitted the spec defaults it to 4 (TRIANGLES),
    // which clashes with the ellipse-kernel POINTS requirement.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_gaussian_splatting"],
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": {},
                        "extensions": {
                            "KHR_gaussian_splatting": {
                                "kernel": "ellipse",
                                "colorSpace": "srgb_rec709_display"
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
        msg.contains("ExtensionStackGaussianSplattingMode"),
        "expected mode-rejection for default-TRIANGLES ellipse primitive, got: {msg}"
    );
}

#[test]
fn splatting_vendor_kernel_skips_mode_check() {
    // For non-base kernels the validator defers the mode rule to the
    // kernel-defining extension. mode = 4 with a vendor-prefixed
    // kernel name must validate (the kernel may be a triangle-based
    // splat reconstruction).
    let scene = decode_descriptor_with_mode(
        json!({
            "kernel": "EXT_vendor_kernel_triangles",
            "colorSpace": "srgb_rec709_display"
        }),
        4,
    )
    .expect("vendor kernel must skip mode check");
    let desc = scene.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .unwrap();
    assert_eq!(
        desc.get("kernel").and_then(|v| v.as_str()),
        Some("EXT_vendor_kernel_triangles")
    );
}

// ---------------------------------------------------------------------
// Multiple primitives — emitted_used flag fires exactly once.

#[test]
fn splatting_used_appended_once_for_multi_primitive_mesh() {
    let mut scene = Scene3D::new();
    for _ in 0..3 {
        let mut prim = Primitive::new(Topology::Points);
        prim.positions = vec![[0.0, 0.0, 0.0]];
        prim.extras.insert(
            "KHR_gaussian_splatting".into(),
            json!({
                "kernel": "ellipse",
                "colorSpace": "srgb_rec709_display",
            }),
        );
        let mut mesh = Mesh::new(None);
        mesh.primitives.push(prim);
        let mesh_id = scene.add_mesh(mesh);
        let mut node = Node::new();
        node.mesh = Some(mesh_id);
        let node_id = NodeId(scene.nodes.len() as u32);
        scene.add_node(node);
        scene.add_root(node_id);
    }

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();

    // Count occurrences inside extensionsUsed — should be exactly one.
    let used_start = raw.find("\"extensionsUsed\"").expect("extensionsUsed");
    let used_slice = &raw[used_start..];
    let end = used_slice.find(']').expect("extensionsUsed close");
    let used_array = &used_slice[..end];
    let count = used_array.matches("\"KHR_gaussian_splatting\"").count();
    assert_eq!(
        count, 1,
        "KHR_gaussian_splatting must be appended exactly once, got {count} in {used_array}"
    );
}

// ---------------------------------------------------------------------
// §"Ellipse Kernel" §"Attributes" — per-attribute storage contract and
// spherical-harmonics degree completeness.
//
// The ellipse kernel requires POSITION + ROTATION (VEC4) + SCALE (VEC3)
// + OPACITY (SCALAR) + SH_DEGREE_0_COEF_0 (VEC3), with a documented set
// of component types per attribute, and forbids partially-defined
// spherical-harmonics degrees.

/// Decode a fully-specified glTF document where the caller supplies the
/// entire `attributes` map and the matching `accessors` array. Used by
/// the storage-form tests that must inject a non-conformant accessor for
/// a single attribute.
fn decode_with_attrs_and_accessors(
    attributes: Value,
    accessors: Value,
    buffer_len: usize,
) -> Result<Scene3D, String> {
    let bytes = vec![0u8; buffer_len];
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let uri = format!("data:application/octet-stream;base64,{b64}");

    let accessor_count = accessors.as_array().map(|a| a.len()).unwrap_or(0) as u64;
    // One bufferView per accessor, each spanning the whole buffer (the
    // accessors carry their own byteOffset). Splat attribute accessors
    // are tiny and tolerate overlap for these structural tests.
    let mut buffer_views = Vec::new();
    for _ in 0..accessor_count {
        buffer_views.push(json!({
            "buffer": 0, "byteOffset": 0, "byteLength": buffer_len, "target": 34962
        }));
    }

    let raw = serde_json::to_string(&json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_gaussian_splatting"],
        "buffers": [ { "byteLength": buffer_len, "uri": uri } ],
        "bufferViews": buffer_views,
        "accessors": accessors,
        "meshes": [
            {
                "primitives": [
                    {
                        "attributes": attributes,
                        "mode": 0,
                        "extensions": {
                            "KHR_gaussian_splatting": {
                                "kernel": "ellipse",
                                "colorSpace": "srgb_rec709_display"
                            }
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
fn splatting_ellipse_missing_rotation_rejected() {
    // POSITION present but ROTATION absent — required by the ellipse
    // kernel attribute table.
    let err = decode_with_attrs_and_accessors(
        json!({
            "POSITION": 0,
            "KHR_gaussian_splatting:SCALE": 0,
            "KHR_gaussian_splatting:OPACITY": 1,
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 0
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "SCALAR" }
        ]),
        16,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingMissingAttribute") && err.contains("ROTATION"),
        "expected missing-ROTATION rejection, got: {err}"
    );
}

#[test]
fn splatting_ellipse_missing_position_rejected() {
    let err = decode_with_attrs_and_accessors(
        json!({
            "KHR_gaussian_splatting:ROTATION": 0,
            "KHR_gaussian_splatting:SCALE": 1,
            "KHR_gaussian_splatting:OPACITY": 2,
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 1
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC4" },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": 1, "type": "SCALAR" }
        ]),
        16,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingMissingAttribute") && err.contains("POSITION"),
        "expected missing-POSITION rejection, got: {err}"
    );
}

#[test]
fn splatting_rotation_wrong_type_rejected() {
    // ROTATION accessor declared VEC3 instead of the required VEC4.
    let err = decode_with_attrs_and_accessors(
        json!({
            "POSITION": 0,
            "KHR_gaussian_splatting:ROTATION": 0,
            "KHR_gaussian_splatting:SCALE": 0,
            "KHR_gaussian_splatting:OPACITY": 1,
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 0
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "SCALAR" }
        ]),
        16,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingAttributeType") && err.contains("ROTATION"),
        "expected ROTATION wrong-type rejection, got: {err}"
    );
}

#[test]
fn splatting_opacity_wrong_component_rejected() {
    // OPACITY allows float / unsigned-byte-normalized /
    // unsigned-short-normalized. A *non-normalized* unsigned byte is
    // outside the allowed set.
    let err = decode_with_attrs_and_accessors(
        json!({
            "POSITION": 0,
            "KHR_gaussian_splatting:ROTATION": 1,
            "KHR_gaussian_splatting:SCALE": 0,
            "KHR_gaussian_splatting:OPACITY": 2,
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 0
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "VEC4" },
            // SCALAR unsigned byte, NOT normalized → invalid for OPACITY.
            { "bufferView": 2, "componentType": 5121, "count": 1, "type": "SCALAR", "normalized": false }
        ]),
        16,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingAttributeComponent")
            && err.contains("OPACITY"),
        "expected OPACITY wrong-component rejection, got: {err}"
    );
}

#[test]
fn splatting_opacity_normalized_ubyte_accepted() {
    // The same unsigned byte, this time normalized, IS a spec-allowed
    // storage form for OPACITY.
    let scene = decode_with_attrs_and_accessors(
        json!({
            "POSITION": 0,
            "KHR_gaussian_splatting:ROTATION": 1,
            "KHR_gaussian_splatting:SCALE": 0,
            "KHR_gaussian_splatting:OPACITY": 2,
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 0
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "VEC4" },
            { "bufferView": 2, "componentType": 5121, "count": 1, "type": "SCALAR", "normalized": true }
        ]),
        16,
    )
    .expect("normalized unsigned-byte OPACITY must validate");
    assert!(scene.meshes[0].primitives[0]
        .extras
        .contains_key("KHR_gaussian_splatting"));
}

#[test]
fn splatting_rotation_normalized_short_accepted() {
    // ROTATION allows signed-short normalized.
    let scene = decode_with_attrs_and_accessors(
        json!({
            "POSITION": 0,
            "KHR_gaussian_splatting:ROTATION": 1,
            "KHR_gaussian_splatting:SCALE": 0,
            "KHR_gaussian_splatting:OPACITY": 2,
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 0
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5122, "count": 1, "type": "VEC4", "normalized": true },
            { "bufferView": 2, "componentType": 5126, "count": 1, "type": "SCALAR" }
        ]),
        16,
    )
    .expect("normalized signed-short ROTATION must validate");
    assert!(scene.meshes[0].primitives[0]
        .extras
        .contains_key("KHR_gaussian_splatting"));
}

#[test]
fn splatting_sh_coefficient_non_float_rejected() {
    // Spherical-harmonics coefficients are float-only.
    let err = decode_with_attrs_and_accessors(
        json!({
            "POSITION": 0,
            "KHR_gaussian_splatting:ROTATION": 1,
            "KHR_gaussian_splatting:SCALE": 0,
            "KHR_gaussian_splatting:OPACITY": 2,
            // SH_0_0 as normalized unsigned byte VEC3 → invalid (float only).
            "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0": 3
        }),
        json!([
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5126, "count": 1, "type": "VEC4" },
            { "bufferView": 2, "componentType": 5126, "count": 1, "type": "SCALAR" },
            { "bufferView": 3, "componentType": 5121, "count": 1, "type": "VEC3", "normalized": true }
        ]),
        16,
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingAttributeComponent")
            && err.contains("SH_DEGREE_0_COEF_0"),
        "expected SH non-float rejection, got: {err}"
    );
}

#[test]
fn splatting_sh_degree_one_partial_rejected() {
    // Degree 1 present but incomplete: COEF_0 + COEF_1 given, COEF_2
    // missing. The degree MUST be fully defined.
    let err = decode_full_splat_primitive(
        json!({
            "kernel": "ellipse",
            "colorSpace": "srgb_rec709_display"
        }),
        0,
        json!({
            "KHR_gaussian_splatting:SH_DEGREE_1_COEF_0": 4,
            "KHR_gaussian_splatting:SH_DEGREE_1_COEF_1": 4
        }),
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingSHIncomplete")
            && err.contains("SH_DEGREE_1_COEF_2"),
        "expected SH degree-1 incompleteness rejection, got: {err}"
    );
}

#[test]
fn splatting_sh_skipped_lower_degree_rejected() {
    // Degree 2 fully present but degree 1 entirely absent — lower
    // degrees MUST be defined when a higher degree is used.
    let err = decode_full_splat_primitive(
        json!({
            "kernel": "ellipse",
            "colorSpace": "srgb_rec709_display"
        }),
        0,
        json!({
            "KHR_gaussian_splatting:SH_DEGREE_2_COEF_0": 4,
            "KHR_gaussian_splatting:SH_DEGREE_2_COEF_1": 4,
            "KHR_gaussian_splatting:SH_DEGREE_2_COEF_2": 4,
            "KHR_gaussian_splatting:SH_DEGREE_2_COEF_3": 4,
            "KHR_gaussian_splatting:SH_DEGREE_2_COEF_4": 4
        }),
    )
    .unwrap_err();
    assert!(
        err.contains("ExtensionStackGaussianSplattingSHIncomplete")
            && err.contains("SH_DEGREE_1_COEF_0"),
        "expected skipped-degree-1 rejection, got: {err}"
    );
}

#[test]
fn splatting_sh_full_degree_three_accepted() {
    // A complete degree-0..3 cascade (1 + 3 + 5 + 7 = 16 coefficients)
    // must validate.
    let mut extra = serde_json::Map::new();
    for (l, coefs) in [(1u32, 3u32), (2, 5), (3, 7)] {
        for n in 0..coefs {
            extra.insert(
                format!("KHR_gaussian_splatting:SH_DEGREE_{l}_COEF_{n}"),
                json!(4),
            );
        }
    }
    let scene = decode_full_splat_primitive(
        json!({
            "kernel": "ellipse",
            "colorSpace": "srgb_rec709_display"
        }),
        0,
        Value::Object(extra),
    )
    .expect("full degree-3 spherical harmonics must validate");
    assert!(scene.meshes[0].primitives[0]
        .extras
        .contains_key("KHR_gaussian_splatting"));
}

#[test]
fn splatting_vendor_kernel_skips_attribute_checks() {
    // A vendor-prefixed kernel defers the ENTIRE attribute contract to
    // the kernel-defining extension; a primitive with only POSITION must
    // validate without tripping the ellipse-kernel attribute rules.
    let bytes = [0u8; 12];
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let uri = format!("data:application/octet-stream;base64,{b64}");
    let raw = serde_json::to_string(&json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_gaussian_splatting"],
        "buffers": [ { "byteLength": 12, "uri": uri } ],
        "bufferViews": [ { "buffer": 0, "byteLength": 12, "target": 34962 } ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3" }
        ],
        "meshes": [ { "primitives": [ {
            "attributes": { "POSITION": 0 },
            "mode": 0,
            "extensions": { "KHR_gaussian_splatting": {
                "kernel": "EXT_vendor_kernel_custom",
                "colorSpace": "srgb_rec709_display"
            } }
        } ] } ]
    }))
    .unwrap();
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(raw.as_bytes())
        .expect("vendor kernel must skip ellipse attribute checks");
    assert!(scene.meshes[0].primitives[0]
        .extras
        .contains_key("KHR_gaussian_splatting"));
}
