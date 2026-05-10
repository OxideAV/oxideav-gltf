//! Translate a [`Scene3D`] → [`GltfRoot`] + a single packed binary
//! buffer that holds every accessor + image payload.
//!
//! The encoder walks meshes once, packing each primitive's vertex /
//! index buffers into the running `bin` `Vec<u8>` and emitting one
//! accessor + one bufferView per attribute. Image bytes
//! ([`ImageData::Source`]) are appended too and an `image` JSON entry
//! that references the bufferView is generated. External-URI images
//! pass through verbatim.
//!
//! Output matches glTF spec §3 — every emitted accessor / bufferView
//! reads back round-trip through this crate's decoder.

use std::collections::HashMap;
use std::io::Read;

use oxideav_mesh3d::{
    AlphaMode, Animation, AnimationProperty, AnimationValues, Camera, ImageData, Indices,
    Interpolation, Light, MagFilter, Material, Mesh, MinFilter, Node, Primitive, Sampler, Scene3D,
    Skin, Texture, Topology, Transform, WrapMode,
};

use crate::accessor::{
    pad_to_4, smallest_index_component, write_indices, write_mat4_f32, write_vec4_u16,
    write_vec_f32,
};
use crate::error::{invalid, Result};
use crate::json_model::{self as gj, GltfRoot};

/// Emitted glTF root + the single packed binary buffer.
#[derive(Debug)]
pub struct EncodedScene {
    pub root: GltfRoot,
    pub bin: Vec<u8>,
}

/// Translate `scene` into a glTF JSON document + the matching packed
/// binary buffer (used as the `.glb` BIN chunk or as an external
/// `<basename>.bin` file in JSON form).
pub fn convert(scene: &Scene3D) -> Result<EncodedScene> {
    let mut root = GltfRoot {
        asset: gj::Asset::default(),
        ..Default::default()
    };
    let mut bin: Vec<u8> = Vec::new();

    // --- meshes + accessors + bufferViews ---
    for mesh in &scene.meshes {
        let primitives = mesh
            .primitives
            .iter()
            .map(|p| encode_primitive(p, &mut root, &mut bin))
            .collect::<Result<Vec<_>>>()?;
        // Lift primitive[0]'s `__mesh_extras` sentinel back to mesh-level
        // extras (matches the decoder's stash; loss-tolerant if absent).
        let mesh_extras = mesh
            .primitives
            .first()
            .and_then(|p| p.extras.get("__mesh_extras").cloned());
        root.meshes.push(gj::Mesh {
            primitives,
            name: mesh.name.clone(),
            extras: mesh_extras,
        });
    }

    // --- materials ---
    for mat in &scene.materials {
        root.materials.push(encode_material(mat));
    }

    // --- textures + images + samplers ---
    encode_textures(scene, &mut root, &mut bin)?;

    // --- cameras ---
    for c in &scene.cameras {
        root.cameras.push(encode_camera(*c));
    }

    // --- lights (KHR_lights_punctual) ---
    if !scene.lights.is_empty() {
        let mut lights = Vec::with_capacity(scene.lights.len());
        for l in &scene.lights {
            lights.push(encode_light(*l));
        }
        root.extensions = Some(gj::RootExtensions {
            khr_lights_punctual: Some(gj::KhrLightsPunctualRoot { lights }),
        });
        root.extensions_used.push("KHR_lights_punctual".to_owned());
    }

    // --- skins ---
    // Each Skin points at a Skeleton; the Skin itself only carries
    // (skeleton_id, root_node) so we read the joint roster + IBM
    // matrices off the referenced Skeleton.
    for skin in &scene.skins {
        let s_json = encode_skin(skin, scene, &mut root, &mut bin)?;
        root.skins.push(s_json);
    }

    // --- nodes ---
    for n in &scene.nodes {
        root.nodes.push(encode_node(n, scene));
    }

    // --- animations ---
    for a in &scene.animations {
        let a_json = encode_animation(a, &mut root, &mut bin)?;
        root.animations.push(a_json);
    }

    // --- scene root + multi-scene side-channel ---
    // The decoder stashes secondary scenes under
    // `scene.extras["__additional_scenes"]` so this round-trip block
    // pulls them back out and recreates the original `scenes[]` order.
    let mut effective_extras = scene.extras.clone();
    let additional_scenes = effective_extras.remove("__additional_scenes");
    let scene_extras = if effective_extras.is_empty() {
        None
    } else {
        Some(map_to_value(&effective_extras))
    };
    let primary_scene = gj::Scene {
        nodes: scene.roots.iter().map(|r| r.0).collect(),
        name: None,
        extras: scene_extras.clone(),
    };
    match additional_scenes {
        Some(serde_json::Value::Array(extras_arr)) => {
            // Re-thread secondary scenes back into their original
            // indices; primary slots into the first un-occupied index.
            let mut entries: Vec<(usize, gj::Scene)> = Vec::with_capacity(extras_arr.len());
            for v in extras_arr {
                let obj = match v {
                    serde_json::Value::Object(o) => o,
                    _ => continue,
                };
                let idx = obj.get("__index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let nodes = obj
                    .get("nodes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_u64().map(|n| n as u32))
                            .collect()
                    })
                    .unwrap_or_default();
                let name = obj.get("name").and_then(|v| v.as_str()).map(String::from);
                let extras = obj.get("extras").cloned();
                entries.push((
                    idx,
                    gj::Scene {
                        nodes,
                        name,
                        extras,
                    },
                ));
            }
            let occupied: std::collections::HashSet<usize> =
                entries.iter().map(|(i, _)| *i).collect();
            let mut primary_index = 0usize;
            while occupied.contains(&primary_index) {
                primary_index += 1;
            }
            let max_idx = entries
                .iter()
                .map(|(i, _)| *i)
                .chain(std::iter::once(primary_index))
                .max()
                .unwrap_or(0);
            let mut slots: Vec<Option<gj::Scene>> = vec![None; max_idx + 1];
            slots[primary_index] = Some(primary_scene);
            for (i, s) in entries {
                if i < slots.len() && slots[i].is_none() {
                    slots[i] = Some(s);
                } else {
                    slots.push(Some(s));
                }
            }
            for slot in slots {
                root.scenes.push(slot.unwrap_or_else(gj::Scene::default));
            }
            root.scene = Some(primary_index as u32);
        }
        _ => {
            root.scenes.push(primary_scene);
            root.scene = Some(0);
        }
    }
    root.extras = scene_extras;

    // --- buffer ---
    if !bin.is_empty() {
        root.buffers.push(gj::Buffer {
            byte_length: bin.len() as u32,
            uri: None,
            name: None,
        });
    }

    Ok(EncodedScene { root, bin })
}

// --------- per-element encoders ---------

fn encode_primitive(
    p: &Primitive,
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
) -> Result<gj::Primitive> {
    let mut attributes: HashMap<String, u32> = HashMap::new();

    // POSITION
    let pos_acc = push_vec3_accessor(root, bin, &p.positions, "POSITION", true)?;
    attributes.insert("POSITION".into(), pos_acc);

    if let Some(normals) = &p.normals {
        if normals.len() != p.positions.len() {
            return Err(invalid("primitive: NORMAL count != POSITION count"));
        }
        let acc = push_vec3_accessor(root, bin, normals, "NORMAL", false)?;
        attributes.insert("NORMAL".into(), acc);
    }
    if let Some(tangents) = &p.tangents {
        if tangents.len() != p.positions.len() {
            return Err(invalid("primitive: TANGENT count != POSITION count"));
        }
        let acc = push_vec4_accessor(root, bin, tangents, "TANGENT")?;
        attributes.insert("TANGENT".into(), acc);
    }
    for (i, set) in p.uvs.iter().enumerate() {
        if set.len() != p.positions.len() {
            return Err(invalid("primitive: TEXCOORD count != POSITION count"));
        }
        let acc = push_vec2_accessor(root, bin, set)?;
        attributes.insert(format!("TEXCOORD_{i}"), acc);
    }
    for (i, set) in p.colors.iter().enumerate() {
        if set.len() != p.positions.len() {
            return Err(invalid("primitive: COLOR count != POSITION count"));
        }
        let acc = push_vec4_accessor(root, bin, set, "COLOR")?;
        attributes.insert(format!("COLOR_{i}"), acc);
    }
    if let Some(joints) = &p.joints {
        if joints.len() != p.positions.len() {
            return Err(invalid("primitive: JOINTS_0 count != POSITION count"));
        }
        let acc = push_joints_accessor(root, bin, joints)?;
        attributes.insert("JOINTS_0".into(), acc);
    }
    if let Some(weights) = &p.weights {
        if weights.len() != p.positions.len() {
            return Err(invalid("primitive: WEIGHTS_0 count != POSITION count"));
        }
        let acc = push_vec4_accessor(root, bin, weights, "WEIGHTS_0")?;
        attributes.insert("WEIGHTS_0".into(), acc);
    }

    // Indices
    let indices = match &p.indices {
        None => None,
        Some(Indices::U16(v)) => Some(push_indices_accessor(root, bin, &widen_u16(v))?),
        Some(Indices::U32(v)) => Some(push_indices_accessor(root, bin, v)?),
    };

    let mode = match p.topology {
        Topology::Points => Some(gj::MODE_POINTS),
        Topology::Lines => Some(gj::MODE_LINES),
        Topology::LineLoop => Some(gj::MODE_LINE_LOOP),
        Topology::LineStrip => Some(gj::MODE_LINE_STRIP),
        Topology::Triangles => None, // default → omit
        Topology::TriangleStrip => Some(gj::MODE_TRIANGLE_STRIP),
        Topology::TriangleFan => Some(gj::MODE_TRIANGLE_FAN),
    };

    // Drop the synthetic mesh-extras sentinel when emitting per-primitive extras.
    let mut prim_extras = p.extras.clone();
    prim_extras.remove("__mesh_extras");
    let extras = if prim_extras.is_empty() {
        None
    } else {
        Some(map_to_value(&prim_extras))
    };

    Ok(gj::Primitive {
        attributes,
        indices,
        material: p.material.map(|m| m.0),
        mode,
        extras,
    })
}

/// Push positions / normals into the bin and emit an accessor +
/// bufferView. Positions get min/max bounds (spec §3.6.2 requires it
/// for the POSITION attribute).
fn push_vec3_accessor(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[f32; 3]],
    name: &'static str,
    with_minmax: bool,
) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    write_vec_f32::<3>(bin, data);
    let byte_length = bin.len() - byte_offset;
    let bv = gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: Some(gj::TARGET_ARRAY_BUFFER),
        name: Some(format!("{name}_view")),
    };
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(bv);

    let (min, max) = if with_minmax && !data.is_empty() {
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
        (Some(mn.to_vec()), Some(mx.to_vec()))
    } else {
        (None, None)
    };

    let acc = gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type: gj::COMPONENT_TYPE_FLOAT,
        count: data.len() as u32,
        kind: "VEC3".into(),
        normalized: false,
        min,
        max,
        name: Some(name.into()),
        sparse: None,
    };
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(acc);
    Ok(acc_idx)
}

fn push_vec4_accessor(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[f32; 4]],
    name: &'static str,
) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    write_vec_f32::<4>(bin, data);
    let byte_length = bin.len() - byte_offset;
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: Some(gj::TARGET_ARRAY_BUFFER),
        name: Some(format!("{name}_view")),
    });
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type: gj::COMPONENT_TYPE_FLOAT,
        count: data.len() as u32,
        kind: "VEC4".into(),
        normalized: false,
        min: None,
        max: None,
        name: Some(name.into()),
        sparse: None,
    });
    Ok(acc_idx)
}

fn push_vec2_accessor(root: &mut GltfRoot, bin: &mut Vec<u8>, data: &[[f32; 2]]) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    write_vec_f32::<2>(bin, data);
    let byte_length = bin.len() - byte_offset;
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: Some(gj::TARGET_ARRAY_BUFFER),
        name: Some("TEXCOORD_view".into()),
    });
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type: gj::COMPONENT_TYPE_FLOAT,
        count: data.len() as u32,
        kind: "VEC2".into(),
        normalized: false,
        min: None,
        max: None,
        name: Some("TEXCOORD".into()),
        sparse: None,
    });
    Ok(acc_idx)
}

fn push_joints_accessor(root: &mut GltfRoot, bin: &mut Vec<u8>, data: &[[u16; 4]]) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    write_vec4_u16(bin, data);
    let byte_length = bin.len() - byte_offset;
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: Some(gj::TARGET_ARRAY_BUFFER),
        name: Some("JOINTS_0_view".into()),
    });
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type: gj::COMPONENT_TYPE_UNSIGNED_SHORT,
        count: data.len() as u32,
        kind: "VEC4".into(),
        normalized: false,
        min: None,
        max: None,
        name: Some("JOINTS_0".into()),
        sparse: None,
    });
    Ok(acc_idx)
}

fn push_indices_accessor(root: &mut GltfRoot, bin: &mut Vec<u8>, indices: &[u32]) -> Result<u32> {
    let component_type = smallest_index_component(indices);
    pad_to_4(bin);
    let byte_offset = bin.len();
    write_indices(bin, indices, component_type);
    let byte_length = bin.len() - byte_offset;
    pad_to_4(bin);
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: Some(gj::TARGET_ELEMENT_ARRAY_BUFFER),
        name: Some("indices_view".into()),
    });
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type,
        count: indices.len() as u32,
        kind: "SCALAR".into(),
        normalized: false,
        min: None,
        max: None,
        name: Some("indices".into()),
        sparse: None,
    });
    Ok(acc_idx)
}

fn widen_u16(v: &[u16]) -> Vec<u32> {
    v.iter().map(|&x| x as u32).collect()
}

fn encode_material(m: &Material) -> gj::Material {
    let pbr = gj::PbrMetallicRoughness {
        base_color_factor: Some(m.base_color),
        base_color_texture: m.base_color_texture.map(|t| gj::TextureInfo {
            index: t.texture.0,
            tex_coord: if t.uv_set == 0 { None } else { Some(t.uv_set) },
        }),
        metallic_factor: Some(m.metallic),
        roughness_factor: Some(m.roughness),
        metallic_roughness_texture: m.metallic_roughness_texture.map(|t| gj::TextureInfo {
            index: t.texture.0,
            tex_coord: if t.uv_set == 0 { None } else { Some(t.uv_set) },
        }),
    };
    let normal_texture = m.normal_texture.map(|t| gj::NormalTextureInfo {
        index: t.texture.0,
        tex_coord: if t.uv_set == 0 { None } else { Some(t.uv_set) },
        scale: if (m.normal_scale - 1.0).abs() < f32::EPSILON {
            None
        } else {
            Some(m.normal_scale)
        },
    });
    let occlusion_texture = m.occlusion_texture.map(|t| gj::OcclusionTextureInfo {
        index: t.texture.0,
        tex_coord: if t.uv_set == 0 { None } else { Some(t.uv_set) },
        strength: if (m.occlusion_strength - 1.0).abs() < f32::EPSILON {
            None
        } else {
            Some(m.occlusion_strength)
        },
    });
    let (alpha_mode, alpha_cutoff) = match m.alpha_mode {
        AlphaMode::Opaque => (None, None),
        AlphaMode::Mask { cutoff } => (
            Some("MASK".to_owned()),
            if (cutoff - 0.5).abs() < f32::EPSILON {
                None
            } else {
                Some(cutoff)
            },
        ),
        AlphaMode::Blend => (Some("BLEND".to_owned()), None),
    };
    let extras = if m.extras.is_empty() {
        None
    } else {
        Some(map_to_value(&m.extras))
    };
    gj::Material {
        pbr_metallic_roughness: Some(pbr),
        normal_texture,
        occlusion_texture,
        emissive_factor: if m.emissive_factor == [0.0, 0.0, 0.0] {
            None
        } else {
            Some(m.emissive_factor)
        },
        emissive_texture: m.emissive_texture.map(|t| gj::TextureInfo {
            index: t.texture.0,
            tex_coord: if t.uv_set == 0 { None } else { Some(t.uv_set) },
        }),
        alpha_mode,
        alpha_cutoff,
        double_sided: m.double_sided,
        name: m.name.clone(),
        extras,
    }
}

fn encode_textures(scene: &Scene3D, root: &mut GltfRoot, bin: &mut Vec<u8>) -> Result<()> {
    // De-duplicate samplers so the JSON stays compact.
    let mut sampler_index: Vec<Sampler> = Vec::new();
    let resolve_sampler = |samplers: &mut Vec<Sampler>, s: Sampler| -> u32 {
        for (i, existing) in samplers.iter().enumerate() {
            if *existing == s {
                return i as u32;
            }
        }
        samplers.push(s);
        (samplers.len() - 1) as u32
    };

    for tex in &scene.textures {
        // Image first.
        let image_idx = root.images.len() as u32;
        let image = encode_image(tex, root, bin)?;
        root.images.push(image);

        let s_idx = resolve_sampler(&mut sampler_index, tex.sampler);
        root.textures.push(gj::Texture {
            source: Some(image_idx),
            sampler: Some(s_idx),
            name: tex.name.clone(),
        });
    }

    // Emit the deduplicated samplers in encounter order.
    for s in sampler_index {
        root.samplers.push(encode_sampler(s));
    }
    Ok(())
}

fn encode_image(tex: &Texture, root: &mut GltfRoot, bin: &mut Vec<u8>) -> Result<gj::Image> {
    match &tex.image {
        ImageData::External { uri, mime } => Ok(gj::Image {
            uri: Some(uri.clone()),
            mime_type: mime.clone(),
            buffer_view: None,
            name: tex.name.clone(),
        }),
        ImageData::Source(src) => {
            let mime = src.mime().map(|s| s.to_owned());
            let mut buf = Vec::new();
            src.open()
                .map_err(|e| invalid(format!("image source open: {e}")))?
                .read_to_end(&mut buf)
                .map_err(|e| invalid(format!("image source read: {e}")))?;
            pad_to_4(bin);
            let byte_offset = bin.len();
            bin.extend_from_slice(&buf);
            let byte_length = buf.len();
            let bv_idx = root.buffer_views.len() as u32;
            root.buffer_views.push(gj::BufferView {
                buffer: 0,
                byte_offset: Some(byte_offset as u32),
                byte_length: byte_length as u32,
                byte_stride: None,
                target: None,
                name: Some("image_view".into()),
            });
            Ok(gj::Image {
                uri: None,
                mime_type: mime,
                buffer_view: Some(bv_idx),
                name: tex.name.clone(),
            })
        }
        #[cfg(feature = "registry")]
        ImageData::Embedded(_) => Err(crate::error::unsupported(
            "encoding ImageData::Embedded (decoded VideoFrame) requires re-encoding to PNG/JPEG; round 1 supports Source + External only",
        )),
    }
}

fn encode_sampler(s: Sampler) -> gj::Sampler {
    let mag = match s.mag_filter {
        MagFilter::Nearest => gj::MAG_FILTER_NEAREST,
        MagFilter::Linear => gj::MAG_FILTER_LINEAR,
    };
    let min = match s.min_filter {
        MinFilter::Nearest => gj::MIN_FILTER_NEAREST,
        MinFilter::Linear => gj::MIN_FILTER_LINEAR,
        MinFilter::NearestMipNearest => gj::MIN_FILTER_NEAREST_MIPMAP_NEAREST,
        MinFilter::LinearMipNearest => gj::MIN_FILTER_LINEAR_MIPMAP_NEAREST,
        MinFilter::NearestMipLinear => gj::MIN_FILTER_NEAREST_MIPMAP_LINEAR,
        MinFilter::LinearMipLinear => gj::MIN_FILTER_LINEAR_MIPMAP_LINEAR,
    };
    let wrap_s = wrap_to_int(s.wrap_s);
    let wrap_t = wrap_to_int(s.wrap_t);
    gj::Sampler {
        mag_filter: Some(mag),
        min_filter: Some(min),
        wrap_s: Some(wrap_s),
        wrap_t: Some(wrap_t),
        name: None,
    }
}

fn wrap_to_int(w: WrapMode) -> u32 {
    match w {
        WrapMode::ClampToEdge => gj::WRAP_CLAMP_TO_EDGE,
        WrapMode::MirroredRepeat => gj::WRAP_MIRRORED_REPEAT,
        WrapMode::Repeat => gj::WRAP_REPEAT,
    }
}

fn encode_camera(c: Camera) -> gj::Camera {
    match c {
        Camera::Perspective {
            aspect_ratio,
            yfov,
            znear,
            zfar,
        } => gj::Camera {
            kind: "perspective".into(),
            perspective: Some(gj::CameraPerspective {
                aspect_ratio,
                yfov,
                znear,
                zfar,
            }),
            orthographic: None,
            name: None,
        },
        Camera::Orthographic {
            xmag,
            ymag,
            znear,
            zfar,
        } => gj::Camera {
            kind: "orthographic".into(),
            perspective: None,
            orthographic: Some(gj::CameraOrthographic {
                xmag,
                ymag,
                znear,
                zfar,
            }),
            name: None,
        },
    }
}

fn encode_light(l: Light) -> gj::KhrLight {
    match l {
        Light::Directional { color, intensity } => gj::KhrLight {
            kind: "directional".into(),
            color: Some(color),
            intensity: Some(intensity),
            range: None,
            spot: None,
            name: None,
        },
        Light::Point {
            color,
            intensity,
            range,
        } => gj::KhrLight {
            kind: "point".into(),
            color: Some(color),
            intensity: Some(intensity),
            range,
            spot: None,
            name: None,
        },
        Light::Spot {
            color,
            intensity,
            range,
            inner_cone_angle,
            outer_cone_angle,
        } => gj::KhrLight {
            kind: "spot".into(),
            color: Some(color),
            intensity: Some(intensity),
            range,
            spot: Some(gj::KhrLightSpot {
                inner_cone_angle: Some(inner_cone_angle),
                outer_cone_angle: Some(outer_cone_angle),
            }),
            name: None,
        },
    }
}

fn encode_node(n: &Node, _scene: &Scene3D) -> gj::Node {
    let (matrix, translation, rotation, scale) = match n.transform {
        Transform::Matrix(m) => {
            // Convert row-major-of-columns to column-major flat array per spec.
            let mut flat = [0.0f32; 16];
            for c in 0..4 {
                for r in 0..4 {
                    flat[c * 4 + r] = m[r][c];
                }
            }
            (Some(flat), None, None, None)
        }
        Transform::Trs {
            translation,
            rotation,
            scale,
        } => (
            None,
            if translation == [0.0; 3] {
                None
            } else {
                Some(translation)
            },
            if rotation == [0.0, 0.0, 0.0, 1.0] {
                None
            } else {
                Some(rotation)
            },
            if scale == [1.0, 1.0, 1.0] {
                None
            } else {
                Some(scale)
            },
        ),
    };
    let extensions = n.light.map(|lid| gj::NodeExtensions {
        khr_lights_punctual: Some(gj::NodeLightRef { light: lid.0 }),
    });
    let extras = if n.extras.is_empty() {
        None
    } else {
        Some(map_to_value(&n.extras))
    };
    gj::Node {
        mesh: n.mesh.map(|m| m.0),
        camera: n.camera.map(|c| c.0),
        skin: n.skin.map(|s| s.0),
        children: n.children.iter().map(|c| c.0).collect(),
        matrix,
        translation,
        rotation,
        scale,
        name: n.name.clone(),
        extensions,
        extras,
    }
}

fn map_to_value(map: &HashMap<String, serde_json::Value>) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        out.insert(k.clone(), v.clone());
    }
    serde_json::Value::Object(out)
}

// --- skin / animation encoders -------------------------------------------

fn encode_skin(
    skin: &Skin,
    scene: &Scene3D,
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
) -> Result<gj::Skin> {
    let skel = scene
        .skeletons
        .get(skin.skeleton.0 as usize)
        .ok_or_else(|| invalid(format!("skin: skeleton {} out of range", skin.skeleton.0)))?;

    // IBM accessor (optional — drop the field when no matrices are stored).
    let ibm_acc = if skel.inverse_bind_matrices.is_empty() {
        None
    } else {
        if skel.inverse_bind_matrices.len() != skel.joints.len() {
            return Err(invalid(format!(
                "skin: IBM count {} != joints count {}",
                skel.inverse_bind_matrices.len(),
                skel.joints.len()
            )));
        }
        Some(push_mat4_accessor(
            root,
            bin,
            &skel.inverse_bind_matrices,
            "inverseBindMatrices",
        )?)
    };

    Ok(gj::Skin {
        inverse_bind_matrices: ibm_acc,
        skeleton: skin.root_node.map(|n| n.0),
        joints: skel.joints.iter().map(|j| j.0).collect(),
        name: skel.name.clone(),
        extras: None,
    })
}

fn encode_animation(
    a: &Animation,
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
) -> Result<gj::Animation> {
    let mut samplers: Vec<gj::AnimationSampler> = Vec::with_capacity(a.channels.len());
    let mut channels: Vec<gj::AnimationChannel> = Vec::with_capacity(a.channels.len());

    for ch in &a.channels {
        // Each channel becomes one sampler — we don't bother
        // de-duplicating identical (input, output, interpolation)
        // samplers; round-trip-faithfulness > tightest JSON.
        let s = &ch.sampler;
        if s.values.is_empty() {
            return Err(invalid("animation channel: sampler has no values"));
        }
        // Input — keyframe times. spec §3.11: input accessor MUST have
        // min/max defined.
        let input_acc = push_scalar_f32_accessor_with_minmax(root, bin, &s.keyframes, "input")?;
        // Output, sized per channel path.
        let output_acc = match &s.values {
            AnimationValues::Vec3(v) => push_vec3_accessor(root, bin, v, "output", false)?,
            AnimationValues::Quat(v) => push_vec4_accessor(root, bin, v, "output")?,
            AnimationValues::Scalar(v) => push_scalar_f32_accessor(root, bin, v, "output")?,
        };
        let interpolation = match s.interpolation {
            // LINEAR is the spec default; omit it for a tighter document.
            Interpolation::Linear => None,
            Interpolation::Step => Some("STEP".to_owned()),
            Interpolation::CubicSpline => Some("CUBICSPLINE".to_owned()),
        };
        let sampler_idx = samplers.len() as u32;
        samplers.push(gj::AnimationSampler {
            input: input_acc,
            output: output_acc,
            interpolation,
        });
        let path = match ch.target.property {
            AnimationProperty::Translation => "translation",
            AnimationProperty::Rotation => "rotation",
            AnimationProperty::Scale => "scale",
            AnimationProperty::MorphWeights => "weights",
        };
        channels.push(gj::AnimationChannel {
            sampler: sampler_idx,
            target: gj::AnimationChannelTarget {
                node: Some(ch.target.node.0),
                path: path.to_owned(),
            },
        });
    }

    Ok(gj::Animation {
        channels,
        samplers,
        name: a.name.clone(),
        extras: None,
    })
}

fn push_scalar_f32_accessor(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[f32],
    name: &'static str,
) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    for v in data {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let byte_length = bin.len() - byte_offset;
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: None,
        name: Some(format!("{name}_view")),
    });
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type: gj::COMPONENT_TYPE_FLOAT,
        count: data.len() as u32,
        kind: "SCALAR".into(),
        normalized: false,
        min: None,
        max: None,
        name: Some(name.into()),
        sparse: None,
    });
    Ok(acc_idx)
}

fn push_scalar_f32_accessor_with_minmax(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[f32],
    name: &'static str,
) -> Result<u32> {
    let acc_idx = push_scalar_f32_accessor(root, bin, data, name)?;
    if !data.is_empty() {
        let mut mn = data[0];
        let mut mx = data[0];
        for &v in &data[1..] {
            if v < mn {
                mn = v;
            }
            if v > mx {
                mx = v;
            }
        }
        let acc = root
            .accessors
            .last_mut()
            .expect("just pushed accessor disappeared");
        acc.min = Some(vec![mn]);
        acc.max = Some(vec![mx]);
    }
    Ok(acc_idx)
}

fn push_mat4_accessor(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[[f32; 4]; 4]],
    name: &'static str,
) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    write_mat4_f32(bin, data);
    let byte_length = bin.len() - byte_offset;
    let bv_idx = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(byte_offset as u32),
        byte_length: byte_length as u32,
        byte_stride: None,
        target: None,
        name: Some(format!("{name}_view")),
    });
    let acc_idx = root.accessors.len() as u32;
    root.accessors.push(gj::Accessor {
        buffer_view: Some(bv_idx),
        byte_offset: None,
        component_type: gj::COMPONENT_TYPE_FLOAT,
        count: data.len() as u32,
        kind: "MAT4".into(),
        normalized: false,
        min: None,
        max: None,
        name: Some(name.into()),
        sparse: None,
    });
    Ok(acc_idx)
}

#[allow(dead_code)]
fn _silence(_: &Mesh) {}
