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
    let mut scene = make_points_scene();
    scene.meshes[0].primitives[0].extras.insert(
        "KHR_gaussian_splatting".into(),
        json!({
            "kernel": "ellipse",
            "colorSpace": "srgb_rec709_display",
            "projection": "perspective",
            "sortingMethod": "cameraDistance",
        }),
    );

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    let desc = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .expect("descriptor must round-trip");
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

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    let desc = decoded.meshes[0].primitives[0]
        .extras
        .get("KHR_gaussian_splatting")
        .expect("descriptor must round-trip");
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
/// referencing a one-element POSITION accessor (three zero floats in a
/// 12-byte base64 buffer) so the descriptor walk can run through
/// `convert_primitive` and surface §3.12 validation errors instead of
/// the missing-POSITION early-exit.
fn decode_descriptor_with_mode(descriptor: Value, mode: u64) -> Result<Scene3D, String> {
    // 12-byte buffer (one VEC3 float at origin) → base64.
    let bytes = [0u8; 12];
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let uri = format!("data:application/octet-stream;base64,{b64}");
    let raw = serde_json::to_string(&json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_gaussian_splatting"],
        "buffers": [ { "byteLength": 12, "uri": uri } ],
        "bufferViews": [ { "buffer": 0, "byteLength": 12, "target": 34962 } ],
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
