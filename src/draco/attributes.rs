//! Attribute section: groups, descriptors, value blocks and the output
//! transforms (`docs/3d/gltf/draco/spec/05-attributes.md`).

use super::edgebreaker::{Connectivity, NIL};
use super::entropy::{BitDecoder, EntropyScratch, read_symbols};
use super::predict::{
    FlagStream, MeshRefs, Octahedral, Positions, Transform, Wrap, constrained_multi_parallelogram,
    difference, geometric_normals, parallelogram, read_orientations, texture_coordinates,
};
use super::reader::{Reader, unfold};
use super::sequencing::{Sequence, View, sequence};
use super::{DataType, DracoAttribute, Limits, Semantic};
use crate::error::{Result, invalid, unsupported};
use std::cell::OnceCell;

const KIND_VERBATIM: u8 = 0;
const KIND_INTEGER: u8 = 1;
const KIND_QUANTISED: u8 = 2;
const KIND_OCTAHEDRAL: u8 = 3;

const PREDICT_NONE: i8 = -2;
const PREDICT_DIFFERENCE: i8 = 0;
const PREDICT_PARALLELOGRAM: i8 = 1;
const PREDICT_MULTI_PARALLELOGRAM: i8 = 4;
const PREDICT_TEXTURE_COORDINATES: i8 = 5;
const PREDICT_GEOMETRIC_NORMAL: i8 = 6;

const TRANSFORM_WRAP: i8 = 1;
const TRANSFORM_OCTAHEDRAL: i8 = 3;

/// Where the attribute section takes its entry order from.
pub(crate) enum Topology<'a> {
    /// Method 0: one entry per point, in point order.
    Sequential { num_points: u32 },
    /// Method 1: entries ordered by a walk over the decoded mesh.
    Edgebreaker(&'a Connectivity),
}

/// Traversal descriptor of a method 1 group (§1).
#[derive(Clone, Copy, Debug)]
struct Traversal {
    seam_set: i8,
    element_kind: u8,
    traversal_order: u8,
}

/// Attribute descriptor (§2) plus its coding kind.
#[derive(Clone, Copy, Debug)]
struct Descriptor {
    semantic: Semantic,
    data_type: DataType,
    num_components: u8,
    normalized: bool,
    unique_id: u32,
    kind: u8,
}

impl Descriptor {
    /// Stored integers per entry (`C` of §3).
    fn stored_components(&self) -> usize {
        if self.kind == KIND_OCTAHEDRAL {
            2
        } else {
            usize::from(self.num_components)
        }
    }
}

/// Values of one attribute between its value block and its transform
/// parameters.
enum Block<'a> {
    Verbatim(&'a [u8]),
    Integers(Vec<i32>),
}

fn read_group_header(r: &mut Reader<'_>) -> Result<Vec<Descriptor>> {
    let n = r.count()?;
    // Each attribute takes at least five descriptor bytes and a kind byte.
    if n == 0 || n > r.remaining() / 6 {
        return Err(invalid("draco: bad attribute count in group"));
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let semantic = Semantic::from_code(r.u8()?)
            .ok_or_else(|| invalid("draco: unknown attribute semantic"))?;
        let data_type = DataType::from_code(r.u8()?)
            .ok_or_else(|| invalid("draco: unknown attribute data type"))?;
        let num_components = r.u8()?;
        if num_components == 0 {
            return Err(invalid("draco: attribute with zero components"));
        }
        let normalized = r.u8()? != 0;
        let unique_id = r.varint32()?;
        out.push(Descriptor {
            semantic,
            data_type,
            num_components,
            normalized,
            unique_id,
            kind: 0,
        });
    }
    for d in &mut out {
        d.kind = r.u8()?;
        match d.kind {
            KIND_VERBATIM => {}
            KIND_INTEGER => {
                if !matches!(
                    d.data_type,
                    DataType::Int8
                        | DataType::Uint8
                        | DataType::Int16
                        | DataType::Uint16
                        | DataType::Int32
                        | DataType::Uint32
                ) {
                    // 64-bit and bool targets of the integer kind are not
                    // covered by the specification (ch. 10 §3).
                    return Err(unsupported(
                        "draco: integer-coded attribute with a data type the specification does not cover",
                    ));
                }
            }
            KIND_QUANTISED => {
                if d.data_type != DataType::Float32 {
                    return Err(invalid("draco: quantised attribute must be float32"));
                }
            }
            KIND_OCTAHEDRAL => {
                if d.data_type != DataType::Float32 || d.num_components != 3 {
                    return Err(invalid(
                        "draco: octahedral attribute must be 3-component float32",
                    ));
                }
            }
            _ => return Err(invalid("draco: unknown attribute coding kind")),
        }
    }
    Ok(out)
}

/// Decoder state shared by the value blocks of the attribute section.
struct Section<'s> {
    scratch: &'s mut EntropyScratch,
    symbols: Vec<u32>,
    trace: &'s mut Option<Vec<String>>,
    /// Remaining budget of stored integers across all attributes.
    budget: u64,
    edgebreaker: bool,
}

impl Section<'_> {
    fn trace_tuples(&mut self, label: &str, d: &[i32], c: usize) {
        if let Some(t) = self.trace.as_mut() {
            for (e, tuple) in d.chunks_exact(c.max(1)).enumerate() {
                let mut line = format!("{label}[{e}] =");
                for v in tuple {
                    line.push_str(&format!(" {v}"));
                }
                t.push(line);
            }
        }
    }

    /// Integer value block (§6): returns the reconstructed array `D`.
    fn read_integer_block<'a>(
        &mut self,
        r: &mut Reader<'a>,
        desc: &Descriptor,
        entries: usize,
        mesh: Option<&MeshRefs<'_>>,
        positions: Option<Positions<'_>>,
    ) -> Result<Vec<i32>> {
        let c = desc.stored_components();
        let n = entries
            .checked_mul(c)
            .filter(|&n| n as u64 <= self.budget)
            .ok_or_else(|| invalid("draco: attribute values above the decoder limit"))?;
        self.budget -= n as u64;

        let mut method = r.i8()?;
        let octahedral = desc.kind == KIND_OCTAHEDRAL;
        if method != PREDICT_NONE {
            let transform = r.i8()?;
            let wanted = if octahedral {
                TRANSFORM_OCTAHEDRAL
            } else {
                TRANSFORM_WRAP
            };
            if transform != wanted {
                return Err(invalid(
                    "draco: prediction transform does not fit the attribute",
                ));
            }
            let mesh_method = matches!(
                method,
                PREDICT_PARALLELOGRAM
                    | PREDICT_MULTI_PARALLELOGRAM
                    | PREDICT_TEXTURE_COORDINATES
                    | PREDICT_GEOMETRIC_NORMAL
            );
            if method != PREDICT_DIFFERENCE && !mesh_method {
                return Err(unsupported(format!(
                    "draco: prediction method {method} is not specified"
                )));
            }
            if mesh_method {
                if !self.edgebreaker {
                    // Ch. 05 §6.3: a method 0 stream has difference only.
                    method = PREDICT_DIFFERENCE;
                } else if octahedral != (method == PREDICT_GEOMETRIC_NORMAL)
                    || (method == PREDICT_TEXTURE_COORDINATES && c != 2)
                {
                    return Err(unsupported(
                        "draco: prediction method outside the specified combinations",
                    ));
                }
            }
        }

        // §6.1 stored values.
        let mut d: Vec<i32> = Vec::new();
        let fold = method == PREDICT_NONE || !octahedral;
        let entropy_coded = r.u8()? != 0;
        if entropy_coded {
            read_symbols(r, n, c, self.scratch, &mut self.symbols)?;
            d.reserve_exact(n);
            if fold {
                d.extend(self.symbols.iter().map(|&u| unfold(u)));
            } else {
                d.extend(self.symbols.iter().map(|&u| u as i32));
            }
        } else {
            let width = usize::from(r.u8()?);
            if !(1..=4).contains(&width) {
                return Err(invalid("draco: bad raw integer width"));
            }
            let total = n
                .checked_mul(width)
                .ok_or_else(|| invalid("draco: raw integer block too large"))?;
            let raw = r.take(total)?;
            d.reserve_exact(n);
            d.extend(raw.chunks_exact(width).map(|b| {
                let mut le = [0u8; 4];
                le[..width].copy_from_slice(b);
                let u = u32::from_le_bytes(le);
                if fold { unfold(u) } else { u as i32 }
            }));
        }
        self.trace_tuples("residual", &d, c);
        if method == PREDICT_NONE {
            self.trace_tuples("integer", &d, c);
            return Ok(d);
        }

        // §6.3 prediction data and reconstruction.
        let read_transform = |r: &mut Reader<'a>| -> Result<Transform> {
            Ok(if octahedral {
                Transform::Octahedral(Octahedral::read(r)?)
            } else {
                Transform::Wrap(Wrap::read(r)?)
            })
        };
        let need_mesh = || invalid("draco: mesh predictor without mesh tables");
        let need_positions =
            || invalid("draco: predictor needs a decoded 3-component integer position attribute");
        match method {
            PREDICT_DIFFERENCE => {
                let tf = read_transform(r)?;
                difference(&mut d, c, &tf);
            }
            PREDICT_PARALLELOGRAM => {
                let tf = read_transform(r)?;
                parallelogram(&mut d, c, mesh.ok_or_else(need_mesh)?, &tf, self.trace);
            }
            PREDICT_MULTI_PARALLELOGRAM => {
                let mut flags = [
                    FlagStream::read(r)?,
                    FlagStream::read(r)?,
                    FlagStream::read(r)?,
                    FlagStream::read(r)?,
                ];
                let tf = read_transform(r)?;
                constrained_multi_parallelogram(
                    &mut d,
                    c,
                    mesh.ok_or_else(need_mesh)?,
                    &mut flags,
                    &tf,
                    self.trace,
                )?;
            }
            PREDICT_TEXTURE_COORDINATES => {
                let mut orientations = read_orientations(r, entries)?;
                let tf = read_transform(r)?;
                texture_coordinates(
                    &mut d,
                    mesh.ok_or_else(need_mesh)?,
                    &positions.ok_or_else(need_positions)?,
                    &mut orientations,
                    &tf,
                    self.trace,
                )?;
            }
            _ => {
                let oct = Octahedral::read(r)?;
                let mut flips = BitDecoder::read(r)?;
                geometric_normals(
                    &mut d,
                    mesh.ok_or_else(need_mesh)?,
                    &positions.ok_or_else(need_positions)?,
                    &oct,
                    &mut flips,
                    self.trace,
                )?;
            }
        }
        self.trace_tuples("integer", &d, c);
        Ok(d)
    }
}

/// Transform parameters of one attribute (§7, §8) and conversion of its
/// block to output bytes.
fn finish_attribute(r: &mut Reader<'_>, desc: &Descriptor, block: &Block<'_>) -> Result<Vec<u8>> {
    match (desc.kind, block) {
        (KIND_VERBATIM, Block::Verbatim(bytes)) => Ok(bytes.to_vec()),
        (KIND_INTEGER, Block::Integers(d)) => {
            // §9: truncation to the data type's width.
            let width = desc.data_type.size();
            let mut out = Vec::with_capacity(d.len() * width);
            for &v in d {
                out.extend_from_slice(&v.to_le_bytes()[..width]);
            }
            Ok(out)
        }
        (KIND_QUANTISED, Block::Integers(d)) => {
            let c = usize::from(desc.num_components);
            let mut min_value = Vec::with_capacity(c);
            for _ in 0..c {
                min_value.push(r.f32()?);
            }
            let range = r.f32()?;
            let bits = u32::from(r.u8()?);
            if !(1..=30).contains(&bits) {
                return Err(invalid("draco: quantisation bits out of range"));
            }
            let max_q = (1u32 << bits) - 1;
            let delta = range / max_q as f32;
            let mut out = vec![0u8; d.len() * 4];
            for (dst, tuple) in out.chunks_exact_mut(4 * c).zip(d.chunks_exact(c)) {
                for ((bytes, &v), &lo) in dst
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(tuple)
                    .zip(&min_value)
                {
                    let scaled = v as f32 * delta;
                    *bytes = (scaled + lo).to_le_bytes();
                }
            }
            Ok(out)
        }
        (KIND_OCTAHEDRAL, Block::Integers(d)) => {
            let bits = u32::from(r.u8()?);
            if !(2..=30).contains(&bits) {
                return Err(invalid("draco: normal quantisation bits out of range"));
            }
            let m = (1u32 << bits) - 2;
            let scale = 2.0f32 / m as f32;
            let mut out = vec![0u8; d.len() / 2 * 12];
            for (dst, st) in out
                .as_chunks_mut::<12>()
                .0
                .iter_mut()
                .zip(d.as_chunks::<2>().0)
            {
                let unit = octahedral_to_unit(st[0], st[1], scale);
                for (bytes, v) in dst.as_chunks_mut::<4>().0.iter_mut().zip(unit) {
                    *bytes = v.to_le_bytes();
                }
            }
            Ok(out)
        }
        _ => Err(invalid("draco: attribute block does not match its kind")),
    }
}

/// §8: octahedral coordinates to a unit vector, in binary32 arithmetic.
fn octahedral_to_unit(s: i32, t: i32, scale: f32) -> [f32; 3] {
    let mut y = s as f32 * scale - 1.0;
    let mut z = t as f32 * scale - 1.0;
    let x = 1.0 - y.abs() - z.abs();
    let k = (-x).max(0.0);
    y += if y < 0.0 { k } else { -k };
    z += if z < 0.0 { k } else { -k };
    let n2 = x * x + y * y + z * z;
    if f64::from(n2) < 1e-6 {
        return [0.0; 3];
    }
    let d = 1.0 / n2.sqrt();
    [x * d, y * d, z * d]
}

/// Read the whole attribute section.
pub(crate) fn read_attributes(
    r: &mut Reader<'_>,
    topology: &Topology<'_>,
    limits: &Limits,
    scratch: &mut EntropyScratch,
    trace: &mut Option<Vec<String>>,
) -> Result<Vec<DracoAttribute>> {
    let num_groups = usize::from(r.u8()?);
    let conn = match topology {
        Topology::Sequential { .. } => None,
        Topology::Edgebreaker(conn) => Some(*conn),
    };
    let num_points = match topology {
        Topology::Sequential { num_points } => *num_points,
        Topology::Edgebreaker(conn) => conn.num_points,
    };

    // §1 traversal descriptors (method 1 only).
    let mut traversals = Vec::with_capacity(num_groups);
    if let Some(conn) = conn {
        let mut seam_used = vec![false; conn.seams.len()];
        let mut mesh_used = false;
        for _ in 0..num_groups {
            let t = Traversal {
                seam_set: r.i8()?,
                element_kind: r.u8()?,
                traversal_order: r.u8()?,
            };
            let slot = match t.seam_set {
                -1 => &mut mesh_used,
                k if k >= 0 && (k as usize) < seam_used.len() => &mut seam_used[k as usize],
                _ => return Err(invalid("draco: attribute group names a missing seam set")),
            };
            if std::mem::replace(slot, true) {
                return Err(invalid("draco: seam set used by two attribute groups"));
            }
            if t.element_kind > 1
                || t.traversal_order > 1
                || (t.element_kind == 1 && (t.seam_set < 0 || t.traversal_order != 0))
            {
                return Err(invalid("draco: bad attribute group traversal descriptor"));
            }
            traversals.push(t);
        }
    }

    // §2 group headers.
    let mut groups = Vec::with_capacity(num_groups);
    for _ in 0..num_groups {
        groups.push(read_group_header(r)?);
    }
    // First position attribute in stream order (ch. 07 §5).
    let first_position = groups.iter().enumerate().find_map(|(g, descs)| {
        descs
            .iter()
            .position(|d| d.semantic == Semantic::Position)
            .map(|i| (g, i))
    });

    let mut section = Section {
        scratch,
        symbols: Vec::new(),
        trace,
        budget: u64::from(limits.max_points).saturating_mul(16),
        edgebreaker: conn.is_some(),
    };
    let mut out = Vec::new();
    // Integers and point map of the first position attribute once its group
    // is complete, for predictors of later groups.
    let mut saved_positions: Option<(Vec<i32>, Vec<u32>)> = None;

    // §4 group data.
    for (g, descs) in groups.iter().enumerate() {
        // Entry order and point-to-value map of the group.
        let seq: Option<(View<'_>, Sequence, Vec<u32>)>;
        let point_to_value: Vec<u32>;
        let entries: usize;
        match conn {
            None => {
                seq = None;
                entries = num_points as usize;
                point_to_value = (0..num_points).collect();
            }
            Some(conn) => {
                let t = traversals[g];
                let view = if t.element_kind == 1 {
                    let seam = &conn.seams[t.seam_set as usize];
                    View {
                        vertex: &seam.vertex,
                        opp: &seam.opp,
                        first_corner: &seam.first_corner,
                    }
                } else {
                    View {
                        vertex: &conn.vertex,
                        opp: &conn.opp,
                        first_corner: &conn.first_corner,
                    }
                };
                let s = sequence(view, conn.points(), t.traversal_order, section.trace)?;
                entries = s.len();
                // Ch. 07 §4, and the entry of every corner for the predictors.
                let mut map = vec![NIL; num_points as usize];
                let mut corner_entry = Vec::with_capacity(view.vertex.len());
                for (&p, &v) in conn.points().iter().zip(view.vertex) {
                    let e = s.vertex_entry[v as usize];
                    map[p as usize] = e;
                    corner_entry.push(e);
                }
                if map.contains(&NIL) {
                    return Err(invalid("draco: point without an attribute value"));
                }
                point_to_value = map;
                seq = Some((view, s, corner_entry));
            }
        }
        let mesh = seq.as_ref().map(|(view, seq, corner_entry)| MeshRefs {
            view: *view,
            seq,
            corner_entry,
        });
        let position_table = OnceCell::new();

        // Value blocks of every attribute, in order.
        let mut blocks: Vec<Block<'_>> = Vec::with_capacity(descs.len());
        for (i, desc) in descs.iter().enumerate() {
            if desc.kind == KIND_VERBATIM {
                let size = entries
                    .checked_mul(usize::from(desc.num_components) * desc.data_type.size())
                    .ok_or_else(|| invalid("draco: verbatim attribute too large"))?;
                blocks.push(Block::Verbatim(r.take(size)?));
                continue;
            }
            let positions = match first_position {
                Some((pg, pi)) if (pg, pi) < (g, i) => {
                    if pg == g {
                        match &blocks[pi] {
                            Block::Integers(d) if descs[pi].stored_components() == 3 => {
                                Some(Positions {
                                    values: d,
                                    point_to_value: &point_to_value,
                                    table: &position_table,
                                })
                            }
                            _ => None,
                        }
                    } else {
                        saved_positions.as_ref().map(|(d, map)| Positions {
                            values: d,
                            point_to_value: map,
                            table: &position_table,
                        })
                    }
                }
                _ => None,
            };
            let d = section.read_integer_block(r, desc, entries, mesh.as_ref(), positions)?;
            blocks.push(Block::Integers(d));
        }

        // Transform parameters and output conversion, in order.
        for (i, (desc, block)) in descs.iter().zip(blocks).enumerate() {
            let values = finish_attribute(r, desc, &block)?;
            if first_position == Some((g, i))
                && desc.stored_components() == 3
                && let Block::Integers(d) = block
            {
                saved_positions = Some((d, point_to_value.clone()));
            }
            out.push(DracoAttribute {
                semantic: desc.semantic,
                data_type: desc.data_type,
                num_components: desc.num_components,
                normalized: desc.normalized,
                unique_id: desc.unique_id,
                values,
                point_to_value: point_to_value.clone(),
            });
        }
    }
    Ok(out)
}
