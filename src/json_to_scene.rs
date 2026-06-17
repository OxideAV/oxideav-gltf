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
use crate::quantization::{self, ATTR_QUANT_KEY, EXTENSION_NAME};
use crate::validation::{
    check_asset_version, validate_accessor_fits_bufferview, validate_accessors, validate_alignment,
    validate_animation_channels, validate_attribute_counts, validate_bufferview_fits_buffer,
    validate_cameras, validate_color0_range, validate_extension_stack, validate_index_no_restart,
    validate_nodes, validate_samplers, validate_sparse_indices_buffer_views,
    validate_sparse_values_buffer_views, validate_tangent_w,
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
    // Spec §5.4.1 — accessor.sparse.values.bufferView MUST NOT carry
    // `target` or `byteStride` properties either (the sparse-values
    // block is tightly-packed per §5.4 "The elements are tightly
    // packed", same shape as the §5.3.1 sparse-indices rule above).
    validate_sparse_values_buffer_views(&root.accessors, &root.buffer_views)?;
    // Spec §3.6.2 + §5.1 — core per-accessor property MUSTs independent
    // of the bufferView: count >= 1 (§5.1), `normalized` MUST NOT be true
    // for FLOAT / UNSIGNED_INT componentType (§5.1.6 / §3.6.2.1), and
    // `min` / `max` array length MUST equal the accessor's component
    // count (§3.6.2.5).
    validate_accessors(&root.accessors)?;
    // Spec §5.12 + §5.13 + §5.14 — camera projection blocks are
    // mutually exclusive; orthographic xmag/ymag MUST NOT be zero,
    // zfar > 0 and > znear, znear >= 0; perspective yfov/znear > 0,
    // aspectRatio (when defined) > 0, zfar (when defined) > znear.
    validate_cameras(&root.cameras)?;

    // Spec §5.26 — texture-sampler filter / wrap modes, when present,
    // MUST hold one of the enumerated WebGL enum constants (magFilter
    // NEAREST/LINEAR; minFilter the six filter+mipmap combinations;
    // wrapS/wrapT CLAMP_TO_EDGE / MIRRORED_REPEAT / REPEAT).
    validate_samplers(&root.samplers)?;

    // Spec §3.5.2 + §3.5.3 — the node hierarchy MUST be a set of
    // disjoint strict trees (child indices in range, single parent, no
    // cycles); per-node transforms MUST keep `matrix` mutually
    // exclusive with TRS, MUST use TRS only on animated nodes, MUST
    // carry a unit-quaternion `rotation`, MUST be finite, and a
    // `matrix` MUST be decomposable to TRS (non-zero determinant).
    validate_nodes(&root.nodes, &root.animations)?;

    let mut buffers = resolve_buffers(root, glb_bin)?;
    // `KHR_meshopt_compression` inflate pass — per
    // `docs/3d/gltf/extensions/KHR_meshopt_compression.md` Appendix A /
    // B. Each compressed bufferView's descriptor sources opaque
    // compressed bytes from its own `buffer` / `byteOffset` /
    // `byteLength`; the decompressed bytes are written into the *parent*
    // bufferView's buffer at its `byteOffset` (per §"Fallback buffers":
    // "encoders should use the decompressed data to populate the
    // fallback buffer view"). After this pass the standard accessor
    // pipeline reads the real attribute / index data unchanged.
    inflate_meshopt_buffer_views(root, &mut buffers)?;
    let mut scene = Scene3D::new();

    // `KHR_meshopt_compression` sidecar — per
    // `docs/3d/gltf/extensions/KHR_meshopt_compression.md`
    // §"Specifying compressed views" the extension hangs off a
    // bufferView and redirects the source bytes to a different
    // buffer/range with a compression mode + filter + count +
    // byteStride. Per §"Fallback buffers" a buffer object may also
    // be tagged as a placeholder (`{ "fallback": true }`). The crate
    // is a pass-through engine — the meshopt bitstream decoder is
    // not implemented yet — so we record both layers under a single
    // sidecar so the encoder can re-emit the document with the
    // descriptors intact:
    //
    //   scene.extras["KHR_meshopt_compression"] = {
    //       "bufferViews": { "<bv_index>": <ext_obj> },
    //       "fallbackBuffers": [<buf_index>, ...]
    //   }
    //
    // §3.12 + the spec say the extension MUST be declared in
    // `extensionsUsed`, and when any fallback buffer is uri-less
    // it MUST be in `extensionsRequired`; both gates are surfaced
    // on encode and policed by `validate_root`.
    {
        let mut bv_map = serde_json::Map::new();
        for (bvi, bv) in root.buffer_views.iter().enumerate() {
            if let Some(mc) = bv
                .extensions
                .as_ref()
                .and_then(|e| e.khr_meshopt_compression.as_ref())
            {
                bv_map.insert(
                    bvi.to_string(),
                    serde_json::to_value(mc).map_err(|e| {
                        invalid(format!(
                            "KHR_meshopt_compression: failed to capture bufferView[{bvi}] descriptor: {e}"
                        ))
                    })?,
                );
            }
        }
        let mut fb_buffers: Vec<serde_json::Value> = Vec::new();
        for (bi, b) in root.buffers.iter().enumerate() {
            if b.extensions
                .as_ref()
                .and_then(|e| e.khr_meshopt_compression.as_ref())
                .map(|m| m.fallback)
                .unwrap_or(false)
            {
                fb_buffers.push(serde_json::Value::from(bi as u32));
            }
        }
        if !bv_map.is_empty() || !fb_buffers.is_empty() {
            let mut top = serde_json::Map::new();
            if !bv_map.is_empty() {
                top.insert("bufferViews".to_owned(), serde_json::Value::Object(bv_map));
            }
            if !fb_buffers.is_empty() {
                top.insert(
                    "fallbackBuffers".to_owned(),
                    serde_json::Value::Array(fb_buffers),
                );
            }
            scene.extras.insert(
                "KHR_meshopt_compression".to_owned(),
                serde_json::Value::Object(top),
            );
        }
    }

    // Materials first — meshes need the IDs.
    let mut material_id_map: HashMap<u32, MaterialId> = HashMap::new();
    // Textures first — materials reference them by index.
    let mut texture_id_map: HashMap<u32, TextureId> = HashMap::new();
    // `KHR_texture_basisu` sidecar — per
    // `docs/3d/gltf/extensions/KHR_texture_basisu.md` the extension
    // adds a per-texture `source` indirection to a KTX v2 image. Two
    // shapes are spec-allowed: (1) "with fallback" where the base
    // `texture.source` also points to a PNG / JPEG image and capable
    // clients pick the KTX2; (2) "without fallback" where only the
    // extension's image is present and `KHR_texture_basisu` MUST
    // appear in `extensionsRequired`. We're a pass-through engine
    // (we don't transcode KTX2), so we pick the base `source` when
    // present (PNG / JPEG path), otherwise we load the extension's
    // KTX2 image as opaque bytes via the usual `BufferViewAsset` /
    // `InMemoryAsset` route — downstream consumers see the
    // `image/ktx2` MIME and decide whether to transcode. The
    // sidecar records the scene-texture indices that came from the
    // extension path so the encoder re-emits them in the spec's
    // "without fallback" shape (extension declared in BOTH
    // `extensionsUsed` AND `extensionsRequired`).
    let mut basisu_textures: Vec<serde_json::Value> = Vec::new();
    for (i, t) in root.textures.iter().enumerate() {
        let (tex, used_basisu) = convert_texture(root, t, &buffers)?;
        let id = scene.add_texture(tex);
        texture_id_map.insert(i as u32, id);
        if used_basisu {
            basisu_textures.push(serde_json::Value::from(i as u32));
        }
    }
    if !basisu_textures.is_empty() {
        let mut top = serde_json::Map::new();
        top.insert(
            "textures".to_owned(),
            serde_json::Value::Array(basisu_textures),
        );
        scene.extras.insert(
            "KHR_texture_basisu".to_owned(),
            serde_json::Value::Object(top),
        );
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
        // KHR_xmp_json_ld — per-mesh packet reference per
        // `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating
        // XMP metadata". The spec's primary illustration of the
        // indirection uses a Mesh. mesh3d's `Mesh` has no `extensions`
        // field, so stash on primitive[0].extras["__mesh_xmp_packet"]
        // as a bare JSON number.
        if let (Some(ext), Some(prim0)) = (&m.extensions, mesh.primitives.first_mut()) {
            if let Some(xmp) = &ext.khr_xmp_json_ld {
                prim0.extras.insert(
                    "__mesh_xmp_packet".to_owned(),
                    serde_json::Value::Number(xmp.packet.into()),
                );
            }
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
            // KHR_node_visibility — boolean `visible` flag on a node
            // (docs/3d/gltf/extensions/KHR_node_visibility.md
            // §Extending Nodes). The spec defines `visible` as
            // optional with a default of `true`, so a bare `{}`
            // object resolves to that default. We surface the value
            // through `Node::extras["KHR_node_visibility"]` as a
            // JSON boolean rather than widening
            // `oxideav_mesh3d::Node`.
            if let Some(nv) = &ext.khr_node_visibility {
                let visible = nv.visible.unwrap_or(true);
                node.extras
                    .insert("KHR_node_visibility".to_owned(), Value::Bool(visible));
            }
            // KHR_xmp_json_ld — per-node packet reference per
            // `docs/3d/gltf/extensions/KHR_xmp_json_ld.md`
            // §"Instantiating XMP metadata". Stash on `node.extras`
            // as a bare JSON number; the encoder lifts it back.
            if let Some(xmp) = &ext.khr_xmp_json_ld {
                node.extras.insert(
                    "KHR_xmp_json_ld".to_owned(),
                    Value::Number(xmp.packet.into()),
                );
            }
        }
        if let Some(extras) = &n.extras {
            extras_into(&mut node.extras, extras.clone());
        }
        scene.add_node(node);
    }

    // Animations — channels target NodeIds, so resolve after nodes are loaded.
    // `KHR_animation_pointer`-flagged channels do NOT bind to a node (per
    // `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Extension
    // Usage"); they accumulate into a Scene3D::extras side-channel for
    // round-trip preservation rather than into the typed Animation.
    let mut pointer_animations: Vec<Value> = Vec::new();
    for (ai, a) in root.animations.iter().enumerate() {
        let (anim, ptr_channels) = convert_animation(a, root, &buffers)?;
        scene.add_animation(anim);
        if !ptr_channels.is_empty() {
            let mut obj = serde_json::Map::new();
            obj.insert("animation".into(), Value::from(ai as u32));
            if let Some(name) = &a.name {
                obj.insert("name".into(), Value::String(name.clone()));
            }
            obj.insert("channels".into(), Value::Array(ptr_channels));
            pointer_animations.push(Value::Object(obj));
        }
    }
    if !pointer_animations.is_empty() {
        let mut top = serde_json::Map::new();
        top.insert("animations".into(), Value::Array(pointer_animations));
        scene
            .extras
            .insert("KHR_animation_pointer".into(), Value::Object(top));
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

    // KHR_materials_variants — root-level variant roster per
    // `docs/3d/gltf/extensions/KHR_materials_variants.md`. The
    // extension stores up to `N` named variants on the document; each
    // primitive then maps a `material` index to one or more variant
    // indices via the per-primitive extension block. The typed
    // `oxideav_mesh3d::Scene3D` has no first-class variants field, so
    // the roster is surfaced through `scene.extras["KHR_materials_variants"]`
    // as the JSON object `{ "variants": [ { "name": "...", ... }, ... ] }`
    // — the same shape the encoder lifts back out for emission. Each
    // primitive's mappings array is preserved under
    // `primitive.extras["KHR_materials_variants"] = { "mappings": [...] }`
    // (see `convert_primitive`).
    if let Some(ext) = &root.extensions {
        if let Some(vroot) = &ext.khr_materials_variants {
            let arr: Vec<serde_json::Value> = vroot
                .variants
                .iter()
                .map(|v| {
                    let mut o = serde_json::Map::new();
                    o.insert("name".into(), serde_json::Value::String(v.name.clone()));
                    if let Some(e) = &v.extras {
                        o.insert("extras".into(), e.clone());
                    }
                    serde_json::Value::Object(o)
                })
                .collect();
            let mut obj = serde_json::Map::new();
            obj.insert("variants".into(), serde_json::Value::Array(arr));
            scene.extras.insert(
                "KHR_materials_variants".to_owned(),
                serde_json::Value::Object(obj),
            );
        }
        // KHR_xmp_json_ld root-level `packets[]` roster per
        // `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Defining XMP
        // Metadata". Each packet is held verbatim as opaque JSON-LD
        // because the spec specifies a restricted JSON-LD subset
        // (§"JSON-LD Restrictions and Recommendations") without
        // pinning the namespace vocabulary. Surfaced under
        // `scene.extras["KHR_xmp_json_ld"] = { "packets": [...] }` so
        // the encoder can lift the roster back into
        // `root.extensions.KHR_xmp_json_ld`.
        if let Some(xroot) = &ext.khr_xmp_json_ld {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "packets".into(),
                serde_json::Value::Array(xroot.packets.clone()),
            );
            scene
                .extras
                .insert("KHR_xmp_json_ld".to_owned(), serde_json::Value::Object(obj));
        }
    }
    // Asset-level KHR_xmp_json_ld packet reference — points at one of
    // the root-level packets per `docs/3d/gltf/extensions/
    // KHR_xmp_json_ld.md` §"Instantiating XMP metadata". Asset-scoped
    // metadata "applies to the entire glTF asset" (spec §Overview).
    // Surfaced under `scene.extras["__asset_xmp_packet"]` because
    // the typed `Scene3D` has no first-class asset field.
    if let Some(aext) = &root.asset.extensions {
        if let Some(xmp) = &aext.khr_xmp_json_ld {
            scene.extras.insert(
                "__asset_xmp_packet".to_owned(),
                serde_json::Value::Number(xmp.packet.into()),
            );
        }
    }
    // Primary-scene KHR_xmp_json_ld packet reference. The active scene
    // (`root.scene` index, defaulting to 0) carries through the same
    // typed `extensions.KHR_xmp_json_ld` block per `docs/3d/gltf/
    // extensions/KHR_xmp_json_ld.md` §"Instantiating XMP metadata".
    // Surfaced under `scene.extras["__primary_scene_xmp_packet"]`.
    if let Some(primary) = root.scenes.get(root.scene.unwrap_or(0) as usize) {
        if let Some(sext) = &primary.extensions {
            if let Some(xmp) = &sext.khr_xmp_json_ld {
                scene.extras.insert(
                    "__primary_scene_xmp_packet".to_owned(),
                    serde_json::Value::Number(xmp.packet.into()),
                );
            }
        }
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
) -> Result<(Animation, Vec<Value>)> {
    let mut anim = Animation::new(a.name.clone());
    anim.channels.reserve(a.channels.len());
    // Pointer channels (KHR_animation_pointer-flagged) are siphoned off
    // into a side roster the caller stashes on `Scene3D::extras`. Per
    // `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Extension
    // Usage": when present, `target.path == "pointer"`, `target.node`
    // MUST NOT be set, and the pointer string lives at
    // `target.extensions.KHR_animation_pointer.pointer`.
    let mut pointer_channels: Vec<Value> = Vec::new();
    for (ci, ch) in a.channels.iter().enumerate() {
        // KHR_animation_pointer detection: a `"pointer"` path or an
        // extension data block. The extension MUST be the only path
        // value (no `node` field allowed). Spec §3.12 enforcement
        // (declared-in-extensionsUsed and node/path consistency) runs
        // in validation.rs; here we just round-trip the data.
        let is_pointer_path = ch.target.path == "pointer";
        let pointer_str = ch
            .target
            .extensions
            .as_ref()
            .and_then(|e| e.khr_animation_pointer.as_ref())
            .map(|p| p.pointer.clone());
        if is_pointer_path || pointer_str.is_some() {
            // Materialise the channel sampler + push onto the side
            // roster as JSON. Per
            // `docs/3d/gltf/extensions/KHR_animation_pointer.md`
            // §"Output Accessor Component Types" (float* Object Model
            // Data Types branch): FLOAT accessor values are used
            // as-is; non-normalized integer values are cast to floats
            // (`1` → `1.0`); normalized integer values dequantise via
            // the §3.6.2.2 equations of the base glTF 2.0 spec. The
            // `int` and `bool` Object Model Data Type branches
            // require an Object Model property registry to dispatch
            // by pointer string; that's a follow-up and isn't reached
            // here — every pointer output decodes to a flat `Vec<f32>`
            // with the source `componentType` + `normalized` flag
            // carried in the side-channel so the encoder can
            // re-emit the original on-the-wire format.
            let pointer = pointer_str.ok_or_else(|| {
                invalid(format!(
                    "animation channel {ci}: target.path = \"pointer\" but \
                     KHR_animation_pointer.pointer is missing (spec)"
                ))
            })?;
            if ch.target.node.is_some() {
                return Err(invalid(format!(
                    "animation channel {ci}: KHR_animation_pointer channels \
                     MUST NOT set target.node (spec §\"Extension Usage\")"
                )));
            }
            if ch.target.path != "pointer" {
                return Err(invalid(format!(
                    "animation channel {ci}: KHR_animation_pointer requires \
                     target.path == \"pointer\", got {:?} (spec §\"Extension Usage\")",
                    ch.target.path
                )));
            }
            let s_idx = ch.sampler as usize;
            let s = a
                .samplers
                .get(s_idx)
                .ok_or_else(|| invalid(format!("animation: sampler {s_idx} out of range")))?;
            let interpolation = match s.interpolation.as_deref() {
                None | Some("LINEAR") => "LINEAR",
                Some("STEP") => "STEP",
                Some("CUBICSPLINE") => "CUBICSPLINE",
                Some(other) => {
                    return Err(unsupported(format!(
                        "animation sampler interpolation {other:?} unknown"
                    )));
                }
            };
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
            let input_values = read_scalar_f32(&in_view)?;
            let output_acc = root
                .accessors
                .get(s.output as usize)
                .ok_or_else(|| invalid(format!("animation sampler output {} oob", s.output)))?;
            let out_bytes = materialise_accessor(output_acc, &root.buffer_views, buffers)?;
            let out_view = view_from_materialised(output_acc, &out_bytes)?;
            let output_kind = output_acc.kind.clone();
            let output_component_type = output_acc.component_type;
            let output_normalized = output_acc.normalized;
            // Read elements per spec §3.6.2.1 layout (arity given by
            // the accessor `type`: SCALAR=1, VEC2=2, VEC3=3, VEC4=4,
            // MAT2=4, MAT3=9, MAT4=16) and dispatch on
            // `componentType` + `normalized` per
            // `docs/3d/gltf/extensions/KHR_animation_pointer.md`
            // §"Output Accessor Component Types" (float* branch).
            let output_values = read_pointer_output_floats(output_acc, &out_view)?;
            // Object-Model data-type dispatch: pointers that resolve
            // through the staged pointer-template registry
            // (`object_model::pointer_data_type`) to a `bool` property
            // surface typed booleans per
            // `docs/3d/gltf/extensions/KHR_animation_pointer.md`
            // §"Output Accessor Component Types": "`0` is converted to
            // `false`, any other value is converted to `true`". The
            // accessor-shape MUSTs (SCALAR / unsigned-byte / STEP) were
            // enforced by `validate_extension_stack` before conversion.
            // Unmatched pointers stay on the `float*` branch.
            let data_type = crate::object_model::pointer_data_type(&pointer);
            let mut obj = serde_json::Map::new();
            obj.insert("channel".into(), Value::from(ci as u32));
            obj.insert("pointer".into(), Value::String(pointer));
            obj.insert("interpolation".into(), Value::String(interpolation.into()));
            obj.insert(
                "input".into(),
                Value::Array(input_values.into_iter().map(json_f32).collect()),
            );
            obj.insert("output_kind".into(), Value::String(output_kind));
            // Carry the source on-the-wire format so the encoder can
            // re-emit the same `componentType` + `normalized` flag.
            // FLOAT + normalized=false is the spec-default lane — emit
            // those defaults to keep the round-trip representation
            // minimal for callers that only ever touch FLOAT outputs.
            obj.insert(
                "output_component_type".into(),
                Value::from(output_component_type),
            );
            obj.insert("output_normalized".into(), Value::Bool(output_normalized));
            match data_type {
                Some(crate::object_model::ObjectModelDataType::Bool) => {
                    // `output_data_type` records the registry hit so
                    // the encoder picks the bool re-emission lane;
                    // sidecars without the key default to the float*
                    // branch (r261-and-earlier documents unchanged).
                    obj.insert("output_data_type".into(), Value::String("bool".into()));
                    obj.insert(
                        "output".into(),
                        Value::Array(
                            output_values
                                .into_iter()
                                .map(|v| Value::Bool(v != 0.0))
                                .collect(),
                        ),
                    );
                }
                None => {
                    obj.insert(
                        "output".into(),
                        Value::Array(output_values.into_iter().map(json_f32).collect()),
                    );
                }
            }
            pointer_channels.push(Value::Object(obj));
            continue;
        }
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
    Ok((anim, pointer_channels))
}

/// Decode every output element of a `KHR_animation_pointer` channel
/// into a flat `Vec<f32>`. Element arity comes from `accessor.type`
/// per spec §3.6.2.1 (SCALAR=1, VEC2=2, VEC3=3, VEC4=4, MAT2=4,
/// MAT3=9, MAT4=16). Per-component conversion follows
/// `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Output
/// Accessor Component Types" (float* Object Model Data Type branch):
/// FLOAT components pass through; non-normalized integer components
/// cast directly to f32 (`1` → `1.0`); normalized integer components
/// dequantise via the §3.6.2.2 equations.
fn read_pointer_output_floats(acc: &gj::Accessor, view: &AccessorView<'_>) -> Result<Vec<f32>> {
    use gj::{
        COMPONENT_TYPE_BYTE, COMPONENT_TYPE_FLOAT, COMPONENT_TYPE_SHORT,
        COMPONENT_TYPE_UNSIGNED_BYTE, COMPONENT_TYPE_UNSIGNED_INT, COMPONENT_TYPE_UNSIGNED_SHORT,
    };
    let arity = match acc.kind.as_str() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        other => {
            return Err(unsupported(format!(
                "KHR_animation_pointer: output accessor type {other:?} not in spec table"
            )))
        }
    };
    let csize = match acc.component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        COMPONENT_TYPE_UNSIGNED_INT | COMPONENT_TYPE_FLOAT => 4,
        other => {
            return Err(unsupported(format!(
                "KHR_animation_pointer: output componentType {other} not in spec table"
            )))
        }
    };
    let expected = arity * csize;
    if view.element_size != expected {
        return Err(invalid(format!(
            "KHR_animation_pointer output accessor: element_size {} != {expected} \
             (kind={:?}, componentType={})",
            view.element_size, acc.kind, acc.component_type
        )));
    }
    let mut out = Vec::with_capacity(view.count * arity);
    for elem in view.elements() {
        for i in 0..arity {
            let off = i * csize;
            let bytes = &elem[off..off + csize];
            out.push(decode_pointer_output_component(
                acc.component_type,
                acc.normalized,
                bytes,
            )?);
        }
    }
    Ok(out)
}

/// One-component conversion for the float* Object Model Data Type
/// branch of KHR_animation_pointer §"Output Accessor Component Types".
/// Normalized integer values use the §3.6.2.2 dequantisation equations
/// from the base glTF 2.0 spec; non-normalized integers cast to f32;
/// FLOAT passes through.
fn decode_pointer_output_component(
    component_type: u32,
    normalized: bool,
    bytes: &[u8],
) -> Result<f32> {
    use gj::{
        COMPONENT_TYPE_BYTE, COMPONENT_TYPE_FLOAT, COMPONENT_TYPE_SHORT,
        COMPONENT_TYPE_UNSIGNED_BYTE, COMPONENT_TYPE_UNSIGNED_INT, COMPONENT_TYPE_UNSIGNED_SHORT,
    };
    Ok(match component_type {
        COMPONENT_TYPE_FLOAT => {
            let b: [u8; 4] = bytes
                .try_into()
                .map_err(|_| invalid("KHR_animation_pointer: short f32 read"))?;
            f32::from_le_bytes(b)
        }
        COMPONENT_TYPE_BYTE => {
            let c = i8::from_le_bytes([bytes[0]]) as f32;
            if normalized {
                (c / 127.0).max(-1.0)
            } else {
                c
            }
        }
        COMPONENT_TYPE_UNSIGNED_BYTE => {
            let c = bytes[0] as f32;
            if normalized {
                c / 255.0
            } else {
                c
            }
        }
        COMPONENT_TYPE_SHORT => {
            let c = i16::from_le_bytes([bytes[0], bytes[1]]) as f32;
            if normalized {
                (c / 32767.0).max(-1.0)
            } else {
                c
            }
        }
        COMPONENT_TYPE_UNSIGNED_SHORT => {
            let c = u16::from_le_bytes([bytes[0], bytes[1]]) as f32;
            if normalized {
                c / 65535.0
            } else {
                c
            }
        }
        COMPONENT_TYPE_UNSIGNED_INT => {
            // Spec line 93 covers non-normalized integers generally;
            // normalized UINT is not used in any ratified extension
            // (no §3.6.2.2 dequantisation row for it), so reject the
            // combination here rather than guess.
            if normalized {
                return Err(unsupported(
                    "KHR_animation_pointer: UNSIGNED_INT output with normalized=true \
                     has no §3.6.2.2 dequantisation row",
                ));
            }
            let c = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            c as f32
        }
        other => {
            return Err(unsupported(format!(
                "KHR_animation_pointer: output componentType {other} not in spec table"
            )))
        }
    })
}

/// Lossless `f32 → JSON Number` (uses f64 widening, finite-only).
fn json_f32(v: f32) -> Value {
    serde_json::Number::from_f64(v as f64)
        .map(Value::Number)
        .unwrap_or(Value::Null)
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
        // Per `docs/3d/gltf/extensions/KHR_meshopt_compression.md`
        // §"Fallback buffers" a buffer marked `{ "extensions":
        // { "KHR_meshopt_compression": { "fallback": true } } }` is a
        // no-data placeholder — it has no `uri`, doesn't refer to the
        // GLB BIN chunk, and only exists so that the parent
        // bufferViews retain a valid `buffer` reference per the
        // base spec. Materialise it as a zero-filled byte vector of
        // the declared `byteLength` so downstream slicing remains
        // safe; a future meshopt decoder lane would inflate the real
        // bytes into this region from the compressed source.
        let is_fallback = b
            .extensions
            .as_ref()
            .and_then(|e| e.khr_meshopt_compression.as_ref())
            .map(|m| m.fallback)
            .unwrap_or(false);
        let bytes = match (&b.uri, is_fallback) {
            (None, true) => vec![0u8; b.byte_length as usize],
            (None, false) => {
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
            (Some(uri), _) if uri.starts_with("data:") => decode_data_uri(uri)?,
            (Some(uri), _) => {
                return Err(unsupported(format!(
                    "buffer[{i}]: external URI {uri:?} not resolved (caller must inline before decode)"
                )));
            }
        };
        out.push(Arc::new(bytes));
    }
    Ok(out)
}

/// Inflate every `KHR_meshopt_compression`-compressed bufferView in
/// place, writing the decompressed bytes into the parent bufferView's
/// region of its backing buffer, per
/// `docs/3d/gltf/extensions/KHR_meshopt_compression.md` Appendix A / B.
///
/// The descriptor's `buffer` / `byteOffset` / `byteLength` locate the
/// compressed source; the meshopt decoder turns it into
/// `byteStride * count` decompressed bytes, which are copied into the
/// parent bufferView's `buffer` at the parent `byteOffset`. The parent
/// `byteLength` MUST be large enough to hold the decompressed result.
fn inflate_meshopt_buffer_views(
    root: &GltfRoot,
    buffers: &mut [std::sync::Arc<Vec<u8>>],
) -> Result<()> {
    for (bvi, bv) in root.buffer_views.iter().enumerate() {
        let Some(mc) = bv
            .extensions
            .as_ref()
            .and_then(|e| e.khr_meshopt_compression.as_ref())
        else {
            continue;
        };

        let mode = crate::meshopt::Mode::parse(&mc.mode)?;
        let filter = crate::meshopt::Filter::parse(mc.filter.as_deref())?;
        let count = mc.count as usize;
        let byte_stride = mc.byte_stride as usize;

        // Compressed source range (descriptor `buffer`).
        let src_buf = buffers.get(mc.buffer as usize).ok_or_else(|| {
            invalid(format!(
                "KHR_meshopt_compression: bufferView[{bvi}] descriptor buffer {} out of range",
                mc.buffer
            ))
        })?;
        let src_off = mc.byte_offset.unwrap_or(0) as usize;
        let src_len = mc.byte_length as usize;
        let src_end = src_off.checked_add(src_len).ok_or_else(|| {
            invalid(format!(
                "KHR_meshopt_compression: bufferView[{bvi}] compressed range overflows"
            ))
        })?;
        if src_end > src_buf.len() {
            return Err(invalid(format!(
                "KHR_meshopt_compression: bufferView[{bvi}] compressed range [{src_off}, {src_end}) \
                 overruns descriptor buffer {} of {} bytes",
                mc.buffer,
                src_buf.len()
            )));
        }
        let compressed = src_buf[src_off..src_end].to_vec();

        let decompressed = crate::meshopt::decode(&compressed, mode, filter, count, byte_stride)?;

        // Destination: the parent bufferView's buffer + offset.
        let dst_off = bv.byte_offset.unwrap_or(0) as usize;
        let need = byte_stride
            .checked_mul(count)
            .ok_or_else(|| invalid("KHR_meshopt_compression: byteStride * count overflows"))?;
        if (bv.byte_length as usize) < need {
            return Err(invalid(format!(
                "KHR_meshopt_compression: bufferView[{bvi}].byteLength {} < decompressed size {need}",
                bv.byte_length
            )));
        }
        let dst_buf = buffers.get_mut(bv.buffer as usize).ok_or_else(|| {
            invalid(format!(
                "KHR_meshopt_compression: bufferView[{bvi}] parent buffer {} out of range",
                bv.buffer
            ))
        })?;
        let dst = std::sync::Arc::get_mut(dst_buf).ok_or_else(|| {
            invalid("KHR_meshopt_compression: parent buffer is shared and cannot be inflated")
        })?;
        let dst_end = dst_off.checked_add(decompressed.len()).ok_or_else(|| {
            invalid("KHR_meshopt_compression: parent destination range overflows")
        })?;
        if dst_end > dst.len() {
            return Err(invalid(format!(
                "KHR_meshopt_compression: bufferView[{bvi}] decompressed bytes do not fit in \
                 parent buffer {} ({} bytes, need [{dst_off}, {dst_end}))",
                bv.buffer,
                dst.len()
            )));
        }
        dst[dst_off..dst_end].copy_from_slice(&decompressed);
    }
    Ok(())
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

fn convert_texture(
    root: &GltfRoot,
    t: &gj::Texture,
    buffers: &[Arc<Vec<u8>>],
) -> Result<(Texture, bool)> {
    let sampler = convert_sampler_index(root, t.sampler);
    // Per `docs/3d/gltf/extensions/KHR_texture_basisu.md` §glTF
    // Schema Updates the extension provides an alternative `source`
    // pointing at a KTX v2 image. Two valid shapes: (a) base
    // `texture.source` present (PNG/JPEG fallback) + extension
    // `source` (KTX2 alternate) — capable engines pick the KTX2,
    // we pick the fallback; (b) base `source` absent, only the
    // extension's `source` — we load that KTX2 image as opaque
    // bytes (the asset MIME `image/ktx2` round-trips through
    // `BufferViewAsset` / `InMemoryAsset`). The boolean half of the
    // return value records whether the loaded image came from the
    // extension so the caller can stash the sidecar.
    let basisu_source = t
        .extensions
        .as_ref()
        .and_then(|e| e.khr_texture_basisu.as_ref())
        .and_then(|b| b.source);
    let (image_idx, used_basisu) = match (t.source, basisu_source) {
        (Some(idx), _) => (idx, false),
        (None, Some(idx)) => (idx, true),
        (None, None) => {
            return Err(invalid(
                "texture: missing source (no base `source` and no \
                 `KHR_texture_basisu.source` indirection)",
            ));
        }
    };
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

    Ok((
        Texture {
            name: t.name.clone(),
            image,
            sampler,
        },
        used_basisu,
    ))
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
        // KHR_materials_anisotropy — anisotropic specular lobe (e.g.
        // brushed metal) on top of the metallic-roughness material per
        // docs/3d/gltf/extensions/KHR_materials_anisotropy.md §Extending
        // Materials. We surface it through
        // `Material::extras["KHR_materials_anisotropy"]` as a JSON object
        // carrying any of the three spec-defined keys
        // (`anisotropyStrength`, `anisotropyRotation`,
        // `anisotropyTexture`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec all three fields are
        // optional; we materialise the scalar defaults
        // (`anisotropyStrength = 0.0`, `anisotropyRotation = 0.0`) so a
        // bare `{}` resolves to a fully-specified record. The texture
        // info passes through verbatim.
        if let Some(an) = &ext.khr_materials_anisotropy {
            let mut obj = serde_json::Map::new();
            let strength = an.anisotropy_strength.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(strength as f64) {
                obj.insert("anisotropyStrength".to_owned(), Value::Number(n));
            }
            let rotation = an.anisotropy_rotation.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(rotation as f64) {
                obj.insert("anisotropyRotation".to_owned(), Value::Number(n));
            }
            if let Some(t) = &an.anisotropy_texture {
                obj.insert("anisotropyTexture".to_owned(), texture_info_to_json(t));
            }
            mat.extras
                .insert("KHR_materials_anisotropy".to_owned(), Value::Object(obj));
        }
        // KHR_materials_dispersion — optical dispersion (chromatic
        // aberration) on top of the metallic-roughness material's
        // volumetric transmission per
        // docs/3d/gltf/extensions/KHR_materials_dispersion.md §Extending
        // Materials. We surface it through
        // `Material::extras["KHR_materials_dispersion"]` as a JSON object
        // carrying the single spec-defined `dispersion` key (`20/Vd`)
        // rather than widening `oxideav_mesh3d::Material`. Per the spec
        // the field is optional with a default of `0.0` (no dispersion,
        // the backwards-compatibility default) so a bare `{}` resolves
        // to a fully-specified record with the default materialised.
        if let Some(dp) = &ext.khr_materials_dispersion {
            let mut obj = serde_json::Map::new();
            let dispersion = dp.dispersion.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(dispersion as f64) {
                obj.insert("dispersion".to_owned(), Value::Number(n));
            }
            mat.extras
                .insert("KHR_materials_dispersion".to_owned(), Value::Object(obj));
        }
        // KHR_materials_diffuse_transmission — models light that
        // diffuses through infinitely-thin surfaces (leaves, paper, wax
        // …) on top of the metallic-roughness material per
        // docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md
        // §Extending Materials. We surface it through
        // `Material::extras["KHR_materials_diffuse_transmission"]` as a
        // JSON object carrying any of the four spec-defined keys
        // (`diffuseTransmissionFactor`, `diffuseTransmissionTexture`,
        // `diffuseTransmissionColorFactor`,
        // `diffuseTransmissionColorTexture`) rather than widening
        // `oxideav_mesh3d::Material`. Per the spec all four fields are
        // optional; we materialise the scalar / colour defaults
        // (`diffuseTransmissionFactor = 0.0`,
        // `diffuseTransmissionColorFactor = [1, 1, 1]`) so a bare `{}`
        // resolves to a fully-specified record. Texture infos pass
        // through verbatim.
        if let Some(dt) = &ext.khr_materials_diffuse_transmission {
            let mut obj = serde_json::Map::new();
            let factor = dt.diffuse_transmission_factor.unwrap_or(0.0);
            if let Some(n) = serde_json::Number::from_f64(factor as f64) {
                obj.insert("diffuseTransmissionFactor".to_owned(), Value::Number(n));
            }
            if let Some(t) = &dt.diffuse_transmission_texture {
                obj.insert(
                    "diffuseTransmissionTexture".to_owned(),
                    texture_info_to_json(t),
                );
            }
            let cf = dt
                .diffuse_transmission_color_factor
                .unwrap_or([1.0, 1.0, 1.0]);
            let cf_arr: Vec<Value> = cf
                .iter()
                .filter_map(|v| serde_json::Number::from_f64(*v as f64).map(Value::Number))
                .collect();
            if cf_arr.len() == 3 {
                obj.insert(
                    "diffuseTransmissionColorFactor".to_owned(),
                    Value::Array(cf_arr),
                );
            }
            if let Some(t) = &dt.diffuse_transmission_color_texture {
                obj.insert(
                    "diffuseTransmissionColorTexture".to_owned(),
                    texture_info_to_json(t),
                );
            }
            mat.extras.insert(
                "KHR_materials_diffuse_transmission".to_owned(),
                Value::Object(obj),
            );
        }
        // KHR_xmp_json_ld — per-material packet reference per
        // `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating
        // XMP metadata". Surface as a bare JSON number under
        // `Material::extras["KHR_xmp_json_ld"]` — the encoder lifts
        // it back through `xmp_packet_from_value` into the typed
        // `extensions.KHR_xmp_json_ld = { packet: N }` block.
        if let Some(xmp) = &ext.khr_xmp_json_ld {
            mat.extras.insert(
                "KHR_xmp_json_ld".to_owned(),
                Value::Number(xmp.packet.into()),
            );
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

    // KHR_mesh_quantization is declared in extensionsUsed when any of
    // this primitive's attribute accessors lift a quantised integer
    // type per the extension table.
    let ext_used = root.extensions_used.iter().any(|e| e == EXTENSION_NAME);

    // Per-attribute quantisation roster: name → {componentType,
    // normalized}. Stashed on the primitive's extras under
    // `__attr_quant` so the encoder can round-trip each attribute in
    // its original form.
    let mut attr_quant = serde_json::Map::new();

    // POSITION is mandatory per spec §3.7.2.1.
    let position_idx = *p
        .attributes
        .get("POSITION")
        .ok_or_else(|| invalid("primitive: missing POSITION attribute"))?;
    prim.positions = read_attr_vec3(root, buffers, position_idx, ext_used, "POSITION")?;
    record_attr_quant(&mut attr_quant, &root.accessors, position_idx, "POSITION");

    if let Some(&i) = p.attributes.get("NORMAL") {
        prim.normals = Some(read_attr_vec3(root, buffers, i, ext_used, "NORMAL")?);
        record_attr_quant(&mut attr_quant, &root.accessors, i, "NORMAL");
    }
    if let Some(&i) = p.attributes.get("TANGENT") {
        let tangents = read_attr_vec4(root, buffers, i, ext_used, "TANGENT")?;
        // Spec §3.7.2.1: TANGENT.w MUST be ±1.0.
        validate_tangent_w(&tangents)?;
        prim.tangents = Some(tangents);
        record_attr_quant(&mut attr_quant, &root.accessors, i, "TANGENT");
    }
    // TEXCOORD_n
    let mut texcoord_idx = 0;
    while let Some(&i) = p.attributes.get(&format!("TEXCOORD_{texcoord_idx}")) {
        let name = format!("TEXCOORD_{texcoord_idx}");
        prim.uvs
            .push(read_attr_vec2(root, buffers, i, ext_used, &name)?);
        record_attr_quant(&mut attr_quant, &root.accessors, i, &name);
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

    // KHR_materials_variants — per-primitive mapping table per
    // `docs/3d/gltf/extensions/KHR_materials_variants.md`. Each
    // mapping pairs a material index with the variant indices that
    // select it. The typed `oxideav_mesh3d::Primitive` carries no
    // variant slot, so we stash the full `mappings` array (preserving
    // material index + variants list + optional name + extras) under
    // `primitive.extras["KHR_materials_variants"]`. The encoder lifts
    // this object back into the typed primitive `extensions` block on
    // write.
    if let Some(ext) = &p.extensions {
        if let Some(vmap) = &ext.khr_materials_variants {
            let arr: Vec<serde_json::Value> = vmap
                .mappings
                .iter()
                .map(|m| {
                    let mut o = serde_json::Map::new();
                    o.insert("material".into(), serde_json::Value::from(m.material));
                    let vlist: Vec<serde_json::Value> = m
                        .variants
                        .iter()
                        .map(|&v| serde_json::Value::from(v))
                        .collect();
                    o.insert("variants".into(), serde_json::Value::Array(vlist));
                    if let Some(name) = &m.name {
                        o.insert("name".into(), serde_json::Value::String(name.clone()));
                    }
                    if let Some(e) = &m.extras {
                        o.insert("extras".into(), e.clone());
                    }
                    serde_json::Value::Object(o)
                })
                .collect();
            let mut obj = serde_json::Map::new();
            obj.insert("mappings".into(), serde_json::Value::Array(arr));
            prim.extras.insert(
                "KHR_materials_variants".to_owned(),
                serde_json::Value::Object(obj),
            );
        }
        // KHR_gaussian_splatting — per-primitive descriptor block per
        // `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Extending
        // Mesh Primitives". The typed `oxideav_mesh3d::Primitive` has no
        // splat slot, so the descriptor (kernel/colorSpace/projection/
        // sortingMethod + optional `extras`) is surfaced through
        // `primitive.extras["KHR_gaussian_splatting"]` for the encoder
        // to lift back on write. The custom attribute semantics
        // (`KHR_gaussian_splatting:ROTATION`, `:SCALE`, `:OPACITY`,
        // `:SH_DEGREE_l_COEF_n`) flow through the standard accessor
        // pipeline as raw attributes; the descriptor object is the
        // primary handshake the renderer needs to switch into splat-
        // rendering mode.
        if let Some(splat) = &ext.khr_gaussian_splatting {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "kernel".into(),
                serde_json::Value::String(splat.kernel.clone()),
            );
            obj.insert(
                "colorSpace".into(),
                serde_json::Value::String(splat.color_space.clone()),
            );
            if let Some(proj) = &splat.projection {
                obj.insert("projection".into(), serde_json::Value::String(proj.clone()));
            }
            if let Some(sort) = &splat.sorting_method {
                obj.insert(
                    "sortingMethod".into(),
                    serde_json::Value::String(sort.clone()),
                );
            }
            if let Some(e) = &splat.extras {
                obj.insert("extras".into(), e.clone());
            }
            prim.extras.insert(
                "KHR_gaussian_splatting".to_owned(),
                serde_json::Value::Object(obj),
            );

            // Typed splat-field decode for the base `"ellipse"` kernel.
            // The base spec defines a per-vertex attribute contract
            // (§"Ellipse Kernel" §"Attributes") — ROTATION (VEC4),
            // SCALE (VEC3), OPACITY (SCALAR), and the
            // `SH_DEGREE_l_COEF_n` colour coefficients (VEC3) — that a
            // splat-aware renderer above this crate consumes alongside
            // POSITION. The typed `oxideav_mesh3d::Primitive` has no
            // splat slot, so we read those raw accessors (the validator
            // has already proven their type + component-type conformance
            // for the ellipse kernel) and surface them as parallel typed
            // arrays under `primitive.extras["__gaussian_splats"]`. The
            // `splatting::SplatField` typed view is reconstructed from
            // these arrays. A vendor-prefixed kernel defers the whole
            // attribute contract to the kernel-defining extension, so we
            // only decode for the base `"ellipse"` kernel.
            if splat.kernel == "ellipse" {
                if let Some(record) = read_gaussian_splat_attributes(root, buffers, &p.attributes)?
                {
                    prim.extras.insert("__gaussian_splats".to_owned(), record);
                }
            }
        }
        // KHR_draco_mesh_compression — per-primitive extension object
        // per `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md`
        // §"glTF Schema Updates". The typed `oxideav_mesh3d::Primitive`
        // has no compressed-payload slot and this crate is a pass-
        // through engine (no Draco bitstream inflate path), so the
        // bufferView indirection + Draco-side attribute-id map are
        // surfaced through `primitive.extras["KHR_draco_mesh_compression"]`
        // for round-trip preservation. The parent primitive's own
        // `attributes` + `indices` accessors are processed normally
        // (per spec §"accessors": the accessors describe the
        // decompressed data and remain authoritative for the
        // uncompressed-fallback lane). Per §Conformance step 4 a Draco-
        // aware consumer would inflate the compressed payload first
        // and rebuild the accessor data from it; we surface enough of
        // the descriptor that such a consumer (layered over this
        // crate) can do so.
        if let Some(draco) = &ext.khr_draco_mesh_compression {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "bufferView".into(),
                serde_json::Value::from(draco.buffer_view),
            );
            let mut attr_obj = serde_json::Map::new();
            for (k, v) in &draco.attributes {
                attr_obj.insert(k.clone(), serde_json::Value::from(*v));
            }
            obj.insert("attributes".into(), serde_json::Value::Object(attr_obj));
            if let Some(e) = &draco.extras {
                obj.insert("extras".into(), e.clone());
            }
            prim.extras.insert(
                "KHR_draco_mesh_compression".to_owned(),
                serde_json::Value::Object(obj),
            );
        }
    }

    // Stash per-attribute quantisation metadata so the encoder can
    // round-trip each attribute in its original component-type form.
    // Only emit the sentinel if at least one attribute is not the
    // spec's default FLOAT — keeping plain FLOAT primitives identical
    // to pre-r179 output.
    if attr_quant.values().any(|v| {
        v.as_object()
            .and_then(|o| o.get("componentType"))
            .and_then(|c| c.as_u64())
            != Some(u64::from(gj::COMPONENT_TYPE_FLOAT))
    }) {
        prim.extras.insert(
            ATTR_QUANT_KEY.to_owned(),
            serde_json::Value::Object(attr_quant),
        );
    }

    // Morph targets (§3.7.2.2). The typed `oxideav_mesh3d::Primitive`
    // model doesn't carry a dedicated field, so we serialise the
    // resolved per-target attribute deltas into a JSON sentinel under
    // `prim.extras["__morph_targets"]` — encoder pulls them back out
    // and re-emits as accessors. Format: an array of objects, one per
    // target, mapping attribute name → array of [f32; N] values.
    //
    // POSITION / NORMAL / TANGENT use VEC3 (TANGENT handedness W is
    // dropped per spec §3.7.2.2) and TEXCOORD_n uses VEC2. When
    // `KHR_mesh_quantization` is declared, accessors may also store
    // the BYTE / SHORT (signed) variants per
    // `docs/3d/gltf/extensions/KHR_mesh_quantization.md` §Extending
    // Morph Target Attributes; the (componentType, normalized) tuple
    // is recorded under the per-primitive `__morph_attr_quant`
    // sentinel keyed by `<target-index>.<attribute>` so the encoder
    // can round-trip the original storage form.
    if !p.targets.is_empty() {
        let mut targets_json = Vec::with_capacity(p.targets.len());
        let mut morph_quant = serde_json::Map::new();
        for (ti, tgt) in p.targets.iter().enumerate() {
            let mut obj = serde_json::Map::new();
            let mut per_target_quant = serde_json::Map::new();
            for (name, &acc_idx) in tgt {
                let acc = root.accessors.get(acc_idx as usize).ok_or_else(|| {
                    invalid(format!(
                        "morph target attribute {name:?}: accessor {acc_idx} oob"
                    ))
                })?;
                let kind = acc.kind.as_str();
                // Spec §3.7.2.2: POSITION / NORMAL / TANGENT morph
                // targets are VEC3; TEXCOORD_n morph targets are VEC2.
                // Anything else is non-conformant regardless of
                // extension state.
                if quantization::base_attr_key_public(name) == "TEXCOORD" {
                    if kind != "VEC2" {
                        return Err(unsupported(format!(
                            "morph target {name:?}: expected VEC2 (got {kind:?})"
                        )));
                    }
                } else if kind != "VEC3" {
                    return Err(unsupported(format!(
                        "morph target {name:?}: expected VEC3 (got {kind:?})"
                    )));
                }

                let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
                let view = view_from_materialised(acc, &bytes)?;

                if acc.component_type == gj::COMPONENT_TYPE_FLOAT {
                    // FLOAT — base-spec path. The sentinel still
                    // round-trips the (componentType=FLOAT,
                    // normalized=false) marker only when at least one
                    // sibling morph attribute is quantised; the
                    // per-target map filtering below skips emitting
                    // pure-FLOAT entries.
                } else {
                    // Quantised integer — must be gated on the
                    // extension being declared AND fall within the
                    // morph-target combo table from
                    // §Extending Morph Target Attributes.
                    if !ext_used {
                        return Err(unsupported(format!(
                            "morph target {name:?} accessor uses componentType {} but {} is not in extensionsUsed",
                            acc.component_type, EXTENSION_NAME
                        )));
                    }
                    if !quantization::is_morph_attr_combo_allowed(
                        name,
                        kind,
                        acc.component_type,
                        acc.normalized,
                    ) {
                        return Err(unsupported(format!(
                            "{} morph target {name:?}: componentType {} (normalized={}, type={kind}) not in the extension's morph-attribute combo table",
                            EXTENSION_NAME, acc.component_type, acc.normalized
                        )));
                    }
                }

                // Dequantise into f32 deltas. FLOAT passes through
                // unchanged via the same helper.
                let arr_value = if kind == "VEC2" {
                    let deltas = quantization::dequantize_vec2(acc, &view)?;
                    deltas_to_json(&deltas)
                } else {
                    let deltas = quantization::dequantize_vec3(acc, &view)?;
                    deltas_to_json(&deltas)
                };
                obj.insert(name.clone(), arr_value);

                // Record the storage form so the encoder can re-emit
                // in the same quantised type. Only record non-FLOAT —
                // the encoder defaults to FLOAT when no entry exists.
                if acc.component_type != gj::COMPONENT_TYPE_FLOAT {
                    let mut entry = serde_json::Map::new();
                    entry.insert(
                        "componentType".to_owned(),
                        serde_json::Value::Number(acc.component_type.into()),
                    );
                    entry.insert(
                        "normalized".to_owned(),
                        serde_json::Value::Bool(acc.normalized),
                    );
                    per_target_quant.insert(name.clone(), serde_json::Value::Object(entry));
                }
            }
            targets_json.push(serde_json::Value::Object(obj));
            if !per_target_quant.is_empty() {
                morph_quant.insert(ti.to_string(), serde_json::Value::Object(per_target_quant));
            }
        }
        prim.extras.insert(
            "__morph_targets".to_owned(),
            serde_json::Value::Array(targets_json),
        );
        if !morph_quant.is_empty() {
            prim.extras.insert(
                quantization::MORPH_ATTR_QUANT_KEY.to_owned(),
                serde_json::Value::Object(morph_quant),
            );
        }
    }
    Ok(prim)
}

/// Pack a f32 VEC3 / VEC2 delta array into a JSON array-of-arrays. Used
/// by morph-target decode to lift the dequantised float deltas into the
/// `__morph_targets` extras sentinel.
fn deltas_to_json<const N: usize>(deltas: &[[f32; N]]) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = deltas
        .iter()
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
    serde_json::Value::Array(arr)
}

/// Read the `KHR_gaussian_splatting` ellipse-kernel splat attributes
/// (`docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Ellipse Kernel"
/// §"Attributes") into a structured JSON record:
///
/// ```json
/// { "count": N,
///   "rotation": [[x,y,z,w], …],   // VEC4 unit quaternions
///   "scale":    [[x,y,z], …],     // VEC3
///   "opacity":  [f, …],           // SCALAR, linear [0,1]
///   "sh":       [[[r,g,b], …], …] }// SH coeffs, evaluate-order outer
/// ```
///
/// The SH coefficients are gathered in the canonical
/// `splatting::evaluate` order — degree 0 first, then each higher degree
/// in turn, lowest order `m` to highest within a degree. The validator
/// has already proven the (type, componentType, normalized) conformance
/// and the degree-completeness contract for the ellipse kernel, so the
/// reads here apply the spec int→float dequantisation per attribute.
///
/// Returns `Ok(None)` when the required attributes are absent (e.g. a
/// non-splat primitive that somehow carried the descriptor) — the
/// caller simply omits the sidecar.
fn read_gaussian_splat_attributes(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    attributes: &HashMap<String, u32>,
) -> Result<Option<Value>> {
    const ROTATION: &str = "KHR_gaussian_splatting:ROTATION";
    const SCALE: &str = "KHR_gaussian_splatting:SCALE";
    const OPACITY: &str = "KHR_gaussian_splatting:OPACITY";

    let (Some(&rot_i), Some(&scale_i), Some(&op_i)) = (
        attributes.get(ROTATION),
        attributes.get(SCALE),
        attributes.get(OPACITY),
    ) else {
        return Ok(None);
    };

    // VEC4 rotation — float / signed-byte-normalized / signed-short-
    // normalized (§"Ellipse Kernel" §"Attributes").
    let rot_acc = root
        .accessors
        .get(rot_i as usize)
        .ok_or_else(|| invalid(format!("splat ROTATION accessor {rot_i} oob")))?;
    let rot_bytes = materialise_accessor(rot_acc, &root.buffer_views, buffers)?;
    let rot_view = view_from_materialised(rot_acc, &rot_bytes)?;
    let rotation = quantization::dequantize_vec4(rot_acc, &rot_view)?;

    // VEC3 scale — float / unsigned-byte(-normalized) / unsigned-short
    // (-normalized).
    let scale_acc = root
        .accessors
        .get(scale_i as usize)
        .ok_or_else(|| invalid(format!("splat SCALE accessor {scale_i} oob")))?;
    let scale_bytes = materialise_accessor(scale_acc, &root.buffer_views, buffers)?;
    let scale_view = view_from_materialised(scale_acc, &scale_bytes)?;
    let scale = quantization::dequantize_vec3(scale_acc, &scale_view)?;

    // SCALAR opacity — float / unsigned-byte-normalized / unsigned-short
    // -normalized.
    let op_acc = root
        .accessors
        .get(op_i as usize)
        .ok_or_else(|| invalid(format!("splat OPACITY accessor {op_i} oob")))?;
    let op_bytes = materialise_accessor(op_acc, &root.buffer_views, buffers)?;
    let op_view = view_from_materialised(op_acc, &op_bytes)?;
    let opacity = quantization::dequantize_scalar(op_acc, &op_view)?;

    // Spherical-harmonics coefficients — every present
    // `SH_DEGREE_l_COEF_n` is a VEC3 of floats. Gather them in canonical
    // evaluate order (degree ascending, m ascending within a degree).
    let mut max_degree = 0u32;
    for name in attributes.keys() {
        if let Some(rest) = name.strip_prefix("KHR_gaussian_splatting:SH_DEGREE_") {
            if let Some((deg_str, _)) = rest.split_once("_COEF_") {
                if let Ok(l) = deg_str.parse::<u32>() {
                    max_degree = max_degree.max(l);
                }
            }
        }
    }
    let mut sh: Vec<Value> = Vec::new();
    for l in 0..=max_degree {
        for n in 0..=(2 * l) {
            let key = format!("KHR_gaussian_splatting:SH_DEGREE_{l}_COEF_{n}");
            let &idx = attributes
                .get(&key)
                .ok_or_else(|| invalid(format!("splat {key} missing (degree-completeness)")))?;
            let acc = root
                .accessors
                .get(idx as usize)
                .ok_or_else(|| invalid(format!("splat {key} accessor {idx} oob")))?;
            let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
            let view = view_from_materialised(acc, &bytes)?;
            let coef = read_vec_f32::<3>(&view)?;
            sh.push(vec3_array_to_json(&coef));
        }
    }

    let count = rotation.len();
    let mut obj = serde_json::Map::new();
    obj.insert("count".into(), Value::from(count as u64));
    obj.insert("rotation".into(), vec4_array_to_json(&rotation));
    obj.insert("scale".into(), vec3_array_to_json(&scale));
    obj.insert(
        "opacity".into(),
        Value::Array(
            opacity
                .iter()
                .filter_map(|&f| serde_json::Number::from_f64(f as f64).map(Value::Number))
                .collect(),
        ),
    );
    obj.insert("sh".into(), Value::Array(sh));
    Ok(Some(Value::Object(obj)))
}

/// Serialise a `&[[f32; 3]]` as a JSON array of 3-element arrays.
fn vec3_array_to_json(data: &[[f32; 3]]) -> Value {
    Value::Array(
        data.iter()
            .map(|v| {
                Value::Array(
                    v.iter()
                        .filter_map(|&c| serde_json::Number::from_f64(c as f64).map(Value::Number))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// Serialise a `&[[f32; 4]]` as a JSON array of 4-element arrays.
fn vec4_array_to_json(data: &[[f32; 4]]) -> Value {
    Value::Array(
        data.iter()
            .map(|v| {
                Value::Array(
                    v.iter()
                        .filter_map(|&c| serde_json::Number::from_f64(c as f64).map(Value::Number))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn read_attr_vec3(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    accessor_idx: u32,
    ext_used: bool,
    attr_name: &str,
) -> Result<Vec<[f32; 3]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.kind != "VEC3" {
        return Err(unsupported(format!(
            "attribute accessor: expected VEC3, got {:?}",
            acc.kind
        )));
    }
    // Non-FLOAT VEC3 storage is only legal when KHR_mesh_quantization is
    // active (extensionsUsed declares it) AND the
    // (componentType, normalized) pair is in the extension's allowed set
    // for this base attribute (§Extending Mesh Attributes).
    if acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        require_quantization_combo(ext_used, attr_name, "VEC3", acc)?;
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        return quantization::dequantize_vec3(acc, &view);
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
    ext_used: bool,
    attr_name: &str,
) -> Result<Vec<[f32; 2]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.kind != "VEC2" {
        return Err(unsupported(format!(
            "TEXCOORD accessor: expected VEC2, got {:?}",
            acc.kind
        )));
    }
    // TEXCOORD allows UNSIGNED_BYTE / UNSIGNED_SHORT *normalized* in the
    // base spec §3.7.2.1 (no extension needed). Any other non-FLOAT
    // form requires KHR_mesh_quantization.
    if acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        if quantization::requires_extension_for_base_attr(
            attr_name,
            "VEC2",
            acc.component_type,
            acc.normalized,
        ) {
            require_quantization_combo(ext_used, attr_name, "VEC2", acc)?;
        } else if !quantization::is_base_attr_combo_allowed(
            attr_name,
            "VEC2",
            acc.component_type,
            acc.normalized,
        ) {
            return Err(unsupported(format!(
                "TEXCOORD accessor: componentType {} (normalized={}) not allowed",
                acc.component_type, acc.normalized
            )));
        }
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        return quantization::dequantize_vec2(acc, &view);
    }
    let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
    let view = view_from_materialised(acc, &bytes)?;
    read_vec_f32::<2>(&view)
}

fn read_attr_vec4(
    root: &GltfRoot,
    buffers: &[Arc<Vec<u8>>],
    accessor_idx: u32,
    ext_used: bool,
    attr_name: &str,
) -> Result<Vec<[f32; 4]>> {
    let acc = root
        .accessors
        .get(accessor_idx as usize)
        .ok_or_else(|| invalid(format!("accessor {accessor_idx} out of range")))?;
    if acc.kind != "VEC4" {
        return Err(unsupported(format!(
            "TANGENT accessor: expected VEC4, got {:?}",
            acc.kind
        )));
    }
    // TANGENT non-FLOAT storage requires KHR_mesh_quantization
    // (byte/short normalized only, per §Extending Mesh Attributes).
    if acc.component_type != gj::COMPONENT_TYPE_FLOAT {
        require_quantization_combo(ext_used, attr_name, "VEC4", acc)?;
        let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
        let view = view_from_materialised(acc, &bytes)?;
        return quantization::dequantize_vec4(acc, &view);
    }
    let bytes = materialise_accessor(acc, &root.buffer_views, buffers)?;
    let view = view_from_materialised(acc, &bytes)?;
    read_vec_f32::<4>(&view)
}

/// Gate a quantised (non-FLOAT) base-mesh attribute on the extension
/// being declared in `extensionsUsed` and the (componentType,
/// normalized) pair being in the extension's allowed set for the named
/// attribute (`POSITION` / `NORMAL` / `TANGENT` / `TEXCOORD_n`).
fn require_quantization_combo(
    ext_used: bool,
    attr_name: &str,
    kind: &str,
    acc: &gj::Accessor,
) -> Result<()> {
    if !ext_used {
        return Err(unsupported(format!(
            "{attr_name} accessor uses componentType {} but {} is not in extensionsUsed",
            acc.component_type,
            quantization::EXTENSION_NAME
        )));
    }
    if !quantization::is_base_attr_combo_allowed(
        attr_name,
        kind,
        acc.component_type,
        acc.normalized,
    ) {
        return Err(unsupported(format!(
            "{} {attr_name} accessor: componentType {} (normalized={}) not in the extension's allowed set",
            quantization::EXTENSION_NAME, acc.component_type, acc.normalized
        )));
    }
    Ok(())
}

/// Record an attribute's storage form (`componentType` + `normalized`)
/// into the per-primitive `__attr_quant` roster so the encoder can
/// round-trip each attribute in its original quantised form. Always
/// records — the caller only emits the sentinel when at least one
/// attribute is non-FLOAT (see [`convert_primitive`]).
fn record_attr_quant(
    map: &mut serde_json::Map<String, serde_json::Value>,
    accessors: &[gj::Accessor],
    accessor_idx: u32,
    attr_name: &str,
) {
    if let Some(acc) = accessors.get(accessor_idx as usize) {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "componentType".to_owned(),
            serde_json::Value::Number(acc.component_type.into()),
        );
        entry.insert(
            "normalized".to_owned(),
            serde_json::Value::Bool(acc.normalized),
        );
        map.insert(attr_name.to_owned(), serde_json::Value::Object(entry));
    }
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
