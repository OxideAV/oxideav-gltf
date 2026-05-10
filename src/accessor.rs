//! Typed read paths from an accessor + bufferView + buffer triple.
//!
//! Decoder consumes `accessors[i]`, looks up `bufferViews[acc.bufferView]`,
//! looks up `buffers[bv.buffer]` (which by now has been resolved to a
//! contiguous `&[u8]` payload — either the `.glb` BIN chunk or a base64
//! data: URI we decoded), then walks `acc.count` elements at
//! `bv.byte_offset + acc.byte_offset` with stride `bv.byte_stride.unwrap_or(elementSize)`.

use crate::error::{invalid, Result};
use crate::json_model::{
    component_size, type_components, Accessor, BufferView, COMPONENT_TYPE_UNSIGNED_BYTE,
    COMPONENT_TYPE_UNSIGNED_INT, COMPONENT_TYPE_UNSIGNED_SHORT,
};
use std::sync::Arc;

/// Total byte length of one element of `accessor`.
pub fn element_size(accessor: &Accessor) -> Result<u32> {
    let n = type_components(&accessor.kind)
        .ok_or_else(|| invalid(format!("accessor: unknown type {:?}", accessor.kind)))?;
    let s = component_size(accessor.component_type).ok_or_else(|| {
        invalid(format!(
            "accessor: unknown componentType {}",
            accessor.component_type
        ))
    })?;
    Ok(n * s)
}

/// Resolve an accessor element span — `(start_offset, stride, count, element_size)`.
pub fn locate<'a>(
    accessor: &Accessor,
    buffer_views: &[BufferView],
    buffer: &'a [u8],
) -> Result<AccessorView<'a>> {
    let bv_index = accessor
        .buffer_view
        .ok_or_else(|| invalid("accessor: missing bufferView (sparse accessors not supported)"))?;
    let bv = buffer_views
        .get(bv_index as usize)
        .ok_or_else(|| invalid(format!("accessor: bufferView {bv_index} out of range")))?;
    let element_size = element_size(accessor)?;
    let stride = bv.byte_stride.unwrap_or(element_size);
    if stride < element_size {
        return Err(invalid(format!(
            "accessor: stride {stride} smaller than element size {element_size}"
        )));
    }
    let start = bv.byte_offset.unwrap_or(0) as usize + accessor.byte_offset.unwrap_or(0) as usize;
    let total_bytes = if accessor.count == 0 {
        0
    } else {
        // Last element starts at start + (count-1)*stride and is element_size long.
        ((accessor.count - 1) as usize) * (stride as usize) + (element_size as usize)
    };
    let end = start + total_bytes;
    if end > buffer.len() {
        return Err(invalid(format!(
            "accessor: span [{start}..{end}) overruns buffer of {} bytes",
            buffer.len()
        )));
    }
    Ok(AccessorView {
        bytes: &buffer[start..end],
        stride: stride as usize,
        element_size: element_size as usize,
        count: accessor.count as usize,
    })
}

/// A located span of accessor data plus the per-element layout.
#[derive(Debug)]
pub struct AccessorView<'a> {
    pub bytes: &'a [u8],
    pub stride: usize,
    pub element_size: usize,
    pub count: usize,
}

impl<'a> AccessorView<'a> {
    /// Iterate over element slices (length `element_size` each).
    pub fn elements(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        let bytes = self.bytes;
        (0..self.count).map(move |i| {
            let start = i * self.stride;
            &bytes[start..start + self.element_size]
        })
    }
}

/// Read `count` `[f32; N]` elements out of an accessor view of FLOAT
/// VECN. The caller has already validated the type/component combo.
pub fn read_vec_f32<const N: usize>(view: &AccessorView<'_>) -> Result<Vec<[f32; N]>> {
    if view.element_size != N * 4 {
        return Err(invalid(format!(
            "accessor: VEC{N} f32 element size {} != {}",
            view.element_size,
            N * 4
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    for elem in view.elements() {
        let mut a = [0f32; N];
        for i in 0..N {
            a[i] = f32::from_le_bytes(elem[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        out.push(a);
    }
    Ok(out)
}

/// Read indices in any of the spec's allowed widths
/// (UNSIGNED_BYTE / UNSIGNED_SHORT / UNSIGNED_INT) as `Vec<u32>`.
pub fn read_indices_u32(accessor: &Accessor, view: &AccessorView<'_>) -> Result<Vec<u32>> {
    if accessor.kind != "SCALAR" {
        return Err(invalid(format!(
            "indices accessor: type must be SCALAR, got {:?}",
            accessor.kind
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    match accessor.component_type {
        COMPONENT_TYPE_UNSIGNED_BYTE => {
            for elem in view.elements() {
                out.push(elem[0] as u32);
            }
        }
        COMPONENT_TYPE_UNSIGNED_SHORT => {
            for elem in view.elements() {
                out.push(u16::from_le_bytes(elem.try_into().unwrap()) as u32);
            }
        }
        COMPONENT_TYPE_UNSIGNED_INT => {
            for elem in view.elements() {
                out.push(u32::from_le_bytes(elem.try_into().unwrap()));
            }
        }
        other => {
            return Err(invalid(format!(
                "indices accessor: componentType {other} not allowed (must be 5121/5123/5125)"
            )));
        }
    }
    Ok(out)
}

/// Read a SCALAR FLOAT joints-weights / generic float accessor.
pub fn read_scalar_f32(view: &AccessorView<'_>) -> Result<Vec<f32>> {
    if view.element_size != 4 {
        return Err(invalid(format!(
            "scalar f32 accessor: element size {} != 4",
            view.element_size
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    for elem in view.elements() {
        out.push(f32::from_le_bytes(elem.try_into().unwrap()));
    }
    Ok(out)
}

/// Read VEC4 UNSIGNED_SHORT (joint indices).
pub fn read_vec4_u16(view: &AccessorView<'_>) -> Result<Vec<[u16; 4]>> {
    if view.element_size != 8 {
        return Err(invalid(format!(
            "vec4 u16 accessor: element size {} != 8",
            view.element_size
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    for elem in view.elements() {
        let mut a = [0u16; 4];
        for i in 0..4 {
            a[i] = u16::from_le_bytes(elem[i * 2..(i + 1) * 2].try_into().unwrap());
        }
        out.push(a);
    }
    Ok(out)
}

/// Promote a `componentType` constant to the smallest accessor
/// componentType that can hold every value in `indices`.
pub fn smallest_index_component(indices: &[u32]) -> u32 {
    let max = indices.iter().copied().max().unwrap_or(0);
    if max <= u8::MAX as u32 {
        COMPONENT_TYPE_UNSIGNED_BYTE
    } else if max <= u16::MAX as u32 {
        COMPONENT_TYPE_UNSIGNED_SHORT
    } else {
        COMPONENT_TYPE_UNSIGNED_INT
    }
}

/// Component size in bytes for index encoding (mirrors `smallest_index_component`).
pub fn index_component_size(component_type: u32) -> usize {
    component_size(component_type).unwrap_or(4) as usize
}

/// Pad the running buffer to a 4-byte boundary so subsequent
/// accessors stay aligned.
pub fn pad_to_4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

/// Helper: emit `[f32; N]` slice as little-endian bytes.
pub fn write_vec_f32<const N: usize>(out: &mut Vec<u8>, values: &[[f32; N]]) {
    for v in values {
        for c in v {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
}

/// Helper: emit indices in their chosen componentType width.
pub fn write_indices(out: &mut Vec<u8>, indices: &[u32], component_type: u32) {
    match component_type {
        COMPONENT_TYPE_UNSIGNED_BYTE => {
            for &i in indices {
                out.push(i as u8);
            }
        }
        COMPONENT_TYPE_UNSIGNED_SHORT => {
            for &i in indices {
                out.extend_from_slice(&(i as u16).to_le_bytes());
            }
        }
        _ => {
            for &i in indices {
                out.extend_from_slice(&i.to_le_bytes());
            }
        }
    }
}

/// Helper: emit `[u16; 4]` joint indices as little-endian bytes.
pub fn write_vec4_u16(out: &mut Vec<u8>, values: &[[u16; 4]]) {
    for v in values {
        for c in v {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
}

/// Read MAT4 FLOAT accessor (inverse bind matrices). glTF stores
/// matrices column-major per spec §3.6.2.4; we surface them as
/// `[[f32; 4]; 4]` row-major (each inner array is one row), to match
/// `oxideav_mesh3d::Skeleton::inverse_bind_matrices`.
pub fn read_mat4_f32(view: &AccessorView<'_>) -> Result<Vec<[[f32; 4]; 4]>> {
    if view.element_size != 64 {
        return Err(invalid(format!(
            "MAT4 f32 accessor: element size {} != 64",
            view.element_size
        )));
    }
    let mut out = Vec::with_capacity(view.count);
    for elem in view.elements() {
        let mut col_major = [0f32; 16];
        for i in 0..16 {
            col_major[i] = f32::from_le_bytes(elem[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        // col_major[c*4 + r] → row_major[r][c]
        let mut m = [[0f32; 4]; 4];
        for c in 0..4 {
            for r in 0..4 {
                m[r][c] = col_major[c * 4 + r];
            }
        }
        out.push(m);
    }
    Ok(out)
}

/// Helper: emit MAT4 column-major little-endian bytes from a row-major
/// `[[f32; 4]; 4]` source (the inverse of [`read_mat4_f32`]).
pub fn write_mat4_f32(out: &mut Vec<u8>, values: &[[[f32; 4]; 4]]) {
    for m in values {
        for c in 0..4 {
            // For each column, emit row 0..3 of that column (col-major).
            for row in m.iter().take(4) {
                out.extend_from_slice(&row[c].to_le_bytes());
            }
        }
    }
}

/// Materialise an accessor's element bytes into a tightly-packed
/// `Vec<u8>` of length `count * element_size`, applying sparse
/// substitution when present.
///
/// Returned layout: every element is contiguous (stride == element_size),
/// in accessor order. This decouples readers from byte_stride / sparse
/// concerns at the cost of one allocation per accessor.
///
/// `buffers` is indexed by `bufferView.buffer`. `byte_stride` from the
/// base buffer view is honoured during the copy step.
pub fn materialise_accessor(
    accessor: &Accessor,
    buffer_views: &[BufferView],
    buffers: &[Arc<Vec<u8>>],
) -> Result<Vec<u8>> {
    let element_size = element_size(accessor)? as usize;
    let count = accessor.count as usize;
    let mut out = vec![0u8; count * element_size];

    // Step 1: fill from base bufferView if present (otherwise leave zeros).
    if let Some(bv_idx) = accessor.buffer_view {
        let bv = buffer_views
            .get(bv_idx as usize)
            .ok_or_else(|| invalid(format!("accessor: bufferView {bv_idx} out of range")))?;
        let buf = buffers
            .get(bv.buffer as usize)
            .ok_or_else(|| invalid(format!("accessor: buffer {} out of range", bv.buffer)))?;
        let stride = bv.byte_stride.unwrap_or(element_size as u32) as usize;
        if stride < element_size {
            return Err(invalid(format!(
                "accessor: stride {stride} smaller than element size {element_size}"
            )));
        }
        let start =
            bv.byte_offset.unwrap_or(0) as usize + accessor.byte_offset.unwrap_or(0) as usize;
        let end_required = if count == 0 {
            start
        } else {
            start + (count - 1) * stride + element_size
        };
        if end_required > buf.len() {
            return Err(invalid(format!(
                "accessor: span overrun: needs {end_required} bytes, buffer has {}",
                buf.len()
            )));
        }
        for i in 0..count {
            let src = start + i * stride;
            let dst = i * element_size;
            out[dst..dst + element_size].copy_from_slice(&buf[src..src + element_size]);
        }
    }

    // Step 2: apply sparse overrides if present.
    if let Some(sparse) = &accessor.sparse {
        let scount = sparse.count as usize;
        // Indices.
        let idx_bv = buffer_views
            .get(sparse.indices.buffer_view as usize)
            .ok_or_else(|| invalid("sparse.indices: bufferView out of range"))?;
        let idx_buf = buffers
            .get(idx_bv.buffer as usize)
            .ok_or_else(|| invalid("sparse.indices: buffer out of range"))?;
        let idx_offset = idx_bv.byte_offset.unwrap_or(0) as usize
            + sparse.indices.byte_offset.unwrap_or(0) as usize;
        let indices =
            read_sparse_indices(sparse.indices.component_type, idx_buf, idx_offset, scount)?;
        for &i in &indices {
            if (i as usize) >= count {
                return Err(invalid(format!(
                    "sparse.indices: {i} >= base accessor count {count}"
                )));
            }
        }
        // Values.
        let val_bv = buffer_views
            .get(sparse.values.buffer_view as usize)
            .ok_or_else(|| invalid("sparse.values: bufferView out of range"))?;
        let val_buf = buffers
            .get(val_bv.buffer as usize)
            .ok_or_else(|| invalid("sparse.values: buffer out of range"))?;
        let val_offset = val_bv.byte_offset.unwrap_or(0) as usize
            + sparse.values.byte_offset.unwrap_or(0) as usize;
        let val_end = val_offset + scount * element_size;
        if val_end > val_buf.len() {
            return Err(invalid(format!(
                "sparse.values: span [{val_offset}..{val_end}) overruns buffer of {} bytes",
                val_buf.len()
            )));
        }
        for (slot, &target_idx) in indices.iter().enumerate() {
            let src = val_offset + slot * element_size;
            let dst = (target_idx as usize) * element_size;
            out[dst..dst + element_size].copy_from_slice(&val_buf[src..src + element_size]);
        }
    } else if accessor.buffer_view.is_none() {
        // Pure-zero accessor (no bufferView, no sparse): legal per spec.
        // out is already zeroed.
    }

    Ok(out)
}

/// Construct a contiguous-stride `AccessorView` over the materialised
/// bytes returned by [`materialise_accessor`]. Caller owns `bytes` and
/// must keep it alive for the lifetime of the view.
pub fn view_from_materialised<'a>(
    accessor: &Accessor,
    bytes: &'a [u8],
) -> Result<AccessorView<'a>> {
    let element_size = element_size(accessor)? as usize;
    let count = accessor.count as usize;
    if bytes.len() < count * element_size {
        return Err(invalid(format!(
            "materialised buffer too short: {} < {}",
            bytes.len(),
            count * element_size
        )));
    }
    Ok(AccessorView {
        bytes,
        stride: element_size,
        element_size,
        count,
    })
}

/// Read sparse-index `count` entries from a buffer view at the given
/// offset, with the spec-allowed component types
/// (UNSIGNED_BYTE / UNSIGNED_SHORT / UNSIGNED_INT). Indices MUST form
/// a strictly increasing sequence per spec §3.6.2.3.
pub fn read_sparse_indices(
    component_type: u32,
    bytes: &[u8],
    byte_offset: usize,
    count: usize,
) -> Result<Vec<u32>> {
    let csize = match component_type {
        COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        COMPONENT_TYPE_UNSIGNED_INT => 4,
        other => {
            return Err(invalid(format!(
                "sparse indices componentType {other} not allowed (must be 5121/5123/5125)"
            )));
        }
    };
    let end = byte_offset + count * csize;
    if end > bytes.len() {
        return Err(invalid(format!(
            "sparse indices: span [{byte_offset}..{end}) overruns buffer of {} bytes",
            bytes.len()
        )));
    }
    let mut out = Vec::with_capacity(count);
    let mut last: i64 = -1;
    for i in 0..count {
        let off = byte_offset + i * csize;
        let v = match component_type {
            COMPONENT_TYPE_UNSIGNED_BYTE => bytes[off] as u32,
            COMPONENT_TYPE_UNSIGNED_SHORT => {
                u16::from_le_bytes([bytes[off], bytes[off + 1]]) as u32
            }
            COMPONENT_TYPE_UNSIGNED_INT => {
                u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
            }
            _ => unreachable!(),
        };
        if (v as i64) <= last {
            return Err(invalid(format!(
                "sparse indices: not strictly increasing at slot {i} (saw {v} after {last})"
            )));
        }
        last = v as i64;
        out.push(v);
    }
    Ok(out)
}
