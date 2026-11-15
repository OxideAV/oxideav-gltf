//! `KHR_draco_mesh_compression` join: decode the compressed payload of a
//! primitive and present it to the accessor pipeline as ordinary
//! uncompressed accessors.
//!
//! Follows `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md`
//! §Conformance (process the extension first, ignore the accessors'
//! `bufferView` / `byteOffset`, fill the accessors from the decoded
//! geometry) and the mapping of `docs/3d/gltf/draco/spec/09-gltf-join.md`:
//!
//! * the extension's `attributes` values are payload **unique ids**;
//! * accessor element `j` of an attribute is the payload attribute's value
//!   for point `j`, and the accessor `count` is the point count;
//! * the `indices` accessor lists the faces' point indices, three per face,
//!   converted to its declared `componentType`.
//!
//! Declared accessor `min` / `max` are not enforced on decoded data: they
//! describe the source geometry, which quantisation moves slightly.
//!
//! The decoded bytes are appended to one synthetic buffer; each inflated
//! primitive is redirected to fresh accessors over it and loses its
//! extension block, so the rest of the loader sees a plain primitive.
//!
//! **Fallback.** When a payload cannot be decoded (corrupt, or a feature
//! the bitstream specification does not cover) and every accessor the
//! extension maps — plus the `indices` accessor — carries its own
//! `bufferView`, the primitive is left untouched and loads from that
//! uncompressed copy. Without such a copy the decode error is returned.

use std::sync::Arc;

use crate::draco::{self, DataType, DracoAttribute, DracoMesh, Limits};
use crate::error::{Result, invalid, unsupported};
use crate::json_model::{self as gj, GltfRoot};

/// Inflate every Draco-compressed primitive of `root`. Returns the patched
/// document when at least one primitive was inflated; the synthetic buffer
/// holding the decoded data is appended to `buffers`.
pub(crate) fn inflate(
    root: &GltfRoot,
    buffers: &mut Vec<Arc<Vec<u8>>>,
) -> Result<Option<GltfRoot>> {
    let mut patched: Option<GltfRoot> = None;
    let mut blob: Vec<u8> = Vec::new();
    let blob_buffer = u32::try_from(root.buffers.len().max(buffers.len()))
        .map_err(|_| invalid("KHR_draco_mesh_compression: too many buffers"))?;

    for (mi, mesh) in root.meshes.iter().enumerate() {
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            let Some(ext) = prim
                .extensions
                .as_ref()
                .and_then(|e| e.khr_draco_mesh_compression.as_ref())
            else {
                continue;
            };
            let has_uncompressed_copy = ext
                .attributes
                .keys()
                .map(|name| prim.attributes.get(name))
                .chain(prim.indices.as_ref().map(Some))
                .all(|idx| {
                    idx.and_then(|&i| root.accessors.get(i as usize))
                        .is_some_and(|a| a.buffer_view.is_some())
                });
            let pieces = match decode_primitive(root, prim, ext, buffers) {
                Ok(pieces) => pieces,
                Err(_) if has_uncompressed_copy => continue,
                Err(e) => {
                    return Err(match e {
                        crate::Error::Unsupported(m) => unsupported(format!(
                            "meshes[{mi}].primitives[{pi}] KHR_draco_mesh_compression: {m}"
                        )),
                        other => invalid(format!(
                            "meshes[{mi}].primitives[{pi}] KHR_draco_mesh_compression: {other}"
                        )),
                    });
                }
            };

            let doc = patched.get_or_insert_with(|| root.clone());
            let target = &mut doc.meshes[mi].primitives[pi];
            for piece in pieces {
                while !blob.len().is_multiple_of(4) {
                    blob.push(0);
                }
                let overflow = || invalid("KHR_draco_mesh_compression: decoded data exceeds 4 GiB");
                let byte_offset = u32::try_from(blob.len()).map_err(|_| overflow())?;
                let byte_length = u32::try_from(piece.bytes.len()).map_err(|_| overflow())?;
                blob.extend_from_slice(&piece.bytes);
                let view = u32::try_from(doc.buffer_views.len()).map_err(|_| overflow())?;
                doc.buffer_views.push(gj::BufferView {
                    buffer: blob_buffer,
                    byte_offset: Some(byte_offset),
                    byte_length,
                    ..Default::default()
                });
                let mut accessor = piece.accessor;
                accessor.buffer_view = Some(view);
                accessor.byte_offset = None;
                // The declared bounds describe the geometry before lossy
                // compression: the staged transcoder output
                // (`glb-grid4/input.glb`) declares a POSITION max of
                // 0.271749 where the decoded maximum is 0.27129441. They
                // are therefore not checked against the decoded values.
                accessor.min = None;
                accessor.max = None;
                let index = u32::try_from(doc.accessors.len()).map_err(|_| overflow())?;
                doc.accessors.push(accessor);
                match piece.slot {
                    Slot::Attribute(name) => {
                        target.attributes.insert(name, index);
                    }
                    Slot::Indices => target.indices = Some(index),
                }
            }
            if let Some(e) = target.extensions.as_mut() {
                e.khr_draco_mesh_compression = None;
            }
        }
    }

    if let Some(doc) = patched.as_mut() {
        let byte_length = u32::try_from(blob.len())
            .map_err(|_| invalid("KHR_draco_mesh_compression: decoded data exceeds 4 GiB"))?;
        // Keep the two buffer lists index-aligned up to the synthetic slot.
        while doc.buffers.len() < blob_buffer as usize {
            doc.buffers.push(gj::Buffer::default());
        }
        while buffers.len() < blob_buffer as usize {
            buffers.push(Arc::new(Vec::new()));
        }
        doc.buffers.push(gj::Buffer {
            byte_length,
            ..Default::default()
        });
        buffers.push(Arc::new(blob));
    }
    Ok(patched)
}

enum Slot {
    Attribute(String),
    Indices,
}

/// Decoded data for one accessor of a primitive.
struct Piece {
    slot: Slot,
    /// The accessor to publish (a copy of the declared one, or a synthetic
    /// index accessor); its `bufferView` is assigned by the caller.
    accessor: gj::Accessor,
    bytes: Vec<u8>,
}

fn decode_primitive(
    root: &GltfRoot,
    prim: &gj::Primitive,
    ext: &gj::KhrDracoMeshCompression,
    buffers: &[Arc<Vec<u8>>],
) -> Result<Vec<Piece>> {
    if prim.mode.unwrap_or(gj::MODE_TRIANGLES) != gj::MODE_TRIANGLES {
        // `docs/3d/gltf/draco/spec/09-gltf-join.md` §3: only TRIANGLES is
        // specified; the strip mapping was not observed.
        return Err(unsupported(
            "only mode 4 (TRIANGLES) has a specified mapping from the payload's faces",
        ));
    }

    // Locate the payload (ch. 09 §1).
    let view = root
        .buffer_views
        .get(ext.buffer_view as usize)
        .ok_or_else(|| invalid("bufferView out of range"))?;
    let buffer = buffers
        .get(view.buffer as usize)
        .ok_or_else(|| invalid("payload buffer out of range"))?;
    let start = view.byte_offset.unwrap_or(0) as usize;
    let payload = start
        .checked_add(view.byte_length as usize)
        .and_then(|end| buffer.get(start..end))
        .ok_or_else(|| invalid("payload bufferView exceeds its buffer"))?;

    // Accessors the extension fills, with the payload unique id of each.
    let mut mapped: Vec<(&String, u32, &gj::Accessor)> = Vec::with_capacity(ext.attributes.len());
    for (name, &unique_id) in &ext.attributes {
        let accessor = prim
            .attributes
            .get(name)
            .and_then(|&i| root.accessors.get(i as usize))
            .ok_or_else(|| invalid(format!("attribute {name} has no accessor")))?;
        mapped.push((name, unique_id, accessor));
    }
    let Some(point_count) = mapped.first().map(|m| m.2.count) else {
        return Err(invalid("extension maps no attribute"));
    };
    let index_accessor = match prim.indices {
        Some(i) => Some(
            root.accessors
                .get(i as usize)
                .ok_or_else(|| invalid("indices accessor out of range"))?,
        ),
        None => None,
    };

    // The accessors already state the decoded sizes; use them as limits so a
    // payload cannot make the decoder allocate more than the document asks.
    // Room for every mapped attribute plus as much again for payload
    // attributes the extension does not map.
    let mapped_components: u32 = mapped
        .iter()
        .map(|m| u32::from(accessor_components(&m.2.kind).unwrap_or(0)))
        .sum();
    let mut limits = Limits::new().with_max_points(point_count);
    limits = limits.with_max_values_per_point(limits.max_values_per_point + mapped_components);
    if let Some(a) = index_accessor {
        limits = limits.with_max_faces(a.count / 3);
    }
    let mesh = draco::decode_with_limits(payload, &limits)?;
    if mesh.num_points != point_count {
        return Err(invalid(format!(
            "payload has {} points but the accessors declare {point_count}",
            mesh.num_points
        )));
    }

    let mut pieces = Vec::with_capacity(mapped.len() + 1);
    for (name, unique_id, accessor) in mapped {
        if accessor.count != point_count {
            return Err(invalid(format!(
                "accessor of {name} declares {} elements, expected {point_count}",
                accessor.count
            )));
        }
        let attribute = mesh.attribute_by_unique_id(unique_id).ok_or_else(|| {
            invalid(format!(
                "payload has no attribute with unique id {unique_id}"
            ))
        })?;
        pieces.push(Piece {
            slot: Slot::Attribute(name.clone()),
            accessor: accessor.clone(),
            bytes: attribute_bytes(name, attribute, accessor, &mesh)?,
        });
    }
    pieces.push(index_piece(&mesh, index_accessor)?);
    Ok(pieces)
}

/// glTF `componentType` → (payload data type, is integer).
fn component_data_type(component_type: u32) -> Option<DataType> {
    Some(match component_type {
        gj::COMPONENT_TYPE_BYTE => DataType::Int8,
        gj::COMPONENT_TYPE_UNSIGNED_BYTE => DataType::Uint8,
        gj::COMPONENT_TYPE_SHORT => DataType::Int16,
        gj::COMPONENT_TYPE_UNSIGNED_SHORT => DataType::Uint16,
        gj::COMPONENT_TYPE_UNSIGNED_INT => DataType::Uint32,
        gj::COMPONENT_TYPE_FLOAT => DataType::Float32,
        _ => return None,
    })
}

fn accessor_components(kind: &str) -> Option<u8> {
    Some(match kind {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        _ => return None,
    })
}

/// Integer component of a payload value, widened.
fn read_integer(data_type: DataType, b: &[u8]) -> Option<i64> {
    Some(match data_type {
        DataType::Int8 => i64::from(b[0] as i8),
        DataType::Uint8 => i64::from(b[0]),
        DataType::Int16 => i64::from(i16::from_le_bytes([b[0], b[1]])),
        DataType::Uint16 => i64::from(u16::from_le_bytes([b[0], b[1]])),
        DataType::Int32 => i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        DataType::Uint32 => i64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        _ => return None,
    })
}

/// Append `v` as an integer of type `target`; `None` when it does not fit.
fn write_integer(out: &mut Vec<u8>, target: DataType, v: i64) -> Option<()> {
    match target {
        DataType::Int8 => out.push(i8::try_from(v).ok()? as u8),
        DataType::Uint8 => out.push(u8::try_from(v).ok()?),
        DataType::Int16 => out.extend_from_slice(&i16::try_from(v).ok()?.to_le_bytes()),
        DataType::Uint16 => out.extend_from_slice(&u16::try_from(v).ok()?.to_le_bytes()),
        DataType::Uint32 => out.extend_from_slice(&u32::try_from(v).ok()?.to_le_bytes()),
        _ => return None,
    }
    Some(())
}

/// Per-point data of `attribute` in the layout `accessor` declares
/// (ch. 09 §2, §4).
fn attribute_bytes(
    name: &str,
    attribute: &DracoAttribute,
    accessor: &gj::Accessor,
    mesh: &DracoMesh,
) -> Result<Vec<u8>> {
    let target = component_data_type(accessor.component_type)
        .ok_or_else(|| invalid(format!("accessor of {name} has an unknown componentType")))?;
    if accessor_components(&accessor.kind) != Some(attribute.num_components) {
        return Err(invalid(format!(
            "accessor of {name} is {} but the payload attribute has {} components",
            accessor.kind, attribute.num_components
        )));
    }
    let points = mesh.num_points as usize;
    if attribute.data_type == target {
        let mut out = Vec::with_capacity(points * attribute.value_size());
        for p in 0..points {
            out.extend_from_slice(attribute.point_bytes(p));
        }
        return Ok(out);
    }
    // Integer payload data in a different integer width: exact conversion.
    let source = attribute.data_type;
    let size = source.size();
    let mut out =
        Vec::with_capacity(points * usize::from(attribute.num_components) * target.size());
    for p in 0..points {
        for b in attribute.point_bytes(p).chunks_exact(size) {
            read_integer(source, b)
                .and_then(|v| write_integer(&mut out, target, v))
                .ok_or_else(|| {
                    unsupported(format!(
                        "payload attribute for {name} is {source:?} but its accessor declares \
                         componentType {}; only exact integer conversions are specified",
                        accessor.component_type
                    ))
                })?;
        }
    }
    Ok(out)
}

/// The face list as the `indices` accessor (ch. 09 §3). A primitive without
/// an `indices` accessor gets a synthetic unsigned-int one, since the
/// decoded geometry is always indexed.
fn index_piece(mesh: &DracoMesh, declared: Option<&gj::Accessor>) -> Result<Piece> {
    let count = mesh.faces.len() * 3;
    let accessor = match declared {
        Some(a) => {
            if a.count as usize != count {
                return Err(invalid(format!(
                    "payload has {} faces but the indices accessor declares {} elements",
                    mesh.faces.len(),
                    a.count
                )));
            }
            a.clone()
        }
        None => gj::Accessor {
            component_type: gj::COMPONENT_TYPE_UNSIGNED_INT,
            count: u32::try_from(count).map_err(|_| invalid("too many indices"))?,
            kind: "SCALAR".to_owned(),
            ..Default::default()
        },
    };
    let target = match accessor.component_type {
        gj::COMPONENT_TYPE_UNSIGNED_BYTE => DataType::Uint8,
        gj::COMPONENT_TYPE_UNSIGNED_SHORT => DataType::Uint16,
        gj::COMPONENT_TYPE_UNSIGNED_INT => DataType::Uint32,
        _ => return Err(invalid("indices accessor has a non-index componentType")),
    };
    let mut bytes = Vec::with_capacity(count * target.size());
    for &index in mesh.faces.iter().flatten() {
        write_integer(&mut bytes, target, i64::from(index)).ok_or_else(|| {
            invalid(format!(
                "point index {index} does not fit the indices accessor's componentType {}",
                accessor.component_type
            ))
        })?;
    }
    Ok(Piece {
        slot: Slot::Indices,
        accessor,
        bytes,
    })
}
