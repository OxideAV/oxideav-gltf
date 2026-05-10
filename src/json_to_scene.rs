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
//! * Sparse accessors aren't supported — they're an authoring
//!   optimisation rare in shipping content. Round-2 candidate.

use std::collections::HashMap;
use std::sync::Arc;

use oxideav_mesh3d::{
    AlphaMode, Camera, ImageData, Indices, Light, MagFilter, Material, MaterialId, Mesh, MinFilter,
    Node, NodeId, Primitive, Sampler, Scene3D, Texture, TextureId, TextureRef, Topology, Transform,
    WrapMode,
};
use serde_json::Value;

use crate::accessor::{locate, read_indices_u32, read_vec4_u16, read_vec_f32};
use crate::asset_source::BufferViewAsset;
use crate::error::{invalid, unsupported, Error, Result};
use crate::json_model::{self as gj, GltfRoot};

/// Decode a parsed [`GltfRoot`] into a [`Scene3D`], using `glb_bin`
/// (when present) as the backing buffer for buffers with no URI.
pub fn convert(root: &GltfRoot, glb_bin: Option<&[u8]>) -> Result<Scene3D> {
    if root.asset.version != "2.0" {
        return Err(unsupported(format!(
            "gltf: only version 2.0 supported, got {:?}",
            root.asset.version
        )));
    }

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

    if let Some(extras) = &root.extras {
        extras_into(&mut scene.extras, extras.clone());
    }

    Ok(scene)
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
    mat.alpha_mode = match m.alpha_mode.as_deref() {
        Some("MASK") => AlphaMode::Mask {
            cutoff: m.alpha_cutoff.unwrap_or(0.5),
        },
        Some("BLEND") => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    };
    mat.double_sided = m.double_sided;
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
        prim.tangents = Some(read_attr_vec4(root, buffers, i)?);
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
        prim.colors.push(read_attr_color(root, buffers, i)?);
        color_idx += 1;
    }
    if let Some(&i) = p.attributes.get("JOINTS_0") {
        let buf = buffers
            .get(buffer_index_for(root, i)?)
            .ok_or_else(|| invalid("joints: buffer out of range"))?;
        let acc = &root.accessors[i as usize];
        let view = locate(acc, &root.buffer_views, buf)?;
        prim.joints = Some(read_vec4_u16(&view)?);
    }
    if let Some(&i) = p.attributes.get("WEIGHTS_0") {
        let buf = buffers
            .get(buffer_index_for(root, i)?)
            .ok_or_else(|| invalid("weights: buffer out of range"))?;
        let acc = &root.accessors[i as usize];
        let view = locate(acc, &root.buffer_views, buf)?;
        let raw = read_vec_f32::<4>(&view)?;
        prim.weights = Some(raw);
    }

    if let Some(idx_acc) = p.indices {
        let acc = &root.accessors[idx_acc as usize];
        let buf = buffers
            .get(buffer_index_for(root, idx_acc)?)
            .ok_or_else(|| invalid("indices: buffer out of range"))?;
        let view = locate(acc, &root.buffer_views, buf)?;
        let widened = read_indices_u32(acc, &view)?;
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
    let buf = buffers
        .get(buffer_index_for(root, accessor_idx)?)
        .ok_or_else(|| invalid("attribute: buffer out of range"))?;
    let view = locate(acc, &root.buffer_views, buf)?;
    read_vec_f32::<3>(&view)
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
    let buf = buffers
        .get(buffer_index_for(root, accessor_idx)?)
        .ok_or_else(|| invalid("TEXCOORD: buffer out of range"))?;
    let view = locate(acc, &root.buffer_views, buf)?;
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
    let buf = buffers
        .get(buffer_index_for(root, accessor_idx)?)
        .ok_or_else(|| invalid("TANGENT: buffer out of range"))?;
    let view = locate(acc, &root.buffer_views, buf)?;
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
    let buf = buffers
        .get(buffer_index_for(root, accessor_idx)?)
        .ok_or_else(|| invalid("COLOR: buffer out of range"))?;
    let view = locate(acc, &root.buffer_views, buf)?;
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

fn buffer_index_for(root: &GltfRoot, accessor_idx: u32) -> Result<usize> {
    let acc = &root.accessors[accessor_idx as usize];
    let bv_idx = acc
        .buffer_view
        .ok_or_else(|| invalid("accessor missing bufferView"))?;
    let bv = root
        .buffer_views
        .get(bv_idx as usize)
        .ok_or_else(|| invalid(format!("bufferView {bv_idx} out of range")))?;
    Ok(bv.buffer as usize)
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
