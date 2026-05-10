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
