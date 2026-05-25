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
use crate::encoder::QuantizeMode;
use crate::error::{invalid, Result};
use crate::json_model::{self as gj, GltfRoot};

/// Emitted glTF root + the single packed binary buffer.
#[derive(Debug)]
pub struct EncodedScene {
    pub root: GltfRoot,
    pub bin: Vec<u8>,
}

/// Knobs the encoder hands down into accessor-emission helpers.
#[derive(Clone, Copy, Debug, Default)]
pub struct EncodeOptions {
    /// When set, a FLOAT vec/scalar accessor whose zero-element
    /// fraction is at least this value is emitted using `accessor.sparse`
    /// storage (zero base + per-index overrides). Clamped to `[0.0, 1.0]`
    /// at construction time on [`crate::GltfEncoder`].
    pub sparse_threshold: Option<f32>,
    /// Quantisation mode for animation sampler outputs that the spec
    /// allows in normalised-int form (ROTATION VEC4 + MORPH_WEIGHTS
    /// SCALAR). See [`QuantizeMode`].
    pub quantize_animation: QuantizeMode,
}

/// Translate `scene` into a glTF JSON document + the matching packed
/// binary buffer (used as the `.glb` BIN chunk or as an external
/// `<basename>.bin` file in JSON form).
pub fn convert(scene: &Scene3D) -> Result<EncodedScene> {
    convert_with_options(scene, &EncodeOptions::default())
}

/// Translate `scene` with the given encoder knobs.
pub fn convert_with_options(scene: &Scene3D, opts: &EncodeOptions) -> Result<EncodedScene> {
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
            .map(|p| encode_primitive(p, &mut root, &mut bin, opts))
            .collect::<Result<Vec<_>>>()?;
        // Lift primitive[0]'s `__mesh_extras` sentinel back to mesh-level
        // extras (matches the decoder's stash; loss-tolerant if absent).
        let mesh_extras = mesh
            .primitives
            .first()
            .and_then(|p| p.extras.get("__mesh_extras").cloned());
        // Mesh-level morph weights default vector (§3.7.2.2) lives on
        // primitive[0]'s extras under `__mesh_weights`.
        let mesh_weights = mesh
            .primitives
            .first()
            .and_then(|p| p.extras.get("__mesh_weights"))
            .and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_f64().map(|f| f as f32))
                        .collect::<Vec<f32>>()
                })
            });
        root.meshes.push(gj::Mesh {
            primitives,
            name: mesh.name.clone(),
            weights: mesh_weights,
            extras: mesh_extras,
        });
    }

    // --- materials ---
    // Track which per-material KHR extensions any material carries so we
    // can append each to `extensionsUsed` per spec §3.12 — see
    // KHR_materials_unlit.md "Extending Materials",
    // KHR_materials_emissive_strength.md "Extending Materials",
    // KHR_materials_ior.md "Extending Materials",
    // KHR_materials_specular.md "Extending Materials",
    // KHR_materials_clearcoat.md "Extending Materials",
    // KHR_materials_sheen.md "Extending Materials",
    // KHR_materials_transmission.md "Extending Materials",
    // KHR_materials_volume.md "Extending Materials", and
    // KHR_materials_iridescence.md "Extending Materials".
    let mut emitted_unlit = false;
    let mut emitted_emissive_strength = false;
    let mut emitted_ior = false;
    let mut emitted_specular = false;
    let mut emitted_clearcoat = false;
    let mut emitted_sheen = false;
    let mut emitted_transmission = false;
    let mut emitted_volume = false;
    let mut emitted_iridescence = false;
    for mat in &scene.materials {
        let m_json = encode_material(mat);
        if let Some(ext) = m_json.extensions.as_ref() {
            if ext.khr_materials_unlit.is_some() {
                emitted_unlit = true;
            }
            if ext.khr_materials_emissive_strength.is_some() {
                emitted_emissive_strength = true;
            }
            if ext.khr_materials_ior.is_some() {
                emitted_ior = true;
            }
            if ext.khr_materials_specular.is_some() {
                emitted_specular = true;
            }
            if ext.khr_materials_clearcoat.is_some() {
                emitted_clearcoat = true;
            }
            if ext.khr_materials_sheen.is_some() {
                emitted_sheen = true;
            }
            if ext.khr_materials_transmission.is_some() {
                emitted_transmission = true;
            }
            if ext.khr_materials_volume.is_some() {
                emitted_volume = true;
            }
            if ext.khr_materials_iridescence.is_some() {
                emitted_iridescence = true;
            }
        }
        root.materials.push(m_json);
    }
    if emitted_unlit {
        root.extensions_used.push("KHR_materials_unlit".to_owned());
    }
    if emitted_emissive_strength {
        root.extensions_used
            .push("KHR_materials_emissive_strength".to_owned());
    }
    if emitted_ior {
        root.extensions_used.push("KHR_materials_ior".to_owned());
    }
    if emitted_specular {
        root.extensions_used
            .push("KHR_materials_specular".to_owned());
    }
    if emitted_clearcoat {
        root.extensions_used
            .push("KHR_materials_clearcoat".to_owned());
    }
    if emitted_sheen {
        root.extensions_used.push("KHR_materials_sheen".to_owned());
    }
    if emitted_transmission {
        root.extensions_used
            .push("KHR_materials_transmission".to_owned());
    }
    if emitted_volume {
        root.extensions_used.push("KHR_materials_volume".to_owned());
    }
    if emitted_iridescence {
        root.extensions_used
            .push("KHR_materials_iridescence".to_owned());
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
        let s_json = encode_skin(skin, scene, &mut root, &mut bin, opts)?;
        root.skins.push(s_json);
    }

    // --- nodes ---
    for n in &scene.nodes {
        root.nodes.push(encode_node(n, scene));
    }

    // --- animations ---
    for a in &scene.animations {
        let a_json = encode_animation(a, &mut root, &mut bin, opts)?;
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
    opts: &EncodeOptions,
) -> Result<gj::Primitive> {
    let mut attributes: HashMap<String, u32> = HashMap::new();

    // POSITION — spec §3.6.2.1.5 mandates min/max; the sparse path
    // recomputes them from `data` so they remain correct.
    let pos_acc =
        push_vec3_accessor_maybe_sparse(root, bin, &p.positions, "POSITION", true, opts, true)?;
    attributes.insert("POSITION".into(), pos_acc);

    if let Some(normals) = &p.normals {
        if normals.len() != p.positions.len() {
            return Err(invalid("primitive: NORMAL count != POSITION count"));
        }
        let acc = push_vec3_accessor_maybe_sparse(root, bin, normals, "NORMAL", false, opts, true)?;
        attributes.insert("NORMAL".into(), acc);
    }
    if let Some(tangents) = &p.tangents {
        if tangents.len() != p.positions.len() {
            return Err(invalid("primitive: TANGENT count != POSITION count"));
        }
        // Spec §3.7.2.1: TANGENT.w MUST be ±1.0 — the sparse path
        // initialises non-overridden slots to the zero vector, which
        // would yield a w=0 element and fail validation. Stay dense.
        let acc = push_vec4_accessor_maybe_sparse(root, bin, tangents, "TANGENT", opts, false)?;
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
        let acc = push_vec4_accessor_maybe_sparse(root, bin, set, "COLOR", opts, true)?;
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
        let acc = push_vec4_accessor_maybe_sparse(root, bin, weights, "WEIGHTS_0", opts, true)?;
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

    // Drop the synthetic sentinels (`__mesh_extras`, `__mesh_weights`,
    // `__morph_targets`) when emitting per-primitive extras — they're
    // re-materialised through their dedicated JSON paths.
    let mut prim_extras = p.extras.clone();
    let morph_targets_extra = prim_extras.remove("__morph_targets");
    prim_extras.remove("__mesh_extras");
    prim_extras.remove("__mesh_weights");
    let extras = if prim_extras.is_empty() {
        None
    } else {
        Some(map_to_value(&prim_extras))
    };

    // Re-emit morph targets as accessors per §3.7.2.2. Two paths feed
    // this:
    //
    // * Typed `Primitive.targets` (mesh3d ≥ 0.0.3) — preferred when
    //   present. The decoder lifts a glTF document's targets into this
    //   field for forward compat; the legacy extras sentinel keeps
    //   round-tripping for older callers.
    // * `__morph_targets` extras sentinel — only consulted when the
    //   typed field is empty (round 2 compatibility).
    //
    // Both paths produce the same `attribute name → accessor index` map
    // the JSON expects; we write the deltas into the binary buffer the
    // same way standard attributes are written.
    let targets = if !p.targets.is_empty() {
        encode_typed_morph_targets(&p.targets, root, bin)?
    } else {
        decode_morph_targets_extra(morph_targets_extra.as_ref(), root, bin)?
    };

    Ok(gj::Primitive {
        attributes,
        indices,
        material: p.material.map(|m| m.0),
        mode,
        targets,
        extras,
    })
}

/// Encode the typed [`oxideav_mesh3d::MorphTarget`] list onto fresh
/// accessors and produce the per-target `attribute name → accessor`
/// roster for the JSON `targets` array (spec §3.7.2.2).
///
/// Per spec the morph-target attribute set is restricted to POSITION /
/// NORMAL / TANGENT (TANGENT.w is dropped — handedness can't be
/// displaced). POSITION targets get min/max bounds since some
/// validators flag their absence even though the spec only mandates
/// min/max for the base POSITION accessor.
fn encode_typed_morph_targets(
    targets: &[oxideav_mesh3d::MorphTarget],
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
) -> Result<Vec<HashMap<String, u32>>> {
    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        let mut tgt: HashMap<String, u32> = HashMap::new();
        if let Some(pos) = &t.position {
            let acc = push_vec3_accessor(root, bin, pos, "MORPH_POSITION", true)?;
            tgt.insert("POSITION".into(), acc);
        }
        if let Some(nrm) = &t.normal {
            let acc = push_vec3_accessor(root, bin, nrm, "MORPH_NORMAL", false)?;
            tgt.insert("NORMAL".into(), acc);
        }
        if let Some(tan) = &t.tangent {
            let acc = push_vec3_accessor(root, bin, tan, "MORPH_TANGENT", false)?;
            tgt.insert("TANGENT".into(), acc);
        }
        out.push(tgt);
    }
    Ok(out)
}

/// Pull morph-target deltas out of the `__morph_targets` sentinel and
/// emit them back into the JSON document as accessors. Returns the
/// per-target attribute → accessor-index roster ready for inclusion in
/// `gj::Primitive::targets`.
fn decode_morph_targets_extra(
    sentinel: Option<&serde_json::Value>,
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
) -> Result<Vec<HashMap<String, u32>>> {
    let arr = match sentinel {
        Some(serde_json::Value::Array(a)) => a,
        Some(_) => {
            return Err(invalid(
                "primitive.extras[__morph_targets]: expected JSON array",
            ));
        }
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::with_capacity(arr.len());
    for (ti, target_val) in arr.iter().enumerate() {
        let obj = target_val.as_object().ok_or_else(|| {
            invalid(format!(
                "primitive.extras[__morph_targets][{ti}]: expected object"
            ))
        })?;
        let mut tgt: HashMap<String, u32> = HashMap::new();
        for (name, vals) in obj {
            let arr = vals.as_array().ok_or_else(|| {
                invalid(format!(
                    "morph target {name:?}: expected array of [f32; 3] elements"
                ))
            })?;
            let mut deltas: Vec<[f32; 3]> = Vec::with_capacity(arr.len());
            for elem in arr {
                let comps = elem.as_array().ok_or_else(|| {
                    invalid(format!("morph target {name:?}: element must be array"))
                })?;
                if comps.len() != 3 {
                    return Err(invalid(format!(
                        "morph target {name:?}: VEC3 element expected (got len {})",
                        comps.len()
                    )));
                }
                let mut a = [0.0f32; 3];
                for (i, c) in comps.iter().enumerate() {
                    a[i] = c.as_f64().ok_or_else(|| {
                        invalid(format!("morph target {name:?}: non-numeric component"))
                    })? as f32;
                }
                deltas.push(a);
            }
            // Spec §3.7.2.2: POSITION accessors must have min/max.
            // The morph delta for POSITION_0 / POSITION still benefits
            // from the bounds (some validators check it), so emit them
            // when the attribute starts with POSITION.
            let with_minmax = name == "POSITION";
            let acc_idx = push_vec3_accessor(root, bin, &deltas, "MORPH_TARGET", with_minmax)?;
            tgt.insert(name.clone(), acc_idx);
        }
        out.push(tgt);
    }
    Ok(out)
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
    // Pull the `KHR_materials_unlit` flag out of extras (decoder
    // parks it there as `Value::Bool(true)`) into the proper
    // per-material extensions block, so the round-trip lands the
    // extension object back where it came from rather than as a
    // surplus `extras` key. Per the KHR_materials_unlit spec the
    // value is an empty object — anything truthy on our side maps
    // to an emitted `{}`.
    let mut effective_extras = m.extras.clone();
    let unlit_flag = effective_extras
        .remove("KHR_materials_unlit")
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false);
    // KHR_materials_emissive_strength — the decoder parks the scalar in
    // extras as a JSON number; lift it back into the typed extensions
    // block so the round-trip emits the spec object rather than a
    // surplus `extras` key (docs/3d/gltf/extensions/
    // KHR_materials_emissive_strength.md §Parameters).
    let emissive_strength = effective_extras
        .remove("KHR_materials_emissive_strength")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    // KHR_materials_ior — the decoder parks the scalar in extras as a
    // JSON number; lift it back into the typed extensions block so the
    // round-trip emits the spec object rather than a surplus `extras`
    // key (docs/3d/gltf/extensions/KHR_materials_ior.md §Extending
    // Materials).
    let ior = effective_extras
        .remove("KHR_materials_ior")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    // KHR_materials_specular — the decoder parks the whole extension
    // object in extras as a `Value::Object` carrying any of the four
    // spec-defined keys (`specularFactor`, `specularTexture`,
    // `specularColorFactor`, `specularColorTexture`). Lift it back into
    // the typed extensions block so the round-trip emits the spec
    // object rather than a surplus `extras` key
    // (docs/3d/gltf/extensions/KHR_materials_specular.md §Extending
    // Materials).
    let specular = effective_extras
        .remove("KHR_materials_specular")
        .and_then(specular_from_value);
    // KHR_materials_clearcoat — the decoder parks the whole extension
    // object in extras as a `Value::Object` carrying any of the five
    // spec-defined keys (`clearcoatFactor`, `clearcoatTexture`,
    // `clearcoatRoughnessFactor`, `clearcoatRoughnessTexture`,
    // `clearcoatNormalTexture`). Lift it back into the typed extensions
    // block so the round-trip emits the spec object rather than a
    // surplus `extras` key (docs/3d/gltf/extensions/
    // KHR_materials_clearcoat.md §Extending Materials).
    let clearcoat = effective_extras
        .remove("KHR_materials_clearcoat")
        .and_then(clearcoat_from_value);
    // KHR_materials_sheen — the decoder parks the whole extension object
    // in extras as a `Value::Object` carrying any of the four
    // spec-defined keys (`sheenColorFactor`, `sheenColorTexture`,
    // `sheenRoughnessFactor`, `sheenRoughnessTexture`). Lift it back into
    // the typed extensions block so the round-trip emits the spec object
    // rather than a surplus `extras` key (docs/3d/gltf/extensions/
    // KHR_materials_sheen.md §Extending Materials).
    let sheen = effective_extras
        .remove("KHR_materials_sheen")
        .and_then(sheen_from_value);
    // KHR_materials_transmission — the decoder parks the whole extension
    // object in extras as a `Value::Object` carrying either of the two
    // spec-defined keys (`transmissionFactor`, `transmissionTexture`).
    // Lift it back into the typed extensions block so the round-trip
    // emits the spec object rather than a surplus `extras` key
    // (docs/3d/gltf/extensions/KHR_materials_transmission.md
    // §Properties).
    let transmission = effective_extras
        .remove("KHR_materials_transmission")
        .and_then(transmission_from_value);
    // KHR_materials_volume — the decoder parks the whole extension object
    // in extras as a `Value::Object` carrying any of the four spec-defined
    // keys (`thicknessFactor`, `thicknessTexture`, `attenuationDistance`,
    // `attenuationColor`). Lift it back into the typed extensions block so
    // the round-trip emits the spec object rather than a surplus `extras`
    // key (docs/3d/gltf/extensions/KHR_materials_volume.md §Properties).
    let volume = effective_extras
        .remove("KHR_materials_volume")
        .and_then(volume_from_value);
    // KHR_materials_iridescence — the decoder parks the whole extension
    // object in extras as a `Value::Object` carrying any of the six
    // spec-defined keys (`iridescenceFactor`, `iridescenceTexture`,
    // `iridescenceIor`, `iridescenceThicknessMinimum`,
    // `iridescenceThicknessMaximum`, `iridescenceThicknessTexture`). Lift
    // it back into the typed extensions block so the round-trip emits the
    // spec object rather than a surplus `extras` key
    // (docs/3d/gltf/extensions/KHR_materials_iridescence.md §Properties).
    let iridescence = effective_extras
        .remove("KHR_materials_iridescence")
        .and_then(iridescence_from_value);
    let extensions = if unlit_flag
        || emissive_strength.is_some()
        || ior.is_some()
        || specular.is_some()
        || clearcoat.is_some()
        || sheen.is_some()
        || transmission.is_some()
        || volume.is_some()
        || iridescence.is_some()
    {
        Some(gj::MaterialExtensions {
            khr_materials_unlit: if unlit_flag {
                Some(gj::MaterialUnlit {})
            } else {
                None
            },
            khr_materials_emissive_strength: emissive_strength.map(|s| {
                gj::MaterialEmissiveStrength {
                    emissive_strength: Some(s),
                }
            }),
            khr_materials_ior: ior.map(|v| gj::MaterialIor { ior: Some(v) }),
            khr_materials_specular: specular,
            khr_materials_clearcoat: clearcoat,
            khr_materials_sheen: sheen,
            khr_materials_transmission: transmission,
            khr_materials_volume: volume,
            khr_materials_iridescence: iridescence,
        })
    } else {
        None
    };
    let extras = if effective_extras.is_empty() {
        None
    } else {
        Some(map_to_value(&effective_extras))
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
        extensions,
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

// Parse the decoder's `Material::extras["KHR_materials_specular"]` JSON
// object back into the typed `MaterialSpecular` for re-emission. The
// decoder normalises defaults, but consumers may also construct
// partial objects directly; this helper accepts both, ignoring keys
// outside the four spec-defined fields (forward-compatibility with
// future spec revisions). See
// `docs/3d/gltf/extensions/KHR_materials_specular.md`.
fn specular_from_value(v: serde_json::Value) -> Option<gj::MaterialSpecular> {
    let obj = v.as_object()?;
    let factor = obj
        .get("specularFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let color = obj
        .get("specularColorFactor")
        .and_then(|x| x.as_array())
        .and_then(|arr| {
            if arr.len() == 3 {
                let r = arr[0].as_f64()? as f32;
                let g = arr[1].as_f64()? as f32;
                let b = arr[2].as_f64()? as f32;
                Some([r, g, b])
            } else {
                None
            }
        });
    let texture = obj.get("specularTexture").and_then(texture_info_from_value);
    let color_texture = obj
        .get("specularColorTexture")
        .and_then(texture_info_from_value);
    if factor.is_none() && color.is_none() && texture.is_none() && color_texture.is_none() {
        return None;
    }
    Some(gj::MaterialSpecular {
        specular_factor: factor,
        specular_texture: texture,
        specular_color_factor: color,
        specular_color_texture: color_texture,
    })
}

fn texture_info_from_value(v: &serde_json::Value) -> Option<gj::TextureInfo> {
    let obj = v.as_object()?;
    let index = obj.get("index").and_then(|x| x.as_u64())? as u32;
    let tex_coord = obj
        .get("texCoord")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    Some(gj::TextureInfo { index, tex_coord })
}

// Parse a `normalTextureInfo` (index + optional texCoord + optional
// scale) back into the typed `NormalTextureInfo`. Used by the
// `KHR_materials_clearcoat` re-emission path for the extension's
// `clearcoatNormalTexture` per
// `docs/3d/gltf/extensions/KHR_materials_clearcoat.md` §Clearcoat.
fn normal_texture_info_from_value(v: &serde_json::Value) -> Option<gj::NormalTextureInfo> {
    let obj = v.as_object()?;
    let index = obj.get("index").and_then(|x| x.as_u64())? as u32;
    let tex_coord = obj
        .get("texCoord")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    let scale = obj.get("scale").and_then(|x| x.as_f64()).map(|x| x as f32);
    Some(gj::NormalTextureInfo {
        index,
        tex_coord,
        scale,
    })
}

// Parse the decoder's `Material::extras["KHR_materials_clearcoat"]` JSON
// object back into the typed `MaterialClearcoat` for re-emission. The
// decoder normalises the factor defaults, but consumers may also
// construct partial objects directly; this helper accepts both,
// ignoring keys outside the five spec-defined fields. See
// `docs/3d/gltf/extensions/KHR_materials_clearcoat.md` §Clearcoat.
fn clearcoat_from_value(v: serde_json::Value) -> Option<gj::MaterialClearcoat> {
    let obj = v.as_object()?;
    let factor = obj
        .get("clearcoatFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let roughness = obj
        .get("clearcoatRoughnessFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let texture = obj
        .get("clearcoatTexture")
        .and_then(texture_info_from_value);
    let roughness_texture = obj
        .get("clearcoatRoughnessTexture")
        .and_then(texture_info_from_value);
    let normal_texture = obj
        .get("clearcoatNormalTexture")
        .and_then(normal_texture_info_from_value);
    if factor.is_none()
        && roughness.is_none()
        && texture.is_none()
        && roughness_texture.is_none()
        && normal_texture.is_none()
    {
        return None;
    }
    Some(gj::MaterialClearcoat {
        clearcoat_factor: factor,
        clearcoat_texture: texture,
        clearcoat_roughness_factor: roughness,
        clearcoat_roughness_texture: roughness_texture,
        clearcoat_normal_texture: normal_texture,
    })
}

// Parse the decoder's `Material::extras["KHR_materials_sheen"]` JSON
// object back into the typed `MaterialSheen` for re-emission. The
// decoder normalises the colour / roughness defaults, but consumers may
// also construct partial objects directly; this helper accepts both,
// ignoring keys outside the four spec-defined fields. See
// `docs/3d/gltf/extensions/KHR_materials_sheen.md` §Sheen.
fn sheen_from_value(v: serde_json::Value) -> Option<gj::MaterialSheen> {
    let obj = v.as_object()?;
    let color = obj
        .get("sheenColorFactor")
        .and_then(|x| x.as_array())
        .and_then(|arr| {
            if arr.len() == 3 {
                let r = arr[0].as_f64()? as f32;
                let g = arr[1].as_f64()? as f32;
                let b = arr[2].as_f64()? as f32;
                Some([r, g, b])
            } else {
                None
            }
        });
    let roughness = obj
        .get("sheenRoughnessFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let color_texture = obj
        .get("sheenColorTexture")
        .and_then(texture_info_from_value);
    let roughness_texture = obj
        .get("sheenRoughnessTexture")
        .and_then(texture_info_from_value);
    if color.is_none()
        && roughness.is_none()
        && color_texture.is_none()
        && roughness_texture.is_none()
    {
        return None;
    }
    Some(gj::MaterialSheen {
        sheen_color_factor: color,
        sheen_color_texture: color_texture,
        sheen_roughness_factor: roughness,
        sheen_roughness_texture: roughness_texture,
    })
}

// Parse the decoder's `Material::extras["KHR_materials_transmission"]`
// JSON object back into the typed `MaterialTransmission` for re-emission.
// The decoder normalises the factor default, but consumers may also
// construct partial objects directly; this helper accepts both, ignoring
// keys outside the two spec-defined fields. See
// `docs/3d/gltf/extensions/KHR_materials_transmission.md` §Properties.
fn transmission_from_value(v: serde_json::Value) -> Option<gj::MaterialTransmission> {
    let obj = v.as_object()?;
    let factor = obj
        .get("transmissionFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let texture = obj
        .get("transmissionTexture")
        .and_then(texture_info_from_value);
    if factor.is_none() && texture.is_none() {
        return None;
    }
    Some(gj::MaterialTransmission {
        transmission_factor: factor,
        transmission_texture: texture,
    })
}

// Parse the decoder's `Material::extras["KHR_materials_volume"]` JSON
// object back into the typed `MaterialVolume` for re-emission. The
// decoder normalises the thickness / attenuation-colour defaults, but
// consumers may also construct partial objects directly; this helper
// accepts both, ignoring keys outside the four spec-defined fields. See
// `docs/3d/gltf/extensions/KHR_materials_volume.md` §Properties.
fn volume_from_value(v: serde_json::Value) -> Option<gj::MaterialVolume> {
    let obj = v.as_object()?;
    let thickness = obj
        .get("thicknessFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let thickness_texture = obj
        .get("thicknessTexture")
        .and_then(texture_info_from_value);
    let attenuation_distance = obj
        .get("attenuationDistance")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let attenuation_color = obj
        .get("attenuationColor")
        .and_then(|x| x.as_array())
        .and_then(|arr| {
            if arr.len() == 3 {
                let r = arr[0].as_f64()? as f32;
                let g = arr[1].as_f64()? as f32;
                let b = arr[2].as_f64()? as f32;
                Some([r, g, b])
            } else {
                None
            }
        });
    if thickness.is_none()
        && thickness_texture.is_none()
        && attenuation_distance.is_none()
        && attenuation_color.is_none()
    {
        return None;
    }
    Some(gj::MaterialVolume {
        thickness_factor: thickness,
        thickness_texture,
        attenuation_distance,
        attenuation_color,
    })
}

// Parse the decoder's `Material::extras["KHR_materials_iridescence"]`
// JSON object back into the typed `MaterialIridescence` for re-emission.
// The decoder normalises the scalar defaults, but consumers may also
// construct partial objects directly; this helper accepts both, ignoring
// keys outside the six spec-defined fields. See
// `docs/3d/gltf/extensions/KHR_materials_iridescence.md` §Properties.
fn iridescence_from_value(v: serde_json::Value) -> Option<gj::MaterialIridescence> {
    let obj = v.as_object()?;
    let factor = obj
        .get("iridescenceFactor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let texture = obj
        .get("iridescenceTexture")
        .and_then(texture_info_from_value);
    let ior_val = obj
        .get("iridescenceIor")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let thmin = obj
        .get("iridescenceThicknessMinimum")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let thmax = obj
        .get("iridescenceThicknessMaximum")
        .and_then(|x| x.as_f64())
        .map(|x| x as f32);
    let thickness_texture = obj
        .get("iridescenceThicknessTexture")
        .and_then(texture_info_from_value);
    if factor.is_none()
        && texture.is_none()
        && ior_val.is_none()
        && thmin.is_none()
        && thmax.is_none()
        && thickness_texture.is_none()
    {
        return None;
    }
    Some(gj::MaterialIridescence {
        iridescence_factor: factor,
        iridescence_texture: texture,
        iridescence_ior: ior_val,
        iridescence_thickness_minimum: thmin,
        iridescence_thickness_maximum: thmax,
        iridescence_thickness_texture: thickness_texture,
    })
}

// --- skin / animation encoders -------------------------------------------

fn encode_skin(
    skin: &Skin,
    scene: &Scene3D,
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    opts: &EncodeOptions,
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
        Some(push_mat4_accessor_maybe_sparse(
            root,
            bin,
            &skel.inverse_bind_matrices,
            "inverseBindMatrices",
            opts,
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
    opts: &EncodeOptions,
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
        // Output, sized per channel path. Sparse-encoding heuristic
        // applies to outputs whose semantic identity is "all zero":
        // translation Vec3 and morph-weight Scalar. Rotation
        // (identity quaternion is `[0,0,0,1]`) and scale (identity
        // `[1,1,1]`) keep dense storage so a zero-base sparse
        // accessor doesn't mis-represent the implicit values.
        let allow_sparse = matches!(
            ch.target.property,
            AnimationProperty::Translation | AnimationProperty::MorphWeights
        );
        let output_acc = match &s.values {
            AnimationValues::Vec3(v) => {
                push_vec3_accessor_maybe_sparse(root, bin, v, "output", false, opts, allow_sparse)?
            }
            AnimationValues::Quat(v) => {
                // Sparse takes precedence: the heuristic only applies
                // to TRANSLATION + MORPH_WEIGHTS (allow_sparse is false
                // here for ROTATION) so we can quantise unconditionally
                // when the mode requests it.
                match opts.quantize_animation {
                    QuantizeMode::Float => push_vec4_accessor(root, bin, v, "output")?,
                    QuantizeMode::UByte => push_vec4_accessor_quantized(
                        root,
                        bin,
                        v,
                        "output",
                        gj::COMPONENT_TYPE_UNSIGNED_BYTE,
                    )?,
                    QuantizeMode::UShort => push_vec4_accessor_quantized(
                        root,
                        bin,
                        v,
                        "output",
                        gj::COMPONENT_TYPE_UNSIGNED_SHORT,
                    )?,
                    QuantizeMode::IByte => push_vec4_accessor_quantized(
                        root,
                        bin,
                        v,
                        "output",
                        gj::COMPONENT_TYPE_BYTE,
                    )?,
                    QuantizeMode::IShort => push_vec4_accessor_quantized(
                        root,
                        bin,
                        v,
                        "output",
                        gj::COMPONENT_TYPE_SHORT,
                    )?,
                }
            }
            AnimationValues::Scalar(v) => {
                // Quantisation and sparse are mutually exclusive: a
                // zero-base sparse accessor with overrides as
                // normalised ints would mix two different value
                // representations. When sparse fires, prefer it (it
                // already discards the zero entries entirely); when it
                // doesn't fire, honour the quantize mode.
                let take_sparse = allow_sparse && opts.sparse_threshold.is_some();
                if take_sparse {
                    push_scalar_f32_accessor_maybe_sparse(
                        root,
                        bin,
                        v,
                        "output",
                        opts,
                        allow_sparse,
                    )?
                } else {
                    match opts.quantize_animation {
                        QuantizeMode::Float => push_scalar_f32_accessor(root, bin, v, "output")?,
                        QuantizeMode::UByte => push_scalar_f32_accessor_quantized(
                            root,
                            bin,
                            v,
                            "output",
                            gj::COMPONENT_TYPE_UNSIGNED_BYTE,
                        )?,
                        QuantizeMode::UShort => push_scalar_f32_accessor_quantized(
                            root,
                            bin,
                            v,
                            "output",
                            gj::COMPONENT_TYPE_UNSIGNED_SHORT,
                        )?,
                        QuantizeMode::IByte => push_scalar_f32_accessor_quantized(
                            root,
                            bin,
                            v,
                            "output",
                            gj::COMPONENT_TYPE_BYTE,
                        )?,
                        QuantizeMode::IShort => push_scalar_f32_accessor_quantized(
                            root,
                            bin,
                            v,
                            "output",
                            gj::COMPONENT_TYPE_SHORT,
                        )?,
                    }
                }
            }
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

/// Quantise a `Vec<f32>` to a normalised-int component type per spec
/// §3.6.2.2 dequantisation equations (run in reverse). The decoder
/// will return `c / 255.0` (UBYTE), `c / 65535.0` (USHORT),
/// `max(c / 127, -1)` (BYTE), or `max(c / 32767, -1)` (SHORT), so we
/// compute the inverse and clamp to the representable range. Width
/// is determined by `component_type`.
fn push_scalar_f32_accessor_quantized(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[f32],
    name: &'static str,
    component_type: u32,
) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    match component_type {
        gj::COMPONENT_TYPE_UNSIGNED_BYTE => {
            for &v in data {
                bin.push(quantize_u8(v));
            }
        }
        gj::COMPONENT_TYPE_UNSIGNED_SHORT => {
            for &v in data {
                bin.extend_from_slice(&quantize_u16(v).to_le_bytes());
            }
        }
        gj::COMPONENT_TYPE_BYTE => {
            for &v in data {
                bin.extend_from_slice(&quantize_i8(v).to_le_bytes());
            }
        }
        gj::COMPONENT_TYPE_SHORT => {
            for &v in data {
                bin.extend_from_slice(&quantize_i16(v).to_le_bytes());
            }
        }
        other => {
            return Err(invalid(format!(
                "quantized scalar accessor: unsupported componentType {other}"
            )));
        }
    }
    let byte_length = bin.len() - byte_offset;
    pad_to_4(bin);
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
        component_type,
        count: data.len() as u32,
        kind: "SCALAR".into(),
        normalized: true,
        min: None,
        max: None,
        name: Some(name.into()),
        sparse: None,
    });
    Ok(acc_idx)
}

/// Quantise a `Vec<[f32; 4]>` (rotation quaternion stream) into
/// normalised UBYTE / USHORT / BYTE / SHORT VEC4 entries. Same
/// per-component equations as the scalar form, applied four times
/// per element.
fn push_vec4_accessor_quantized(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[f32; 4]],
    name: &'static str,
    component_type: u32,
) -> Result<u32> {
    pad_to_4(bin);
    let byte_offset = bin.len();
    match component_type {
        gj::COMPONENT_TYPE_UNSIGNED_BYTE => {
            for v in data {
                for &c in v {
                    bin.push(quantize_u8(c));
                }
            }
        }
        gj::COMPONENT_TYPE_UNSIGNED_SHORT => {
            for v in data {
                for &c in v {
                    bin.extend_from_slice(&quantize_u16(c).to_le_bytes());
                }
            }
        }
        gj::COMPONENT_TYPE_BYTE => {
            for v in data {
                for &c in v {
                    bin.extend_from_slice(&quantize_i8(c).to_le_bytes());
                }
            }
        }
        gj::COMPONENT_TYPE_SHORT => {
            for v in data {
                for &c in v {
                    bin.extend_from_slice(&quantize_i16(c).to_le_bytes());
                }
            }
        }
        other => {
            return Err(invalid(format!(
                "quantized vec4 accessor: unsupported componentType {other}"
            )));
        }
    }
    let byte_length = bin.len() - byte_offset;
    pad_to_4(bin);
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
        component_type,
        count: data.len() as u32,
        kind: "VEC4".into(),
        normalized: true,
        min: None,
        max: None,
        name: Some(name.into()),
        sparse: None,
    });
    Ok(acc_idx)
}

/// Round-clamp `f` into a `u8` per `f = c / 255` inverted. Negatives
/// clamp to 0; > 1.0 clamps to 255. NaNs map to 0.
fn quantize_u8(f: f32) -> u8 {
    if !f.is_finite() {
        return 0;
    }
    let scaled = (f.clamp(0.0, 1.0) * 255.0).round();
    scaled as u8
}

/// Round-clamp `f` into a `u16` per `f = c / 65535` inverted.
fn quantize_u16(f: f32) -> u16 {
    if !f.is_finite() {
        return 0;
    }
    let scaled = (f.clamp(0.0, 1.0) * 65535.0).round();
    scaled as u16
}

/// Round-clamp `f` into an `i8` per `f = max(c / 127, -1)` inverted.
/// Spec §3.6.2.2 reserves the `-128` slot so the dequantised range
/// stays symmetric — we clamp to `[-127, 127]`. NaNs map to 0.
fn quantize_i8(f: f32) -> i8 {
    if !f.is_finite() {
        return 0;
    }
    let scaled = (f.clamp(-1.0, 1.0) * 127.0).round();
    scaled.clamp(-127.0, 127.0) as i8
}

/// Round-clamp `f` into an `i16` per `f = max(c / 32767, -1)` inverted.
/// Spec §3.6.2.2 reserves `-32768` — clamp to `[-32767, 32767]`.
fn quantize_i16(f: f32) -> i16 {
    if !f.is_finite() {
        return 0;
    }
    let scaled = (f.clamp(-1.0, 1.0) * 32767.0).round();
    scaled.clamp(-32767.0, 32767.0) as i16
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

/// Sparse-aware VEC4 accessor emitter (mesh TANGENT / COLOR_0 /
/// WEIGHTS_0 attributes). Falls back to dense when the heuristic
/// decides not to or when `allow_sparse` is false.
fn push_vec4_accessor_maybe_sparse(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[f32; 4]],
    name: &'static str,
    opts: &EncodeOptions,
    allow_sparse: bool,
) -> Result<u32> {
    if allow_sparse {
        if let Some(nonzero) = maybe_sparse_indices_vec4(data, opts) {
            let sparse = build_sparse_block_vec4(root, bin, &nonzero, data);
            let acc_idx = root.accessors.len() as u32;
            root.accessors.push(gj::Accessor {
                buffer_view: None,
                byte_offset: None,
                component_type: gj::COMPONENT_TYPE_FLOAT,
                count: data.len() as u32,
                kind: "VEC4".into(),
                normalized: false,
                min: None,
                max: None,
                name: Some(name.into()),
                sparse: Some(sparse),
            });
            return Ok(acc_idx);
        }
    }
    push_vec4_accessor(root, bin, data, name)
}

/// MAT4 sparse-encoding heuristic — an element is "zero" iff every
/// one of its 16 components is exactly 0.0. When the zero fraction
/// crosses the configured threshold, emit using `accessor.sparse`
/// storage (zero-base, no bufferView; the decoder initialises every
/// matrix to the all-zero MAT4 and overlays only the non-zero entries).
///
/// Note: per spec §3.6.2.3 the sparse `count` MUST be > 0, so an
/// all-zero accessor stays dense.
fn maybe_sparse_indices_mat4(data: &[[[f32; 4]; 4]], opts: &EncodeOptions) -> Option<Vec<u32>> {
    let threshold = opts.sparse_threshold?;
    if data.is_empty() {
        return None;
    }
    let nonzero: Vec<u32> = data
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let any_nonzero = m.iter().any(|row| row.iter().any(|&c| c != 0.0));
            if any_nonzero {
                Some(i as u32)
            } else {
                None
            }
        })
        .collect();
    let zero_fraction = 1.0 - (nonzero.len() as f32 / data.len() as f32);
    if !nonzero.is_empty() && zero_fraction >= threshold {
        Some(nonzero)
    } else {
        None
    }
}

/// Push the indices+values bufferViews for a sparse MAT4 accessor
/// (zero-base) and return the constructed `Sparse` block. Values are
/// written column-major to match the dense path (spec §3.6.2.4).
fn build_sparse_block_mat4(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    indices: &[u32],
    source: &[[[f32; 4]; 4]],
) -> gj::AccessorSparse {
    let max_idx = indices.iter().copied().max().unwrap_or(0);
    let idx_ct = smallest_sparse_index_component(max_idx);
    pad_to_4(bin);
    let idx_offset = bin.len();
    crate::accessor::write_indices(bin, indices, idx_ct);
    let idx_len = bin.len() - idx_offset;
    let idx_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(idx_offset as u32),
        byte_length: idx_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_indices_view".into()),
    });
    pad_to_4(bin);
    let val_offset = bin.len();
    // Same column-major layout as the dense `write_mat4_f32` helper.
    for &i in indices {
        let m = &source[i as usize];
        for c in 0..4 {
            for row in m.iter().take(4) {
                bin.extend_from_slice(&row[c].to_le_bytes());
            }
        }
    }
    let val_len = bin.len() - val_offset;
    let val_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(val_offset as u32),
        byte_length: val_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_values_view".into()),
    });
    gj::AccessorSparse {
        count: indices.len() as u32,
        indices: gj::AccessorSparseIndices {
            buffer_view: idx_bv,
            byte_offset: None,
            component_type: idx_ct,
        },
        values: gj::AccessorSparseValues {
            buffer_view: val_bv,
            byte_offset: None,
        },
    }
}

/// Sparse-aware MAT4 accessor emitter — falls back to dense when the
/// heuristic decides not to. Used for `skin.inverseBindMatrices`
/// where a heavily-symmetric rig may carry many all-zero rows for
/// unused joints; emitting sparse for that case shrinks the buffer
/// roughly proportionally to the zero fraction.
fn push_mat4_accessor_maybe_sparse(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[[f32; 4]; 4]],
    name: &'static str,
    opts: &EncodeOptions,
) -> Result<u32> {
    if let Some(nonzero) = maybe_sparse_indices_mat4(data, opts) {
        let sparse = build_sparse_block_mat4(root, bin, &nonzero, data);
        let acc_idx = root.accessors.len() as u32;
        root.accessors.push(gj::Accessor {
            buffer_view: None,
            byte_offset: None,
            component_type: gj::COMPONENT_TYPE_FLOAT,
            count: data.len() as u32,
            kind: "MAT4".into(),
            normalized: false,
            min: None,
            max: None,
            name: Some(name.into()),
            sparse: Some(sparse),
        });
        return Ok(acc_idx);
    }
    push_mat4_accessor(root, bin, data, name)
}

#[allow(dead_code)]
fn _silence(_: &Mesh) {}

// --- sparse-encoding heuristic helpers -----------------------------------

/// Decide whether `data` should be sparse-encoded given the threshold.
/// Returns `Some(zero_indices)` if sparse should be used (the indices
/// of the *non-zero* elements — i.e., the slots that need overrides);
/// `None` for dense.
fn maybe_sparse_indices_scalar(data: &[f32], opts: &EncodeOptions) -> Option<Vec<u32>> {
    let threshold = opts.sparse_threshold?;
    if data.is_empty() {
        return None;
    }
    let nonzero: Vec<u32> = data
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| if v != 0.0 { Some(i as u32) } else { None })
        .collect();
    let zero_fraction = 1.0 - (nonzero.len() as f32 / data.len() as f32);
    // Spec §3.6.2.3: sparse `count` must be > 0 (the schema marks it
    // `minimum: 1`). All-zero accessors stay dense — no overrides
    // means the sparse block would be invalid.
    if !nonzero.is_empty() && zero_fraction >= threshold {
        Some(nonzero)
    } else {
        None
    }
}

/// VEC4 variant — an element is "zero" iff all four components are 0.0.
fn maybe_sparse_indices_vec4(data: &[[f32; 4]], opts: &EncodeOptions) -> Option<Vec<u32>> {
    let threshold = opts.sparse_threshold?;
    if data.is_empty() {
        return None;
    }
    let nonzero: Vec<u32> = data
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            if v[0] != 0.0 || v[1] != 0.0 || v[2] != 0.0 || v[3] != 0.0 {
                Some(i as u32)
            } else {
                None
            }
        })
        .collect();
    let zero_fraction = 1.0 - (nonzero.len() as f32 / data.len() as f32);
    if !nonzero.is_empty() && zero_fraction >= threshold {
        Some(nonzero)
    } else {
        None
    }
}

/// VEC3 variant — an element is "zero" iff all three components are 0.0.
fn maybe_sparse_indices_vec3(data: &[[f32; 3]], opts: &EncodeOptions) -> Option<Vec<u32>> {
    let threshold = opts.sparse_threshold?;
    if data.is_empty() {
        return None;
    }
    let nonzero: Vec<u32> = data
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            if v[0] != 0.0 || v[1] != 0.0 || v[2] != 0.0 {
                Some(i as u32)
            } else {
                None
            }
        })
        .collect();
    let zero_fraction = 1.0 - (nonzero.len() as f32 / data.len() as f32);
    if !nonzero.is_empty() && zero_fraction >= threshold {
        Some(nonzero)
    } else {
        None
    }
}

/// Pick the smallest spec-allowed componentType for a sparse-indices
/// array of `count` displaced elements. Mirrors the dense
/// [`smallest_index_component`] helper but with the indices' own
/// upper-bound (sparse indices are 0..base.count − 1).
fn smallest_sparse_index_component(max_index: u32) -> u32 {
    if max_index <= u8::MAX as u32 {
        crate::json_model::COMPONENT_TYPE_UNSIGNED_BYTE
    } else if max_index <= u16::MAX as u32 {
        crate::json_model::COMPONENT_TYPE_UNSIGNED_SHORT
    } else {
        crate::json_model::COMPONENT_TYPE_UNSIGNED_INT
    }
}

/// Push the indices+values bufferViews for a sparse accessor whose
/// base is implicit-zero, then return the constructed `Sparse` block.
fn build_sparse_block_scalar(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    indices: &[u32],
    nonzero_values: &[f32],
) -> gj::AccessorSparse {
    let max_idx = indices.iter().copied().max().unwrap_or(0);
    let idx_ct = smallest_sparse_index_component(max_idx);
    pad_to_4(bin);
    let idx_offset = bin.len();
    crate::accessor::write_indices(bin, indices, idx_ct);
    let idx_len = bin.len() - idx_offset;
    let idx_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(idx_offset as u32),
        byte_length: idx_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_indices_view".into()),
    });
    pad_to_4(bin);
    let val_offset = bin.len();
    for v in nonzero_values {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let val_len = bin.len() - val_offset;
    let val_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(val_offset as u32),
        byte_length: val_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_values_view".into()),
    });
    gj::AccessorSparse {
        count: indices.len() as u32,
        indices: gj::AccessorSparseIndices {
            buffer_view: idx_bv,
            byte_offset: None,
            component_type: idx_ct,
        },
        values: gj::AccessorSparseValues {
            buffer_view: val_bv,
            byte_offset: None,
        },
    }
}

/// VEC3 variant — same layout, three f32 per non-zero entry.
fn build_sparse_block_vec3(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    indices: &[u32],
    source: &[[f32; 3]],
) -> gj::AccessorSparse {
    let max_idx = indices.iter().copied().max().unwrap_or(0);
    let idx_ct = smallest_sparse_index_component(max_idx);
    pad_to_4(bin);
    let idx_offset = bin.len();
    crate::accessor::write_indices(bin, indices, idx_ct);
    let idx_len = bin.len() - idx_offset;
    let idx_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(idx_offset as u32),
        byte_length: idx_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_indices_view".into()),
    });
    pad_to_4(bin);
    let val_offset = bin.len();
    for &i in indices {
        let v = source[i as usize];
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let val_len = bin.len() - val_offset;
    let val_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(val_offset as u32),
        byte_length: val_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_values_view".into()),
    });
    gj::AccessorSparse {
        count: indices.len() as u32,
        indices: gj::AccessorSparseIndices {
            buffer_view: idx_bv,
            byte_offset: None,
            component_type: idx_ct,
        },
        values: gj::AccessorSparseValues {
            buffer_view: val_bv,
            byte_offset: None,
        },
    }
}

/// VEC4 variant — same layout, four f32 per non-zero entry.
fn build_sparse_block_vec4(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    indices: &[u32],
    source: &[[f32; 4]],
) -> gj::AccessorSparse {
    let max_idx = indices.iter().copied().max().unwrap_or(0);
    let idx_ct = smallest_sparse_index_component(max_idx);
    pad_to_4(bin);
    let idx_offset = bin.len();
    crate::accessor::write_indices(bin, indices, idx_ct);
    let idx_len = bin.len() - idx_offset;
    let idx_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(idx_offset as u32),
        byte_length: idx_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_indices_view".into()),
    });
    pad_to_4(bin);
    let val_offset = bin.len();
    for &i in indices {
        let v = source[i as usize];
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let val_len = bin.len() - val_offset;
    let val_bv = root.buffer_views.len() as u32;
    root.buffer_views.push(gj::BufferView {
        buffer: 0,
        byte_offset: Some(val_offset as u32),
        byte_length: val_len as u32,
        byte_stride: None,
        target: None,
        name: Some("sparse_values_view".into()),
    });
    gj::AccessorSparse {
        count: indices.len() as u32,
        indices: gj::AccessorSparseIndices {
            buffer_view: idx_bv,
            byte_offset: None,
            component_type: idx_ct,
        },
        values: gj::AccessorSparseValues {
            buffer_view: val_bv,
            byte_offset: None,
        },
    }
}

/// Sparse-aware scalar f32 accessor emitter — falls back to dense when
/// the heuristic decides not to (or when `allow_sparse` is false).
fn push_scalar_f32_accessor_maybe_sparse(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[f32],
    name: &'static str,
    opts: &EncodeOptions,
    allow_sparse: bool,
) -> Result<u32> {
    if allow_sparse {
        if let Some(nonzero) = maybe_sparse_indices_scalar(data, opts) {
            let nz_values: Vec<f32> = nonzero.iter().map(|&i| data[i as usize]).collect();
            let sparse = build_sparse_block_scalar(root, bin, &nonzero, &nz_values);
            let acc_idx = root.accessors.len() as u32;
            root.accessors.push(gj::Accessor {
                buffer_view: None,
                byte_offset: None,
                component_type: gj::COMPONENT_TYPE_FLOAT,
                count: data.len() as u32,
                kind: "SCALAR".into(),
                normalized: false,
                min: None,
                max: None,
                name: Some(name.into()),
                sparse: Some(sparse),
            });
            return Ok(acc_idx);
        }
    }
    push_scalar_f32_accessor(root, bin, data, name)
}

/// Sparse-aware VEC3 accessor emitter (used by animation translation
/// outputs and mesh POSITION/NORMAL/TANGENT attributes). On the
/// sparse path the data still flows through the decoder as
/// zero-base + overrides, so the min/max bounds — which describe
/// the dequantised result, not the buffer layout — stay correct
/// when computed from `data` directly. POSITION accessors must
/// keep them per spec §3.6.2.1.5.
fn push_vec3_accessor_maybe_sparse(
    root: &mut GltfRoot,
    bin: &mut Vec<u8>,
    data: &[[f32; 3]],
    name: &'static str,
    with_minmax: bool,
    opts: &EncodeOptions,
    allow_sparse: bool,
) -> Result<u32> {
    if allow_sparse {
        if let Some(nonzero) = maybe_sparse_indices_vec3(data, opts) {
            let sparse = build_sparse_block_vec3(root, bin, &nonzero, data);
            // Compute min/max from the *post-overlay* data (which is
            // exactly `data` itself — zero base + overrides reproduces
            // the dense values). POSITION attributes need them per spec.
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
            let acc_idx = root.accessors.len() as u32;
            root.accessors.push(gj::Accessor {
                buffer_view: None,
                byte_offset: None,
                component_type: gj::COMPONENT_TYPE_FLOAT,
                count: data.len() as u32,
                kind: "VEC3".into(),
                normalized: false,
                min,
                max,
                name: Some(name.into()),
                sparse: Some(sparse),
            });
            return Ok(acc_idx);
        }
    }
    push_vec3_accessor(root, bin, data, name, with_minmax)
}
