//! Translate parsed glTF JSON + (optional) buffers → [`Scene3D`].
//!
//! The decoder is forgiving:
//!
//! * Buffers without a URI on a `.glb` are pointed at the BIN chunk;
//!   buffers with a `data:` URI are decoded inline (base64 only — the
//!   only encoding the spec defines for `data:` URIs in glTF).
//! * Buffers with a relative URI (`textures/foo.bin`) surface as
//!   [`oxideav_mesh3d::ImageData::External`] only when they back an
//!   image. For accessor-backing buffers the same buffer is needed
//!   eagerly; when the URI is unresolvable we return
//!   [`Error::Unsupported`] rather than guessing.
//! * Sparse accessors are decoded by materialising the base buffer
//!   and overlaying the per-index overrides per spec §3.6.2.3
//!   (round 2). The encoder always emits dense storage on the way
//!   back out.

use std::collections::HashMap;
use std::sync::Arc;

use oxideav_mesh3d::{
    AlphaMode, Animation, AnimationChannel, AnimationProperty, AnimationSampler, AnimationTarget,
    AnimationValues, Camera, ImageData, Indices, Interpolation, Light, MagFilter, Material,
    MaterialId, Mesh, MinFilter, Node, NodeId, Primitive, Sampler, Scene3D, Skeleton, Skin, SkinId,
    Texture, TextureId, TextureRef, Topology, Transform, WrapMode,
};
use serde_json::Value;

use crate::accessor::{
    materialise_accessor, read_indices_u32, read_mat4_f32, read_scalar_f32, read_vec4_u16,
    read_vec_f32, view_from_materialised, AccessorView,
};
use crate::asset_source::BufferViewAsset;
use crate::error::{invalid, unsupported, Error, Result};
use crate::json_model::{self as gj, GltfRoot};
use crate::validation::{
    check_asset_version, validate_accessor_fits_bufferview, validate_alignment,
    validate_animation_channels, validate_attribute_counts, validate_bufferview_fits_buffer,
    validate_color0_range, validate_extension_stack, validate_index_no_restart,
    validate_sparse_indices_buffer_views, validate_tangent_w,
};

/// Decode a parsed [`GltfRoot`] into a [`Scene3D`], using `glb_bin`
/// (when present) as the backing buffer for buffers with no URI.
pub fn convert(root: &GltfRoot, glb_bin: Option<&[u8]>) -> Result<Scene3D> {
    // Spec §3.2 + §5.9 — asset.version MUST follow <major>.<minor>;
    // asset.minVersion (when present) MUST NOT exceed asset.version and
    // MUST be within an edition this decoder implements. Round 8
    // replaces the old `version == "2.0"` exact-string check that
    // wrongly rejected forward-compatible 2.1 assets that only used 2.0
    // features.
    check_asset_version(&root.asset)?;

    // Spec §3.12 — extensionsUsed / extensionsRequired stack must be
    // self-consistent; any extension whose data block appears in the
    // document MUST be declared in extensionsUsed.
    validate_extension_stack(root)?;

    // Spec §3.11 — every animation channel must point at a known
    // path; sampler indices must resolve; "weights" channels must
    // target a node bound to a mesh with morph targets. Validate
    // before buffer materialisation so the failure surfaces early.
    for (i, a) in root.animations.iter().enumerate() {
        validate_animation_channels(i, a, &root.nodes, &root.meshes, &root.accessors)?;
    }

    // Spec §5.11 — every bufferView MUST fit inside the buffer it
    // points into; bufferView.byteStride (when defined) MUST be in
    // [4, 252] per the JSON schema §5.11.4.
    for (i, bv) in root.buffer_views.iter().enumerate() {
        validate_bufferview_fits_buffer(i, bv, &root.buffers)?;
    }
    // Spec §3.6.2.4 line 3104 — every accessor MUST fit inside the
    // bufferView it points into (EFFECTIVE_BYTE_STRIDE * (count - 1) +
    // element size + byteOffset <= bufferView.byteLength). Covers
    // tightly-packed and strided layouts.
    for (i, acc) in root.accessors.iter().enumerate() {
        validate_accessor_fits_bufferview(i, acc, &root.buffer_views)?;
    }
    // Spec §5.3.1 — accessor.sparse.indices.bufferView MUST NOT carry
    // `target` or `byteStride` properties.
    validate_sparse_indices_buffer_views(&root.accessors, &root.buffer_views)?;

    let buffers = resolve_buffers(root, glb_bin)?;
    let mut scene = Scene3D::new();

    // Materials first — meshes need the IDs.
    let mut material_id_map: HashMap<u32, MaterialId> = HashMap::new();
    // Textures first — materials reference them by index.
    let mut texture_id_map: HashMap<u32, TextureId> = HashMap::new();
    for (i, t) in root.textures.iter().enumerate() {
        let id = scene.add_texture(convert_texture(root, t, &buffers)?);
        texture_id_map.insert(i as u32, id);
    }

    for (i, m) in root.materials.iter().enumerate() {
        let id = scene.add_material(convert_material(m, &texture_id_map)?);
        material_id_map.insert(i as u32, id);
    }

    // Meshes
    let mut mesh_id_map: HashMap<u32, oxideav_mesh3d::MeshId> = HashMap::new();
    for (i, m) in root.meshes.iter().enumerate() {
        let mut mesh = Mesh::new(m.name.clone());
        mesh.primitives.reserve(m.primitives.len());
        for p in &m.primitives {
            mesh.primitives
                .push(convert_primitive(root, p, &buffers, &material_id_map)?);
        }
        // Mesh-level `extras` (glTF allows it but mesh3d's Mesh has no
        // extras field): stash on the first primitive under the
        // sentinel key `__mesh_extras` so the encoder can lift it back
        // up. No primitives → drop silently.
        if let (Some(extras), Some(prim0)) = (&m.extras, mesh.primitives.first_mut()) {
            prim0
                .extras
                .insert("__mesh_extras".to_owned(), extras.clone());
        }
        // Mesh-level `weights` (default morph weights per §3.7.2.2):
        // mesh3d's `Mesh` has no `weights` field, so stash on
        // primitive[0].extras["__mesh_weights"] in the same style as
        // the morph-targets sentinel.
        if let (Some(weights), Some(prim0)) = (&m.weights, mesh.primitives.first_mut()) {
            let arr: Vec<serde_json::Value> = weights
                .iter()
                .map(|&w| {
                    serde_json::Number::from_f64(w as f64)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect();
            prim0
                .extras
                .insert("__mesh_weights".to_owned(), serde_json::Value::Array(arr));
        }
        let id = scene.add_mesh(mesh);
        mesh_id_map.insert(i as u32, id);
    }

    // Cameras
    let mut camera_id_map: HashMap<u32, oxideav_mesh3d::CameraId> = HashMap::new();
    for (i, c) in root.cameras.iter().enumerate() {
        let id = scene.add_camera(convert_camera(c)?);
        camera_id_map.insert(i as u32, id);
    }

    // Lights — KHR_lights_punctual root extension.
    let mut light_id_map: HashMap<u32, oxideav_mesh3d::LightId> = HashMap::new();
    if let Some(ext) = &root.extensions {
        if let Some(lp) = &ext.khr_lights_punctual {
            for (i, l) in lp.lights.iter().enumerate() {
                let id = scene.add_light(convert_light(l)?);
                light_id_map.insert(i as u32, id);
            }
        }
    }

    // Skins — convert before nodes so node.skin can reference SkinIds.
    let mut skin_id_map: HashMap<u32, SkinId> = HashMap::new();
    for (i, s) in root.skins.iter().enumerate() {
        let id = convert_skin(s, root, &buffers, &mut scene)?;
        skin_id_map.insert(i as u32, id);
    }

    // Nodes — pre-allocate so child references resolve.
    for n in &root.nodes {
        let mut node = Node::new();
        node.name = n.name.clone();
        node.transform = node_transform(n);
        for &c in &n.children {
            node.children.push(NodeId(c));
        }
        if let Some(m) = n.mesh {
            node.mesh = mesh_id_map.get(&m).copied();
        }
        if let Some(c) = n.camera {
            node.camera = camera_id_map.get(&c).copied();
        }
        if let Some(s) = n.skin {
            node.skin = skin_id_map.get(&s).copied();
        }
        if let Some(ext) = &n.extensions {
            if let Some(lr) = &ext.khr_lights_punctual {
                node.light = light_id_map.get(&lr.light).copied();
            }
        }
        if let Some(extras) = &n.extras {
            extras_into(&mut node.extras, extras.clone());
        }
        scene.add_node(node);
    }

    // Animations — channels target NodeIds, so resolve after nodes are loaded.
    for a in &root.animations {
        scene.add_animation(convert_animation(a, root, &buffers)?);
    }

    // Roots — explicit `scenes[scene].nodes` if any, otherwise treat
    // every top-level (non-child) node as a root.
    let scene_idx = root.scene.unwrap_or(0) as usize;
    if let Some(s) = root.scenes.get(scene_idx) {
        for &r in &s.nodes {
            scene.add_root(NodeId(r));
        }
    } else if !root.nodes.is_empty() && root.scenes.is_empty() {
        // Synthesize: any node not referenced as a child is a root.
        let mut child_set = std::collections::HashSet::<u32>::new();
        for n in &root.nodes {
            for &c in &n.children {
                child_set.insert(c);
            }
        }
        for i in 0..root.nodes.len() as u32 {
            if !child_set.contains(&i) {
                scene.add_root(NodeId(i));
            }
        }
    }

    // Multi-scene preservation: when more than one scene is present we
    // load the active one as the live scene-graph; secondary scenes (the
    // node-id rosters + names) round-trip through `scene.extras` so the
    // encoder can re-emit them. Spec §5.27 lets a glTF asset host
    // several `scenes[]` and pick one as default via top-level `scene`.
    if root.scenes.len() > 1 {
        let secondary: Vec<serde_json::Value> = root
            .scenes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != scene_idx)
            .map(|(i, s)| {
                let nodes: Vec<serde_json::Value> = s
                    .nodes
                    .iter()
                    .map(|&n| serde_json::Value::from(n))
                    .collect();
                let mut obj = serde_json::Map::new();
                obj.insert("__index".into(), serde_json::Value::from(i as u32));
                obj.insert("nodes".into(), serde_json::Value::Array(nodes));
                if let Some(name) = &s.name {
                    obj.insert("name".into(), serde_json::Value::String(name.clone()));
                }
                if let Some(extras) = &s.extras {
                    obj.insert("extras".into(), extras.clone());
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        scene.extras.insert(
            "__additional_scenes".into(),
            serde_json::Value::Array(secondary),
        );
    }

    if let Some(extras) = &root.extras {
        extras_into(&mut scene.extras, extras.clone());
    }

    Ok(scene)
}

// --- skin / animation converters ----------------------------------------

fn convert_skin(
    s: &gj::Skin,
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    scene: &mut Scene3D,
) -> Result<SkinId> {
    // Build the skeleton: joint NodeIds + IBM matrices.
    let mut skeleton = Skeleton::new();
    skeleton.name = s.name.clone();
    skeleton.joints = s.joints.iter().map(|&n| NodeId(n)).collect();
    if let Some(ibm_idx) = s.inverse_bind_matrices {
        let acc = root
            .accessors
            .get(ibm_idx as usize)
            .ok_or_else(|| invalid(format!("skin: inverseBindMatrices accessor {ibm_idx} oob")))?;
        if acc.kind != "MAT4" || acc.component_type != gj::COMPONENT_TYPE_FLOAT {
            return Err(unsupported(format!(
                "skin.inverseBindMatrices must be MAT4 FLOAT, got {:?} {}",
                acc.kind, acc.component_type
            )));
        }
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        skeleton.inverse_bind_matrices = read_mat4_f32(&view)?;
    }
    let skel_id = scene.add_skeleton(skeleton);
    let mut skin = Skin::new(skel_id);
    if let Some(root_node) = s.skeleton {
        skin = skin.with_root(NodeId(root_node));
    }
    Ok(scene.add_skin(skin))
}

fn convert_animation(
    a: &gj::Animation,
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
) -> Result<Animation> {
    let mut anim = Animation::new(a.name.clone());
    anim.channels.reserve(a.channels.len());
    for ch in &a.channels {
        let target_node = match ch.target.node {
            Some(n) => NodeId(n),
            // Spec §3.11: "When node isn't defined, channel SHOULD be ignored."
            None => continue,
        };
        let property = match ch.target.path.as_str() {
            "translation" => AnimationProperty::Translation,
            "rotation" => AnimationProperty::Rotation,
            "scale" => AnimationProperty::Scale,
            "weights" => AnimationProperty::MorphWeights,
            other => {
                return Err(unsupported(format!(
                    "animation channel path {other:?} unknown"
                )));
            }
        };
        let s_idx = ch.sampler as usize;
        let s = a
            .samplers
            .get(s_idx)
            .ok_or_else(|| invalid(format!("animation: sampler {s_idx} out of range")))?;
        let interpolation = match s.interpolation.as_deref() {
            None | Some("LINEAR") => Interpolation::Linear,
            Some("STEP") => Interpolation::Step,
            Some("CUBICSPLINE") => Interpolation::CubicSpline,
            Some(other) => {
                return Err(unsupported(format!(
                    "animation sampler interpolation {other:?} unknown"
                )));
            }
        };
        // Input — keyframe times, SCALAR FLOAT per spec table.
        let input_acc = root
            .accessors
            .get(s.input as usize)
            .ok_or_else(|| invalid(format!("animation sampler input {} oob", s.input)))?;
        if input_acc.kind != "SCALAR" || input_acc.component_type != gj::COMPONENT_TYPE_FLOAT {
            return Err(unsupported(format!(
                "animation input accessor must be SCALAR FLOAT, got {:?} {}",
                input_acc.kind, input_acc.component_type
            )));
        }
        let in_bytes = materialise_accessor(input_acc, &root.buffer_views, buffers)?;
        let in_view = view_from_materialised(input_acc, &in_bytes)?;
        let keyframes = read_scalar_f32(&in_view)?;
        // Output — type depends on path. FLOAT components decode
        // directly; for ROTATION (VEC4) and MORPH_WEIGHTS (SCALAR) the
        // spec also permits the four normalised-integer
        // component-types (BYTE / UBYTE / SHORT / USHORT) — see spec
        // §3.11 animation-sampler-output table. They MUST carry
        // `normalized: true` and dequantise via the equations in
        // §3.6.2.2 (e.g. ubyte: f = c/255.0).
        let output_acc = root
            .accessors
            .get(s.output as usize)
            .ok_or_else(|| invalid(format!("animation sampler output {} oob", s.output)))?;
        let out_bytes = materialise_accessor(output_acc, &root.buffer_views, buffers)?;
        let out_view = view_from_materialised(output_acc, &out_bytes)?;
        let values = decode_animation_output(property, output_acc, &out_view)?;
        anim.channels.push(AnimationChannel {
            target: AnimationTarget {
                node: target_node,
                property,
            },
            sampler: AnimationSampler {
                keyframes,
                values,
                interpolation,
            },
        });
    }
    Ok(anim)
}

/// Decode an animation sampler's output accessor into the typed
/// [`AnimationValues`] variant for `property`. Handles the FLOAT
/// path plus the four normalised-int component-types the spec allows
/// for ROTATION and MORPH_WEIGHTS outputs.
fn decode_animation_output(
    property: AnimationProperty,
    acc: &gj::Accessor,
    view: &AccessorView<'_>,
) -> Result<AnimationValues> {
    use gj::{
        COMPONENT_TYPE_BYTE, COMPONENT_TYPE_FLOAT, COMPONENT_TYPE_SHORT,
        COMPONENT_TYPE_UNSIGNED_BYTE, COMPONENT_TYPE_UNSIGNED_SHORT,
    };

    // Spec §3.11: per the channel-target / accessor-format table,
    // ROTATION VEC4 and MORPH_WEIGHTS SCALAR may use one of:
    //   FLOAT (5126) — direct
    //   BYTE (5120) normalized — f = max(c/127, -1)
    //   UBYTE (5121) normalized — f = c/255
    //   SHORT (5122) normalized — f = max(c/32767, -1)
    //   USHORT (5123) normalized — f = c/65535
    // TRANSLATION + SCALE only allow FLOAT.
    let is_normalized_int = matches!(
        acc.component_type,
        COMPONENT_TYPE_BYTE
            | COMPONENT_TYPE_UNSIGNED_BYTE
            | COMPONENT_TYPE_SHORT
            | COMPONENT_TYPE_UNSIGNED_SHORT
    );
    if is_normalized_int && !acc.normalized {
        return Err(invalid(format!(
            "animation output: integer componentType {} requires `normalized: true`",
            acc.component_type
        )));
    }

    match (property, acc.kind.as_str(), acc.component_type) {
        (
            AnimationProperty::Translation | AnimationProperty::Scale,
            "VEC3",
            COMPONENT_TYPE_FLOAT,
        ) => Ok(AnimationValues::Vec3(read_vec_f32::<3>(view)?)),
        (AnimationProperty::Translation | AnimationProperty::Scale, _, ct)
            if ct != COMPONENT_TYPE_FLOAT =>
        {
            Err(unsupported(format!(
                "animation output: TRANSLATION/SCALE only allow FLOAT (5126), got componentType {ct}"
            )))
        }
        (AnimationProperty::Rotation, "VEC4", COMPONENT_TYPE_FLOAT) => {
            Ok(AnimationValues::Quat(read_vec_f32::<4>(view)?))
        }
        (AnimationProperty::Rotation, "VEC4", ct) if is_normalized_int => {
            Ok(AnimationValues::Quat(read_normalized_vec4(view, ct)?))
        }
        (AnimationProperty::MorphWeights, "SCALAR", COMPONENT_TYPE_FLOAT) => {
            Ok(AnimationValues::Scalar(read_scalar_f32(view)?))
        }
        (AnimationProperty::MorphWeights, "SCALAR", ct) if is_normalized_int => {
            Ok(AnimationValues::Scalar(read_normalized_scalar(view, ct)?))
        }
        (p, k, ct) => Err(invalid(format!(
            "animation channel: path {p:?} incompatible with output type {k:?} componentType {ct}"
        ))),
    }
}

/// Dequantise a SCALAR normalised-integer accessor view into `Vec<f32>`
/// per spec §3.6.2.2 equations.
fn read_normalized_scalar(view: &AccessorView<'_>, component_type: u32) -> Result<Vec<f32>> {
    let expected = component_byte_size(component_type)?;
    if view.element_size != expected {
        return Err(invalid(format!(
            "normalized scalar accessor: element size {} != {expected}",
            view.element_size
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    for elem in view.elements() {
        out.push(decode_normalized_component(component_type, elem)?);
    }
    Ok(out)
}

/// Dequantise a VEC4 normalised-integer accessor view into `Vec<[f32; 4]>`.
fn read_normalized_vec4(view: &AccessorView<'_>, component_type: u32) -> Result<Vec<[f32; 4]>> {
    let csize = component_byte_size(component_type)?;
    let expected = 4 * csize;
    if view.element_size != expected {
        return Err(invalid(format!(
            "normalized vec4 accessor: element size {} != {expected}",
            view.element_size
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    for elem in view.elements() {
        let mut a = [0.0f32; 4];
        for (i, slot) in a.iter_mut().enumerate() {
            *slot = decode_normalized_component(component_type, &elem[i * csize..(i + 1) * csize])?;
        }
        out.push(a);
    }
    Ok(out)
}

fn component_byte_size(component_type: u32) -> Result<usize> {
    use gj::{
        COMPONENT_TYPE_BYTE, COMPONENT_TYPE_SHORT, COMPONENT_TYPE_UNSIGNED_BYTE,
        COMPONENT_TYPE_UNSIGNED_SHORT,
    };
    Ok(match component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        other => {
            return Err(invalid(format!(
                "normalized accessor: componentType {other} not allowed"
            )));
        }
    })
}

/// Decode one component per spec §3.6.2.2 dequantisation table.
fn decode_normalized_component(component_type: u32, bytes: &[u8]) -> Result<f32> {
    use gj::{
        COMPONENT_TYPE_BYTE, COMPONENT_TYPE_SHORT, COMPONENT_TYPE_UNSIGNED_BYTE,
        COMPONENT_TYPE_UNSIGNED_SHORT,
    };
    Ok(match component_type {
        COMPONENT_TYPE_BYTE => {
            // i8 normalized: f = max(c / 127, -1)
            let c = i8::from_le_bytes([bytes[0]]) as f32;
            (c / 127.0).max(-1.0)
        }
        COMPONENT_TYPE_UNSIGNED_BYTE => {
            // u8 normalized: f = c / 255
            (bytes[0] as f32) / 255.0
        }
        COMPONENT_TYPE_SHORT => {
            // i16 normalized: f = max(c / 32767, -1)
            let c = i16::from_le_bytes([bytes[0], bytes[1]]) as f32;
            (c / 32767.0).max(-1.0)
        }
        COMPONENT_TYPE_UNSIGNED_SHORT => {
            // u16 normalized: f = c / 65535
            let c = u16::from_le_bytes([bytes[0], bytes[1]]) as f32;
            c / 65535.0
        }
        other => {
            return Err(invalid(format!(
                "normalized componentType {other} unsupported here"
            )));
        }
    })
}

// --- buffer / accessor resolution ------------------------------------------

/// Materialise every glTF buffer into an `Arc<Vec<u8>>` so accessors
/// can borrow from a stable owner. The first buffer of a `.glb` reuses
/// the BIN-chunk slice; `data:` URI buffers decode base64 inline;
/// external file URIs are unsupported here (caller-resolved).
fn resolve_buffers(root: &GltfRoot, glb_bin: Option<&[u8]>) -> Result<Vec<Arc<Vec<u8>>>> {
    let mut out = Vec::with_capacity(root.buffers.len());
    for (i, b) in root.buffers.iter().enumerate() {
        let bytes = match &b.uri {
            None => {
                // `.glb` BIN chunk — only legal for buffer 0.
                if i != 0 {
                    return Err(invalid(format!(
                        "buffer[{i}] has no uri but is not buffer 0 (only the .glb BIN chunk may be uri-less)"
                    )));
                }
                let bin = glb_bin.ok_or_else(|| {
                    invalid("buffer[0] is uri-less but no .glb BIN chunk was provided")
                })?;
                if bin.len() < b.byte_length as usize {
                    return Err(invalid(format!(
                        "buffer[0]: BIN chunk has {} bytes < declared byteLength {}",
                        bin.len(),
                        b.byte_length
                    )));
                }
                bin[..b.byte_length as usize].to_vec()
            }
            Some(uri) if uri.starts_with("data:") => decode_data_uri(uri)?,
            Some(uri) => {
                return Err(unsupported(format!(
                    "buffer[{i}]: external URI {uri:?} not resolved (caller must inline before decode)"
                )));
            }
        };
        out.push(Arc::new(bytes));
    }
    Ok(out)
}

/// Decode a `data:[<mediatype>][;base64],<data>` URI. We only support
/// `;base64,`; the spec allows raw textual data but it's never used
/// for buffers in practice.
pub(crate) fn decode_data_uri(uri: &str) -> Result<Vec<u8>> {
    let body = uri
        .strip_prefix("data:")
        .ok_or_else(|| invalid(format!("data URI missing scheme: {uri}")))?;
    let comma = body
        .find(',')
        .ok_or_else(|| invalid(format!("data URI missing comma: {uri}")))?;
    let header = &body[..comma];
    let payload = &body[comma + 1..];
    if !header.ends_with(";base64") {
        return Err(unsupported(format!(
            "data URI without ;base64 not supported: {header:?}"
        )));
    }
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| invalid(format!("data URI base64 decode failed: {e}")))
}

/// Pull the MIME hint out of a `data:` URI (the `image/png` part).
pub(crate) fn data_uri_mime(uri: &str) -> Option<String> {
    let body = uri.strip_prefix("data:")?;
    let comma = body.find(',')?;
    let header = &body[..comma];
    let mime = header.strip_suffix(";base64").unwrap_or(header);
    if mime.is_empty() {
        None
    } else {
        Some(mime.to_owned())
    }
}

// --- per-element converters ------------------------------------------------

fn node_transform(n: &gj::Node) -> Transform {
    if let Some(m) = n.matrix {
        // glTF matrix is column-major: m[c*4 + r]. Convert to our
        // row-major-of-columns `[[f32;4];4]` layout.
        let mut row_major = [[0.0f32; 4]; 4];
        for c in 0..4 {
            for r in 0..4 {
                row_major[r][c] = m[c * 4 + r];
            }
        }
        return Transform::Matrix(row_major);
    }
    let translation = n.translation.unwrap_or([0.0; 3]);
    let rotation = n.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let scale = n.scale.unwrap_or([1.0; 3]);
    Transform::Trs {
        translation,
        rotation,
        scale,
    }
}

fn convert_camera(c: &gj::Camera) -> Result<Camera> {
    match c.kind.as_str() {
        "perspective" => {
            let p = c
                .perspective
                .as_ref()
                .ok_or_else(|| invalid("camera.type=perspective but perspective block missing"))?;
            Ok(Camera::Perspective {
                aspect_ratio: p.aspect_ratio,
                yfov: p.yfov,
                znear: p.znear,
                zfar: p.zfar,
            })
        }
        "orthographic" => {
            let o = c.orthographic.as_ref().ok_or_else(|| {
                invalid("camera.type=orthographic but orthographic block missing")
            })?;
            Ok(Camera::Orthographic {
                xmag: o.xmag,
                ymag: o.ymag,
                znear: o.znear,
                zfar: o.zfar,
            })
        }
        other => Err(invalid(format!("camera.type {other:?} unknown"))),
    }
}

fn convert_light(l: &gj::KhrLight) -> Result<Light> {
    let color = l.color.unwrap_or([1.0; 3]);
    let intensity = l.intensity.unwrap_or(1.0);
    match l.kind.as_str() {
        "directional" => Ok(Light::Directional { color, intensity }),
        "point" => Ok(Light::Point {
            color,
            intensity,
            range: l.range,
        }),
        "spot" => {
            let s = l
                .spot
                .as_ref()
                .ok_or_else(|| invalid("KHR_lights_punctual: type=spot but spot block missing"))?;
            // Spec defaults: inner = 0, outer = pi/4.
            Ok(Light::Spot {
                color,
                intensity,
                range: l.range,
                inner_cone_angle: s.inner_cone_angle.unwrap_or(0.0),
                outer_cone_angle: s.outer_cone_angle.unwrap_or(std::f32::consts::FRAC_PI_4),
            })
        }
        other => Err(invalid(format!(
            "KHR_lights_punctual: unknown type {other:?}"
        ))),
    }
}

fn convert_sampler_index(root: &GltfRoot, idx: Option<u32>) -> Sampler {
    let s = match idx {
        Some(i) => match root.samplers.get(i as usize) {
            Some(s) => s,
            None => return Sampler::default_sampler(),
        },
        None => return Sampler::default_sampler(),
    };
    let mag_filter = match s.mag_filter {
        Some(gj::MAG_FILTER_NEAREST) => MagFilter::Nearest,
        _ => MagFilter::Linear,
    };
    let min_filter = match s.min_filter {
        Some(gj::MIN_FILTER_NEAREST) => MinFilter::Nearest,
        Some(gj::MIN_FILTER_LINEAR) => MinFilter::Linear,
        Some(gj::MIN_FILTER_NEAREST_MIPMAP_NEAREST) => MinFilter::NearestMipNearest,
        Some(gj::MIN_FILTER_LINEAR_MIPMAP_NEAREST) => MinFilter::LinearMipNearest,
        Some(gj::MIN_FILTER_NEAREST_MIPMAP_LINEAR) => MinFilter::NearestMipLinear,
        Some(gj::MIN_FILTER_LINEAR_MIPMAP_LINEAR) | None => MinFilter::LinearMipLinear,
        Some(_) => MinFilter::LinearMipLinear,
    };
    let wrap_s = wrap_mode(s.wrap_s);
    let wrap_t = wrap_mode(s.wrap_t);
    Sampler {
        mag_filter,
        min_filter,
        wrap_s,
        wrap_t,
    }
}

fn wrap_mode(w: Option<u32>) -> WrapMode {
    match w {
        Some(gj::WRAP_CLAMP_TO_EDGE) => WrapMode::ClampToEdge,
        Some(gj::WRAP_MIRRORED_REPEAT) => WrapMode::MirroredRepeat,
        _ => WrapMode::Repeat,
    }
}

fn convert_texture(root: &GltfRoot, t: &gj::Texture, buffers: &[Arc<Vec<u8>>]) -> Result<Texture> {
    let sampler = convert_sampler_index(root, t.sampler);
    let image_idx = t
        .source
        .ok_or_else(|| invalid("texture: missing source (image index)"))?;
    let img = root
        .images
        .get(image_idx as usize)
        .ok_or_else(|| invalid(format!("texture: image {image_idx} out of range")))?;

    let image = if let Some(bv_idx) = img.buffer_view {
        let bv = root
            .buffer_views
            .get(bv_idx as usize)
            .ok_or_else(|| invalid(format!("image: bufferView {bv_idx} out of range")))?;
        let buf = buffers
            .get(bv.buffer as usize)
            .ok_or_else(|| invalid(format!("image: buffer {} out of range", bv.buffer)))?;
        let asset = BufferViewAsset::new(
            buf.clone(),
            bv.byte_offset.unwrap_or(0) as usize,
            bv.byte_length as usize,
            img.mime_type.clone(),
        );
        ImageData::Source(Arc::new(asset))
    } else if let Some(uri) = &img.uri {
        if uri.starts_with("data:") {
            let mime = img.mime_type.clone().or_else(|| data_uri_mime(uri));
            let bytes = decode_data_uri(uri)?;
            let asset = oxideav_mesh3d::asset::InMemoryAsset { mime, bytes };
            ImageData::Source(Arc::new(asset))
        } else {
            ImageData::External {
                uri: uri.clone(),
                mime: img.mime_type.clone(),
            }
        }
    } else {
        return Err(invalid(
            "image: must have either bufferView or uri (sparse accessors not supported)",
        ));
    };

    Ok(Texture {
        name: t.name.clone(),
        image,
        sampler,
    })
}

fn convert_material(
    m: &gj::Material,
    texture_id_map: &HashMap<u32, TextureId>,
) -> Result<Material> {
    let mut mat = Material::new();
    mat.name = m.name.clone();
    if let Some(p) = &m.pbr_metallic_roughness {
        if let Some(c) = p.base_color_factor {
            mat.base_color = c;
        }
        mat.base_color_texture = p.base_color_texture.as_ref().and_then(|t| {
            texture_id_map.get(&t.index).map(|&id| TextureRef {
                texture: id,
                uv_set: t.tex_coord.unwrap_or(0),
            })
        });
        if let Some(v) = p.metallic_factor {
            mat.metallic = v;
        }
        if let Some(v) = p.roughness_factor {
            mat.roughness = v;
        }
        mat.metallic_roughness_texture = p.metallic_roughness_texture.as_ref().and_then(|t| {
            texture_id_map.get(&t.index).map(|&id| TextureRef {
                texture: id,
                uv_set: t.tex_coord.unwrap_or(0),
            })
        });
    }
    if let Some(n) = &m.normal_texture {
        mat.normal_texture = texture_id_map.get(&n.index).map(|&id| TextureRef {
            texture: id,
            uv_set: n.tex_coord.unwrap_or(0),
        });
        if let Some(s) = n.scale {
            mat.normal_scale = s;
        }
    }
    if let Some(o) = &m.occlusion_texture {
        mat.occlusion_texture = texture_id_map.get(&o.index).map(|&id| TextureRef {
            texture: id,
            uv_set: o.tex_coord.unwrap_or(0),
        });
        if let Some(s) = o.strength {
            mat.occlusion_strength = s;
        }
    }
    if let Some(e) = m.emissive_factor {
        mat.emissive_factor = e;
    }
    if let Some(e) = &m.emissive_texture {
        mat.emissive_texture = texture_id_map.get(&e.index).map(|&id| TextureRef {
            texture: id,
            uv_set: e.tex_coord.unwrap_or(0),
        });
    }
    // KHR_texture_transform — a `textureInfo.extensions` block carrying
    // offset / rotation / scale / texCoord per
    // `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
    // Updates. We surface it through `Material::extras` under the key
    // `KHR_texture_transform:<slot>` (one entry per the five core PBR
    // texture slots) so downstream raster consumers can apply the affine
    // UV transform without us widening `oxideav_mesh3d::TextureRef`.
    // Bare `{}` resolves to all four spec defaults (`offset = [0, 0]`,
    // `rotation = 0`, `scale = [1, 1]`, `texCoord` unset).
    if let Some(p) = &m.pbr_metallic_roughness {
        stash_texture_transform(&mut mat, "baseColor", p.base_color_texture.as_ref());
        stash_texture_transform(
            &mut mat,
            "metallicRoughness",
            p.metallic_roughness_texture.as_ref(),
        );
    }
    if let Some(n) = &m.normal_texture {
        if let Some(tt) = n
            .extensions
            .as_ref()
            .and_then(|e| e.khr_texture_transform.as_ref())
        {
            mat.extras.insert(
                "KHR_texture_transform:normal".to_owned(),
                texture_transform_to_json(tt),
            );
        }
    }
    if let Some(o) = &m.occlusion_texture {
        if let Some(tt) = o
            .extensions
            .as_ref()
            .and_then(|e| e.khr_texture_transform.as_ref())
        {
            mat.extras.insert(
                "KHR_texture_transform:occlusion".to_owned(),
                texture_transform_to_json(tt),
            );
        }
    }
    stash_texture_transform(&mut mat, "emissive", m.emissive_texture.as_ref());
    mat.alpha_mode = match m.alpha_mode.as_deref() {
        Some("MASK") => AlphaMode::Mask {
            cutoff: m.alpha_cutoff.unwrap_or(0.5),
        },
        Some("BLEND") => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    };
    mat.double_sided = m.double_sided;
    // Per-material extensions — currently `KHR_materials_unlit`
    // (docs/3d/gltf/extensions/KHR_materials_unlit.md). The empty
    // extension object is a Boolean flag; we surface it through
    // `Material::extras["KHR_materials_unlit"] = true` so downstream
    // raster consumers can branch without us having to widen
    // `oxideav_mesh3d::Material`.
    if let Some(ext) = &m.extensions {
        if ext.khr_materials_unlit.is_some() {
            mat.extras
                .insert("KHR_materials_unlit".to_owned(), Value::Bool(true));
        }
        // KHR_materials_emissive_strength — a scalar multiplier on the
        // core emissive value (docs/3d/gltf/extensions/
        // KHR_materials_emissive_strength.md §Parameters). We surface
        // it through `Material::extras["KHR_materials_emissive_strength"]`
        // as a JSON number rather than widening `oxideav_mesh3d::Material`.
        // Per the spec the field is optional with a default of 1.0, so a
        // bare `{}` object resolves to that default.
        if let Some(es) = &ext.khr_materials_emissive_strength {
            let strength = es.emissive_strength.unwrap_or(1.0);
            if let Some(n) = serde_json::Number::from_f64(strength as f64) {
                mat.extras.insert(
                    "KHR_materials_emissive_strength".to_owned(),
                    Value::Number(n),
                );
            }
        }
        // KHR_materials_ior — a scalar index of refraction that overrides
        // the metallic-roughness dielectric BRDF's fixed 1.5 (docs/3d/gltf/
        // extensions/KHR_materials_ior.md). We surface it through
        // `Material::extras["KHR_materials_ior"]` as a JSON number rather
        // than widening `oxideav_mesh3d::Material`. Per the spec the field
        // is optional with a default of 1.5, so a bare `{}` object resolves
        // to that default.
        if let Some(io) = &ext.khr_materials_ior {
            let ior = io.ior.unwrap_or(1.5);
            if let Some(n) = serde_json::Number::from_f64(ior as f64) {
                mat.extras
                    .insert("KHR_materials_ior".to_owned(), Value::Number(n));
            }
        }
        // KHR_materials_specular — a specular reflection factor + F0
        // colour + optional textures per docs/3d/gltf/extensions/
        // KHR_materials_specular.md §Extending Materials. We surface it
        // through `Material::extras["KHR_materials_specular"]` as a JSON
        // object carrying any of the four spec-defined keys
        // (`specularFactor`, `specularTexture`, `specularColorFactor`,
        // `specularColorTexture`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec all four fields are
        // optional; we materialise the scalar / vector defaults
        // (`specularFactor = 1.0`, `specularColorFactor = [1, 1, 1]`)
        // so a bare `{}` object resolves to a fully-specified record,
        // and pass texture infos through verbatim (the raw `index`
        // numbers refer to positions in the round-tripped `textures[]`
        // array, which keep their ordering through scene→json on the
        // encode side).
        if let Some(sp) = &ext.khr_materials_specular {
            let mut obj = serde_json::Map::new();
            let factor = sp.specular_factor.unwrap_or(1.0);
            if let Some(n) = serde_json::Number::from_f64(factor as f64) {
                obj.insert("specularFactor".to_owned(), Value::Number(n));
            }
            let cf = sp.specular_color_factor.unwrap_or([1.0, 1.0, 1.0]);
            let cf_arr: Vec<Value> = cf
                .iter()
                .filter_map(|v| serde_json::Number::from_f64(*v as f64).map(Value::Number))
                .collect();
            if cf_arr.len() == 3 {
                obj.insert("specularColorFactor".to_owned(), Value::Array(cf_arr));
            }
            if let Some(t) = &sp.specular_texture {
                obj.insert("specularTexture".to_owned(), texture_info_to_json(t));
            }
            if let Some(t) = &sp.specular_color_texture {
                obj.insert("specularColorTexture".to_owned(), texture_info_to_json(t));
            }
            mat.extras
                .insert("KHR_materials_specular".to_owned(), Value::Object(obj));
        }
        // KHR_materials_clearcoat — a clear-coat layer (intensity +
        // roughness factors + optional textures) layered on top of the
        // metallic-roughness material per docs/3d/gltf/extensions/
        // KHR_materials_clearcoat.md §Extending Materials §Clearcoat. We
        // surface it through `Material::extras["KHR_materials_clearcoat"]`
        // as a JSON object carrying any of the five spec-defined keys
        // (`clearcoatFactor`, `clearcoatTexture`,
        // `clearcoatRoughnessFactor`, `clearcoatRoughnessTexture`,
        // `clearcoatNormalTexture`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec both factors are
        // optional with a default of 0.0, so we materialise those
        // defaults — a bare `{}` resolves to
        // `clearcoatFactor = clearcoatRoughnessFactor = 0.0` (and the
        // spec notes a zero `clearcoatFactor` disables the whole layer).
        // Texture infos pass through verbatim; `clearcoatNormalTexture`
        // is a `normalTextureInfo`, so it additionally carries an
        // optional `scale`.
        if let Some(cc) = &ext.khr_materials_clearcoat {
            let mut obj = serde_json::Map::new();
            let factor = cc.clearcoat_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(factor as f64) {
                obj.insert("clearcoatFactor".to_owned(), Value::Number(n));
            }
            let rough = cc.clearcoat_roughness_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(rough as f64) {
                obj.insert("clearcoatRoughnessFactor".to_owned(), Value::Number(n));
            }
            if let Some(t) = &cc.clearcoat_texture {
                obj.insert("clearcoatTexture".to_owned(), texture_info_to_json(t));
            }
            if let Some(t) = &cc.clearcoat_roughness_texture {
                obj.insert(
                    "clearcoatRoughnessTexture".to_owned(),
                    texture_info_to_json(t),
                );
            }
            if let Some(t) = &cc.clearcoat_normal_texture {
                obj.insert(
                    "clearcoatNormalTexture".to_owned(),
                    normal_texture_info_to_json(t),
                );
            }
            mat.extras
                .insert("KHR_materials_clearcoat".to_owned(), Value::Object(obj));
        }
        // KHR_materials_sheen — a sheen BRDF (colour + roughness factors
        // + optional textures) layered on top of the metallic-roughness
        // material per docs/3d/gltf/extensions/KHR_materials_sheen.md
        // §Extending Materials §Sheen. We surface it through
        // `Material::extras["KHR_materials_sheen"]` as a JSON object
        // carrying any of the four spec-defined keys (`sheenColorFactor`,
        // `sheenColorTexture`, `sheenRoughnessFactor`,
        // `sheenRoughnessTexture`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec all four fields are
        // optional; we materialise the colour / scalar defaults
        // (`sheenColorFactor = [0, 0, 0]`, `sheenRoughnessFactor = 0.0`)
        // so a bare `{}` resolves to a fully-specified record (the spec
        // notes a zero `sheenColorFactor` disables the whole layer).
        // Texture infos pass through verbatim.
        if let Some(sh) = &ext.khr_materials_sheen {
            let mut obj = serde_json::Map::new();
            let cf = sh.sheen_color_factor.unwrap_or([0.0, 0.0, 0.0]);
            let cf_arr: Vec<Value> = cf
                .iter()
                .filter_map(|v| serde_json::Number::from_f64(*v as f64).map(Value::Number))
                .collect();
            if cf_arr.len() == 3 {
                obj.insert("sheenColorFactor".to_owned(), Value::Array(cf_arr));
            }
            let rough = sh.sheen_roughness_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(rough as f64) {
                obj.insert("sheenRoughnessFactor".to_owned(), Value::Number(n));
            }
            if let Some(t) = &sh.sheen_color_texture {
                obj.insert("sheenColorTexture".to_owned(), texture_info_to_json(t));
            }
            if let Some(t) = &sh.sheen_roughness_texture {
                obj.insert("sheenRoughnessTexture".to_owned(), texture_info_to_json(t));
            }
            mat.extras
                .insert("KHR_materials_sheen".to_owned(), Value::Object(obj));
        }
        // KHR_materials_transmission — makes the metallic-roughness
        // material optically transparent per
        // docs/3d/gltf/extensions/KHR_materials_transmission.md
        // §Properties. We surface it through
        // `Material::extras["KHR_materials_transmission"]` as a JSON
        // object carrying either of the two spec-defined keys
        // (`transmissionFactor`, `transmissionTexture`) rather than
        // widening `oxideav_mesh3d::Material`. Per the spec both fields
        // are optional; we materialise the scalar default
        // (`transmissionFactor = 0.0`) so a bare `{}` resolves to a
        // fully-specified record. The texture info passes through
        // verbatim.
        if let Some(tr) = &ext.khr_materials_transmission {
            let mut obj = serde_json::Map::new();
            let factor = tr.transmission_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(factor as f64) {
                obj.insert("transmissionFactor".to_owned(), Value::Number(n));
            }
            if let Some(t) = &tr.transmission_texture {
                obj.insert("transmissionTexture".to_owned(), texture_info_to_json(t));
            }
            mat.extras
                .insert("KHR_materials_transmission".to_owned(), Value::Object(obj));
        }
        // KHR_materials_volume — turns the surface into the boundary of a
        // homogeneous volumetric medium (thickness + attenuation) per
        // docs/3d/gltf/extensions/KHR_materials_volume.md §Properties. We
        // surface it through `Material::extras["KHR_materials_volume"]` as
        // a JSON object carrying any of the four spec-defined keys
        // (`thicknessFactor`, `thicknessTexture`, `attenuationDistance`,
        // `attenuationColor`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec all four fields are
        // optional; we materialise the scalar / colour defaults
        // (`thicknessFactor = 0.0`, `attenuationColor = [1, 1, 1]`) so a
        // bare `{}` resolves to a fully-specified record. The
        // `attenuationDistance` default per the spec is `+Infinity`, which
        // JSON cannot encode, so we leave that key absent when the source
        // document omits it — consumers interpret a missing key as the
        // spec default of `+Infinity` (thin-walled materials with
        // `thicknessFactor = 0` ignore the attenuation parameters
        // altogether). Texture infos pass through verbatim.
        if let Some(vol) = &ext.khr_materials_volume {
            let mut obj = serde_json::Map::new();
            let tf = vol.thickness_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(tf as f64) {
                obj.insert("thicknessFactor".to_owned(), Value::Number(n));
            }
            if let Some(t) = &vol.thickness_texture {
                obj.insert("thicknessTexture".to_owned(), texture_info_to_json(t));
            }
            // Only emit `attenuationDistance` when the source provided
            // a value; the `+Infinity` default cannot round-trip through
            // JSON, so absence carries the spec default.
            if let Some(d) = vol.attenuation_distance {
                if let Some(n) = serde_json::Number::from_f64(d as f64) {
                    obj.insert("attenuationDistance".to_owned(), Value::Number(n));
                }
            }
            let ac = vol.attenuation_color.unwrap_or([1.0, 1.0, 1.0]);
            let ac_arr: Vec<Value> = ac
                .iter()
                .filter_map(|v| serde_json::Number::from_f64(*v as f64).map(Value::Number))
                .collect();
            if ac_arr.len() == 3 {
                obj.insert("attenuationColor".to_owned(), Value::Array(ac_arr));
            }
            mat.extras
                .insert("KHR_materials_volume".to_owned(), Value::Object(obj));
        }
        // KHR_materials_iridescence — thin-film interference layer on top
        // of the metallic-roughness material; the hue varies with viewing
        // angle and thin-film thickness per
        // docs/3d/gltf/extensions/KHR_materials_iridescence.md §Properties.
        // We surface it through `Material::extras["KHR_materials_iridescence"]`
        // as a JSON object carrying any of the six spec-defined keys
        // (`iridescenceFactor`, `iridescenceTexture`, `iridescenceIor`,
        // `iridescenceThicknessMinimum`, `iridescenceThicknessMaximum`,
        // `iridescenceThicknessTexture`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec all six fields are
        // optional; we materialise the scalar defaults
        // (`iridescenceFactor = 0.0`, `iridescenceIor = 1.3`,
        // `iridescenceThicknessMinimum = 100.0`,
        // `iridescenceThicknessMaximum = 400.0`) so a bare `{}` resolves
        // to a fully-specified record. Texture infos pass through verbatim.
        if let Some(ir) = &ext.khr_materials_iridescence {
            let mut obj = serde_json::Map::new();
            let factor = ir.iridescence_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(factor as f64) {
                obj.insert("iridescenceFactor".to_owned(), Value::Number(n));
            }
            if let Some(t) = &ir.iridescence_texture {
                obj.insert("iridescenceTexture".to_owned(), texture_info_to_json(t));
            }
            let ior_val = ir.iridescence_ior.unwrap_or(1.3);
            if let Some(n) = serde_json::Number::from_f64(ior_val as f64) {
                obj.insert("iridescenceIor".to_owned(), Value::Number(n));
            }
            let thmin = ir.iridescence_thickness_minimum.unwrap_or(100.0);
            if let Some(n) = serde_json::Number::from_f64(thmin as f64) {
                obj.insert("iridescenceThicknessMinimum".to_owned(), Value::Number(n));
            }
            let thmax = ir.iridescence_thickness_maximum.unwrap_or(400.0);
            if let Some(n) = serde_json::Number::from_f64(thmax as f64) {
                obj.insert("iridescenceThicknessMaximum".to_owned(), Value::Number(n));
            }
            if let Some(t) = &ir.iridescence_thickness_texture {
                obj.insert(
                    "iridescenceThicknessTexture".to_owned(),
                    texture_info_to_json(t),
                );
            }
            mat.extras
                .insert("KHR_materials_iridescence".to_owned(), Value::Object(obj));
        }
    }
    if let Some(extras) = &m.extras {
        extras_into(&mut mat.extras, extras.clone());
    }
    Ok(mat)
}

fn convert_primitive(
    root: &GltfRoot,
    p: &gj::Primitive,
    buffers: &[Arc<Vec<u8>>],
    material_id_map: &HashMap<u32, MaterialId>,
) -> Result<Primitive> {
    let topology = topology_from_mode(p.mode.unwrap_or(gj::MODE_TRIANGLES))?;
    let mut prim = Primitive::new(topology);

    // Spec §3.7.2.1 — all attribute accessors of a primitive MUST share
    // a single `count`. Spec §3.6.2.4 — attribute accessor byteOffset +
    // bufferView byteStride alignment.
    validate_attribute_counts(&p.attributes, &root.accessors)?;
    for (name, &acc_idx) in &p.attributes {
        if let Some(acc) = root.accessors.get(acc_idx as usize) {
            validate_alignment(acc, &root.buffer_views, true, name)?;
        }
    }

    // POSITION is mandatory per spec §3.7.2.1.
    let position_idx = *p
        .attributes
        .get("POSITION")
        .ok_or_else(|| invalid("primitive: missing POSITION attribute"))?;
    prim.positions = read_attr_vec3(root, buffers, position_idx)?;

    if let Some(&i) = p.attributes.get("NORMAL") {
        prim.normals = Some(read_attr_vec3(root, buffers, i)?);
    }
    if let Some(&i) = p.attributes.get("TANGENT") {
        let tangents = read_attr_vec4(root, buffers, i)?;
        // Spec §3.7.2.1: TANGENT.w MUST be ±1.0.
        validate_tangent_w(&tangents)?;
        prim.tangents = Some(tangents);
    }
    // TEXCOORD_n
    let mut texcoord_idx = 0;
    while let Some(&i) = p.attributes.get(&format!("TEXCOORD_{texcoord_idx}")) {
        prim.uvs.push(read_attr_vec2(root, buffers, i)?);
        texcoord_idx += 1;
    }
    // COLOR_n — accept VEC3 or VEC4 (we promote VEC3 to VEC4 with alpha=1.0).
    let mut color_idx = 0;
    while let Some(&i) = p.attributes.get(&format!("COLOR_{color_idx}")) {
        let colors = read_attr_color(root, buffers, i)?;
        // Spec §3.7.2.1: COLOR_0 components MUST be in [0.0, 1.0].
        // Higher COLOR_n sets are not constrained.
        if color_idx == 0 {
            validate_color0_range(&colors)?;
        }
        prim.colors.push(colors);
        color_idx += 1;
    }
    if let Some(&i) = p.attributes.get("JOINTS_0") {
        let acc = &root.accessors[i as usize];
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        prim.joints = Some(read_vec4_u16(&view)?);
    }
    if let Some(&i) = p.attributes.get("WEIGHTS_0") {
        let acc = &root.accessors[i as usize];
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        let raw = read_vec_f32::<4>(&view)?;
        prim.weights = Some(raw);
    }

    if let Some(idx_acc) = p.indices {
        let acc = &root.accessors[idx_acc as usize];
        // Spec §3.6.2.4: index accessor byteOffset must be aligned to
        // its component-type size (the 4-byte vertex-attribute rule
        // does NOT apply to indices — they're tightly packed per
        // §3.6.2 for non-vertex-attribute accessors).
        validate_alignment(acc, &root.buffer_views, false, "indices")?;
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        let widened = read_indices_u32(acc, &view)?;
        // Spec §3.7.2.1: index value MUST NOT equal the max value for
        // the chosen componentType (reserved for primitive-restart).
        validate_index_no_restart(acc, &widened)?;
        // Pick the narrowest representable width — this is glTF's
        // own convention (5121 / 5123 / 5125 are all valid).
        let max = widened.iter().copied().max().unwrap_or(0);
        if max <= u16::MAX as u32 {
            prim.indices = Some(Indices::U16(
                widened.into_iter().map(|x| x as u16).collect(),
            ));
        } else {
            prim.indices = Some(Indices::U32(widened));
        }
    }

    if let Some(m) = p.material {
        prim.material = material_id_map.get(&m).copied();
    }
    if let Some(extras) = &p.extras {
        extras_into(&mut prim.extras, extras.clone());
    }

    // Morph targets (§3.7.2.2). The typed `oxideav_mesh3d::Primitive`
    // model doesn't carry a dedicated field, so we serialise the
    // resolved per-target attribute deltas into a JSON sentinel under
    // `prim.extras["__morph_targets"]` — encoder pulls them back out
    // and re-emits as accessors. Format: an array of objects, one per
    // target, mapping attribute name → array of [f32; N] values.
    if !p.targets.is_empty() {
        let mut targets_json = Vec::with_capacity(p.targets.len());
        for tgt in &p.targets {
            let mut obj = serde_json::Map::new();
            for (name, &acc_idx) in tgt {
                // POSITION/NORMAL/TANGENT all read as VEC3 FLOAT per
                // the §3.7.2.2 morph-target table (handedness W is
                // dropped on TANGENT since it can't be displaced).
                let acc = root.accessors.get(acc_idx as usize).ok_or_else(|| {
                    invalid(format!(
                        "morph target attribute {name:?}: accessor {acc_idx} oob"
                    ))
                })?;
                if acc.kind != "VEC3" || acc.component_type != gj::COMPONENT_TYPE_FLOAT {
                    return Err(unsupported(format!(
                        "morph target {name:?}: only VEC3 FLOAT supported in r4 (got {:?} {})",
                        acc.kind, acc.component_type
                    )));
                }
                let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
                let view = view_from_materialised(acc, &bytes)?;
                let deltas = read_vec_f32::<3>(&view)?;
                let arr: Vec<serde_json::Value> = deltas
                    .into_iter()
                    .map(|v| {
                        serde_json::Value::Array(
                            v.iter()
                                .map(|&c| {
                                    serde_json::Number::from_f64(c as f64)
                                        .map(serde_json::Value::Number)
                                        .unwrap_or(serde_json::Value::Null)
                                })
                                .collect(),
                        )
                    })
                    .collect();
                obj.insert(name.clone(), serde_json::Value::Array(arr));
            }
            targets_json.push(serde_json::Value::Object(obj));
        }
        prim.extras.insert(
            "__morph_targets".to_owned(),
            serde_json::Value::Array(targets_json),
        );
    }
    Ok(prim)
}

fn read_attr_vec3(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    accessor_idx: u32,
) -> Result<Vec<[f32; 3]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.kind != "VEC3" || acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        return Err(unsupported(format!(
            "attribute accessor: expected VEC3 FLOAT, got {:?} {}",
            acc.kind, acc.component_type
        )));
    }
    let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
    let view = view_from_materialised(acc, &bytes)?;
    let data = read_vec_f32::<3>(&view)?;
    // Spec §3.6.2.1.5: when accessor.min/max are declared they MUST
    // match the actual component-wise extrema. (Animation input + the
    // POSITION attribute REQUIRE them to be declared, but any accessor
    // that DOES declare them must agree with the data.) Validate here
    // for VEC3 attributes (covers POSITION + NORMAL + TANGENT base
    // attributes; morph targets are read by a separate path).
    validate_vec3_bounds(acc, &data)?;
    Ok(data)
}

/// Spec §3.6.2.1.5 bounds check: when an accessor declares `min` /
/// `max` the values MUST match the component-wise extrema of the
/// stored data. Returns an `AccessorBoundsMismatch`-prefixed
/// `InvalidData` (the typed `Error` enum lives in `oxideav-core` and
/// can't gain a new variant from a sibling crate; the prefix lets
/// callers grep for the condition without an enum check).
fn validate_vec3_bounds(acc: &gj::Accessor, data: &[[f32; 3]]) -> Result<()> {
    let (Some(declared_min), Some(declared_max)) = (&acc.min, &acc.max) else {
        return Ok(());
    };
    if declared_min.len() != 3 || declared_max.len() != 3 {
        return Err(invalid(format!(
            "AccessorBoundsMismatch: VEC3 accessor min/max must have 3 components (got {} / {})",
            declared_min.len(),
            declared_max.len()
        )));
    }
    if data.is_empty() {
        return Ok(());
    }
    let mut mn = data[0];
    let mut mx = data[0];
    for v in &data[1..] {
        for c in 0..3 {
            if v[c] < mn[c] {
                mn[c] = v[c];
            }
            if v[c] > mx[c] {
                mx[c] = v[c];
            }
        }
    }
    // Tolerance: bounds are stored as f32 in our document; round-trip
    // through JSON serialisation can introduce sub-ulp drift, so
    // accept differences below an absolute epsilon scaled by the
    // value magnitude (1e-5 relative or 1e-6 absolute, whichever
    // wins).
    for c in 0..3 {
        let dmin = declared_min[c];
        let dmax = declared_max[c];
        let tol = (mn[c].abs().max(mx[c].abs()) * 1e-5).max(1e-6);
        if (dmin - mn[c]).abs() > tol {
            return Err(invalid(format!(
                "AccessorBoundsMismatch: declared min[{c}] = {dmin}, actual = {}",
                mn[c]
            )));
        }
        if (dmax - mx[c]).abs() > tol {
            return Err(invalid(format!(
                "AccessorBoundsMismatch: declared max[{c}] = {dmax}, actual = {}",
                mx[c]
            )));
        }
    }
    Ok(())
}

fn read_attr_vec2(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    accessor_idx: u32,
) -> Result<Vec<[f32; 2]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.kind != "VEC2" || acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        return Err(unsupported(format!(
            "TEXCOORD accessor: expected VEC2 FLOAT, got {:?} {}",
            acc.kind, acc.component_type
        )));
    }
    let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
    let view = view_from_materialised(acc, &bytes)?;
    read_vec_f32::<2>(&view)
}

fn read_attr_vec4(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    accessor_idx: u32,
) -> Result<Vec<[f32; 4]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.kind != "VEC4" || acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        return Err(unsupported(format!(
            "TANGENT accessor: expected VEC4 FLOAT, got {:?} {}",
            acc.kind, acc.component_type
        )));
    }
    let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
    let view = view_from_materialised(acc, &bytes)?;
    read_vec_f32::<4>(&view)
}

fn read_attr_color(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    accessor_idx: u32,
) -> Result<Vec<[f32; 4]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        return Err(unsupported(format!(
            "COLOR accessor: only FLOAT supported in r1, got {}",
            acc.component_type
        )));
    }
    let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
    let view = view_from_materialised(acc, &bytes)?;
    match acc.kind.as_str() {
        "VEC3" => {
            let raw = read_vec_f32::<3>(&view)?;
            Ok(raw.into_iter().map(|c| [c[0], c[1], c[2], 1.0]).collect())
        }
        "VEC4" => read_vec_f32::<4>(&view),
        other => Err(unsupported(format!(
            "COLOR accessor: type {other:?} not supported"
        ))),
    }
}

fn topology_from_mode(mode: u32) -> Result<Topology> {
    Ok(match mode {
        gj::MODE_POINTS => Topology::Points,
        gj::MODE_LINES => Topology::Lines,
        gj::MODE_LINE_LOOP => Topology::LineLoop,
        gj::MODE_LINE_STRIP => Topology::LineStrip,
        gj::MODE_TRIANGLES => Topology::Triangles,
        gj::MODE_TRIANGLE_STRIP => Topology::TriangleStrip,
        gj::MODE_TRIANGLE_FAN => Topology::TriangleFan,
        other => return Err(invalid(format!("primitive.mode {other} unknown"))),
    })
}

// Stash a `KHR_texture_transform` block, if present on the given
// textureInfo, into `mat.extras["KHR_texture_transform:<slot>"]`. A
// no-op when the texture isn't set or has no transform. The slot key
// pairs the transform back to the right textureInfo on the encoder
// side. See `docs/3d/gltf/extensions/KHR_texture_transform.md`.
fn stash_texture_transform(mat: &mut Material, slot: &str, info: Option<&gj::TextureInfo>) {
    let Some(info) = info else { return };
    let Some(tt) = info
        .extensions
        .as_ref()
        .and_then(|e| e.khr_texture_transform.as_ref())
    else {
        return;
    };
    mat.extras.insert(
        format!("KHR_texture_transform:{slot}"),
        texture_transform_to_json(tt),
    );
}

// Render a `TextureInfo` (texture index + optional texCoord) back to a
// JSON object the way it appears on the wire. Used by the
// `KHR_materials_specular` decoder to keep the raw texture-info shape
// when surfacing the extension through the `Material::extras`
// side-channel. Per-textureInfo extensions (today only
// `KHR_texture_transform`, docs/3d/gltf/extensions/
// KHR_texture_transform.md) pass through verbatim too.
fn texture_info_to_json(t: &gj::TextureInfo) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("index".to_owned(), Value::from(t.index));
    if let Some(tc) = t.tex_coord {
        m.insert("texCoord".to_owned(), Value::from(tc));
    }
    if let Some(ext) = &t.extensions {
        if let Some(ext_json) = texture_info_extensions_to_json(ext) {
            m.insert("extensions".to_owned(), ext_json);
        }
    }
    Value::Object(m)
}

// Render a `NormalTextureInfo` (texture index + optional texCoord +
// optional scale) back to a JSON object the way it appears on the wire.
// Used by the `KHR_materials_clearcoat` decoder for the extension's
// `clearcoatNormalTexture`, which is a `normalTextureInfo` and so
// carries an optional `scale` per
// `docs/3d/gltf/extensions/KHR_materials_clearcoat.md` §Clearcoat.
// Per-textureInfo extensions (today only `KHR_texture_transform`,
// docs/3d/gltf/extensions/KHR_texture_transform.md) pass through
// verbatim too.
fn normal_texture_info_to_json(t: &gj::NormalTextureInfo) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("index".to_owned(), Value::from(t.index));
    if let Some(tc) = t.tex_coord {
        m.insert("texCoord".to_owned(), Value::from(tc));
    }
    if let Some(s) = t.scale {
        if let Some(n) = serde_json::Number::from_f64(s as f64) {
            m.insert("scale".to_owned(), Value::Number(n));
        }
    }
    if let Some(ext) = &t.extensions {
        if let Some(ext_json) = texture_info_extensions_to_json(ext) {
            m.insert("extensions".to_owned(), ext_json);
        }
    }
    Value::Object(m)
}

// Render a `TextureInfoExtensions` block into the JSON shape that
// matches the source document: a `{ "KHR_texture_transform": {...} }`
// object containing only the keys actually present. Returns `None` if
// no recognised sub-extension is set, so the caller can skip the
// `extensions` key entirely.
fn texture_info_extensions_to_json(ext: &gj::TextureInfoExtensions) -> Option<Value> {
    let mut obj = serde_json::Map::new();
    if let Some(t) = &ext.khr_texture_transform {
        obj.insert(
            "KHR_texture_transform".to_owned(),
            texture_transform_to_json(t),
        );
    }
    if obj.is_empty() {
        None
    } else {
        Some(Value::Object(obj))
    }
}

// Render a `KHR_texture_transform` extension object — emitting only
// the keys actually present per `docs/3d/gltf/extensions/
// KHR_texture_transform.md` §glTF Schema Updates (all four fields are
// optional, with defaults `offset = [0, 0]`, `rotation = 0`,
// `scale = [1, 1]`).
pub(crate) fn texture_transform_to_json(t: &gj::TextureTransform) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(o) = t.offset {
        let arr: Vec<Value> = o
            .iter()
            .filter_map(|v| serde_json::Number::from_f64(*v as f64).map(Value::Number))
            .collect();
        if arr.len() == 2 {
            obj.insert("offset".to_owned(), Value::Array(arr));
        }
    }
    if let Some(r) = t.rotation {
        if let Some(n) = serde_json::Number::from_f64(r as f64) {
            obj.insert("rotation".to_owned(), Value::Number(n));
        }
    }
    if let Some(s) = t.scale {
        let arr: Vec<Value> = s
            .iter()
            .filter_map(|v| serde_json::Number::from_f64(*v as f64).map(Value::Number))
            .collect();
        if arr.len() == 2 {
            obj.insert("scale".to_owned(), Value::Array(arr));
        }
    }
    if let Some(tc) = t.tex_coord {
        obj.insert("texCoord".to_owned(), Value::from(tc));
    }
    Value::Object(obj)
}

// `extras` is a JSON object — flatten one level into the
// `HashMap<String, Value>` we carry on every type.
fn extras_into(target: &mut HashMap<String, Value>, value: Value) {
    if let Value::Object(map) = value {
        for (k, v) in map {
            target.insert(k, v);
        }
    } else {
        target.insert("_value".to_owned(), value);
    }
}

#[allow(dead_code)]
fn _silence(_: &Error) {}
