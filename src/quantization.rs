//! `KHR_mesh_quantization` extension support — additional vertex
//! attribute component types per
//! `docs/3d/gltf/extensions/KHR_mesh_quantization.md`.
//!
//! The extension extends the spec §3.7.2.1 / §3.7.2.2 allowed
//! componentTypes for the mesh attributes named POSITION / NORMAL /
//! TANGENT / TEXCOORD_n with 8-bit and 16-bit signed/unsigned variants
//! (normalised and unnormalised, per attribute slot).
//!
//! Decoder side: when the extension is declared in `extensionsUsed`,
//! the `read_attr_*` helpers accept the additional component types and
//! dequantise per the spec equations:
//!
//! | componentType         | int-to-float                  |
//! |-----------------------|-------------------------------|
//! | 5120 BYTE             | `f = max(c / 127.0, -1.0)`    |
//! | 5121 UNSIGNED_BYTE    | `f = c / 255.0`               |
//! | 5122 SHORT            | `f = max(c / 32767.0, -1.0)`  |
//! | 5123 UNSIGNED_SHORT   | `f = c / 65535.0`             |
//!
//! Unnormalised integer types are cast directly to `f32` per the spec's
//! "unnormalized integer 2 corresponds to 2.0" rule (KHR_mesh_quantization
//! §Extending Mesh Attributes line "using unnormalized integers does not
//! change semantics of the stored values").
//!
//! The decoder stashes the original (componentType, normalized) pair of
//! each quantised attribute under the primitive's
//! `extras["__attr_quant"]` sentinel (one object per attribute name).
//! The float→int [`quantize_normalized`] / [`write_quantized_vec2`] etc.
//! helpers and the morph-attribute combo tables exist so a future
//! encoder pass can re-quantise and re-emit each attribute in its
//! original form, appending `KHR_mesh_quantization` to BOTH
//! `extensionsUsed` AND `extensionsRequired` per the extension's
//! §Overview MUST ("files that use the extension must specify it in
//! extensionsRequired array - the extension is not optional"). The
//! encoder integration itself is not yet wired; see the crate README.

use crate::accessor::AccessorView;
use crate::error::{invalid, Result};
use crate::json_model::{
    Accessor, COMPONENT_TYPE_BYTE, COMPONENT_TYPE_FLOAT, COMPONENT_TYPE_SHORT,
    COMPONENT_TYPE_UNSIGNED_BYTE, COMPONENT_TYPE_UNSIGNED_SHORT,
};

/// Extension identifier string used in `extensionsUsed` /
/// `extensionsRequired`.
pub const EXTENSION_NAME: &str = "KHR_mesh_quantization";

/// `extras` sentinel key under which the decoder stashes per-primitive
/// quantisation metadata so the encoder can round-trip the original
/// component types. The value is a JSON object keyed by attribute name
/// (`POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_n`) mapping to
/// `{ "componentType": u32, "normalized": bool }`.
pub const ATTR_QUANT_KEY: &str = "__attr_quant";

/// Spec-defined int → float conversion for a normalised integer
/// component. `c` carries the raw integer cast to `i32`.
pub fn dequantize_normalized(component_type: u32, c: i32) -> Result<f32> {
    match component_type {
        // f = max(c / 127.0, -1.0)
        COMPONENT_TYPE_BYTE => Ok(((c as f32) / 127.0).max(-1.0)),
        COMPONENT_TYPE_UNSIGNED_BYTE => Ok((c as f32) / 255.0),
        COMPONENT_TYPE_SHORT => Ok(((c as f32) / 32767.0).max(-1.0)),
        COMPONENT_TYPE_UNSIGNED_SHORT => Ok((c as f32) / 65535.0),
        other => Err(invalid(format!(
            "KHR_mesh_quantization: componentType {other} is not a quantisable integer type"
        ))),
    }
}

/// Spec-defined float → int conversion for a normalised integer
/// component. The output is the encoded integer as `i32` (caller casts
/// to the on-disk width).
pub fn quantize_normalized(component_type: u32, f: f32) -> Result<i32> {
    let r = match component_type {
        // c = round(f * 127.0)
        COMPONENT_TYPE_BYTE => (f.clamp(-1.0, 1.0) * 127.0).round() as i32,
        COMPONENT_TYPE_UNSIGNED_BYTE => (f.clamp(0.0, 1.0) * 255.0).round() as i32,
        COMPONENT_TYPE_SHORT => (f.clamp(-1.0, 1.0) * 32767.0).round() as i32,
        COMPONENT_TYPE_UNSIGNED_SHORT => (f.clamp(0.0, 1.0) * 65535.0).round() as i32,
        other => {
            return Err(invalid(format!(
                "KHR_mesh_quantization: componentType {other} is not a quantisable integer type"
            )))
        }
    };
    Ok(r)
}

/// True when `component_type` is one of the integer widths the
/// extension introduces for vertex / morph attribute storage.
pub fn is_quantizable_integer(component_type: u32) -> bool {
    matches!(
        component_type,
        COMPONENT_TYPE_BYTE
            | COMPONENT_TYPE_UNSIGNED_BYTE
            | COMPONENT_TYPE_SHORT
            | COMPONENT_TYPE_UNSIGNED_SHORT
    )
}

/// Read one signed/unsigned integer component out of `bytes` at offset
/// `off`, sign-extended to `i32`.
#[inline]
fn read_component(component_type: u32, bytes: &[u8], off: usize) -> i32 {
    match component_type {
        COMPONENT_TYPE_BYTE => bytes[off] as i8 as i32,
        COMPONENT_TYPE_UNSIGNED_BYTE => bytes[off] as i32,
        COMPONENT_TYPE_SHORT => i16::from_le_bytes([bytes[off], bytes[off + 1]]) as i32,
        COMPONENT_TYPE_UNSIGNED_SHORT => u16::from_le_bytes([bytes[off], bytes[off + 1]]) as i32,
        _ => unreachable!("read_component requires a quantisable integer type"),
    }
}

/// Read one f32 component out of `bytes` at offset `off`.
#[inline]
fn read_f32(bytes: &[u8], off: usize) -> f32 {
    f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

/// Dequantise a VEC2 attribute view per the extension. When the
/// accessor's componentType is FLOAT the data is returned unchanged.
/// `normalized` selects the spec int-to-float equation; an unnormalised
/// integer is cast directly to `f32`.
pub fn dequantize_vec2(acc: &Accessor, view: &AccessorView<'_>) -> Result<Vec<[f32; 2]>> {
    let ct = acc.component_type;
    let mut out = Vec::with_capacity(view.count);
    if ct == COMPONENT_TYPE_FLOAT {
        if view.element_size != 8 {
            return Err(invalid(format!(
                "KHR_mesh_quantization VEC2 FLOAT element size {} != 8",
                view.element_size
            )));
        }
        for elem in view.elements() {
            out.push([read_f32(elem, 0), read_f32(elem, 4)]);
        }
        return Ok(out);
    }
    if !is_quantizable_integer(ct) {
        return Err(invalid(format!(
            "KHR_mesh_quantization VEC2: componentType {ct} not allowed"
        )));
    }
    let csize = match ct {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        _ => unreachable!(),
    };
    if view.element_size < 2 * csize {
        return Err(invalid(format!(
            "KHR_mesh_quantization VEC2: element size {} < expected {}",
            view.element_size,
            2 * csize
        )));
    }
    for elem in view.elements() {
        let c0 = read_component(ct, elem, 0);
        let c1 = read_component(ct, elem, csize);
        if acc.normalized {
            out.push([
                dequantize_normalized(ct, c0)?,
                dequantize_normalized(ct, c1)?,
            ]);
        } else {
            out.push([c0 as f32, c1 as f32]);
        }
    }
    Ok(out)
}

/// Dequantise a VEC3 attribute view per the extension. Behaviour
/// mirrors [`dequantize_vec2`]: FLOAT passes through, integer types
/// dequantise via the spec equation when `normalized`, otherwise cast.
pub fn dequantize_vec3(acc: &Accessor, view: &AccessorView<'_>) -> Result<Vec<[f32; 3]>> {
    let ct = acc.component_type;
    let mut out = Vec::with_capacity(view.count);
    if ct == COMPONENT_TYPE_FLOAT {
        if view.element_size != 12 {
            return Err(invalid(format!(
                "KHR_mesh_quantization VEC3 FLOAT element size {} != 12",
                view.element_size
            )));
        }
        for elem in view.elements() {
            out.push([read_f32(elem, 0), read_f32(elem, 4), read_f32(elem, 8)]);
        }
        return Ok(out);
    }
    if !is_quantizable_integer(ct) {
        return Err(invalid(format!(
            "KHR_mesh_quantization VEC3: componentType {ct} not allowed"
        )));
    }
    let csize = match ct {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        _ => unreachable!(),
    };
    if view.element_size < 3 * csize {
        return Err(invalid(format!(
            "KHR_mesh_quantization VEC3: element size {} < expected {}",
            view.element_size,
            3 * csize
        )));
    }
    for elem in view.elements() {
        let c0 = read_component(ct, elem, 0);
        let c1 = read_component(ct, elem, csize);
        let c2 = read_component(ct, elem, 2 * csize);
        if acc.normalized {
            out.push([
                dequantize_normalized(ct, c0)?,
                dequantize_normalized(ct, c1)?,
                dequantize_normalized(ct, c2)?,
            ]);
        } else {
            out.push([c0 as f32, c1 as f32, c2 as f32]);
        }
    }
    Ok(out)
}

/// Dequantise a VEC4 attribute view per the extension. Behaviour
/// mirrors [`dequantize_vec2`].
pub fn dequantize_vec4(acc: &Accessor, view: &AccessorView<'_>) -> Result<Vec<[f32; 4]>> {
    let ct = acc.component_type;
    let mut out = Vec::with_capacity(view.count);
    if ct == COMPONENT_TYPE_FLOAT {
        if view.element_size != 16 {
            return Err(invalid(format!(
                "KHR_mesh_quantization VEC4 FLOAT element size {} != 16",
                view.element_size
            )));
        }
        for elem in view.elements() {
            out.push([
                read_f32(elem, 0),
                read_f32(elem, 4),
                read_f32(elem, 8),
                read_f32(elem, 12),
            ]);
        }
        return Ok(out);
    }
    if !is_quantizable_integer(ct) {
        return Err(invalid(format!(
            "KHR_mesh_quantization VEC4: componentType {ct} not allowed"
        )));
    }
    let csize = match ct {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        _ => unreachable!(),
    };
    if view.element_size < 4 * csize {
        return Err(invalid(format!(
            "KHR_mesh_quantization VEC4: element size {} < expected {}",
            view.element_size,
            4 * csize
        )));
    }
    for elem in view.elements() {
        let c0 = read_component(ct, elem, 0);
        let c1 = read_component(ct, elem, csize);
        let c2 = read_component(ct, elem, 2 * csize);
        let c3 = read_component(ct, elem, 3 * csize);
        if acc.normalized {
            out.push([
                dequantize_normalized(ct, c0)?,
                dequantize_normalized(ct, c1)?,
                dequantize_normalized(ct, c2)?,
                dequantize_normalized(ct, c3)?,
            ]);
        } else {
            out.push([c0 as f32, c1 as f32, c2 as f32, c3 as f32]);
        }
    }
    Ok(out)
}

/// Spec int-element byte size, padded to a 4-byte vertex-attribute
/// stride per `KHR_mesh_quantization` §Extending Mesh Attributes
/// (alignment rules: each element is aligned to a 4-byte boundary;
/// e.g. a `BYTE` normal expects a stride of 4, not 3).
pub fn quantized_element_stride(component_type: u32, component_count: usize) -> usize {
    let csize = match component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        COMPONENT_TYPE_FLOAT => 4,
        _ => 4,
    };
    let raw = csize * component_count;
    // Round up to a 4-byte boundary per §Extending Mesh Attributes.
    raw.div_ceil(4) * 4
}

/// Encode a single integer component into `out`, appending its
/// width-correct little-endian bytes.
#[inline]
fn write_component(out: &mut Vec<u8>, component_type: u32, c: i32) {
    match component_type {
        COMPONENT_TYPE_BYTE => out.push(c as i8 as u8),
        COMPONENT_TYPE_UNSIGNED_BYTE => out.push(c.clamp(0, 255) as u8),
        COMPONENT_TYPE_SHORT => out.extend_from_slice(&(c as i16).to_le_bytes()),
        COMPONENT_TYPE_UNSIGNED_SHORT => {
            out.extend_from_slice(&(c.clamp(0, 65535) as u16).to_le_bytes())
        }
        _ => {}
    }
}

/// Emit a quantised VEC2 array. The encoder writes the chosen
/// componentType + the 4-byte element padding the spec requires.
pub fn write_quantized_vec2(
    out: &mut Vec<u8>,
    data: &[[f32; 2]],
    component_type: u32,
    normalized: bool,
) -> Result<()> {
    let stride = quantized_element_stride(component_type, 2);
    let csize = match component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        COMPONENT_TYPE_FLOAT => 4,
        other => {
            return Err(invalid(format!(
                "KHR_mesh_quantization encode: VEC2 componentType {other} not allowed"
            )))
        }
    };
    let pad = stride - csize * 2;
    for v in data {
        for &f in v {
            if component_type == COMPONENT_TYPE_FLOAT {
                out.extend_from_slice(&f.to_le_bytes());
            } else if normalized {
                let c = quantize_normalized(component_type, f)?;
                write_component(out, component_type, c);
            } else {
                write_component(out, component_type, f.round() as i32);
            }
        }
        for _ in 0..pad {
            out.push(0);
        }
    }
    Ok(())
}

/// Emit a quantised VEC3 array. Same stride / padding rules as
/// [`write_quantized_vec2`].
pub fn write_quantized_vec3(
    out: &mut Vec<u8>,
    data: &[[f32; 3]],
    component_type: u32,
    normalized: bool,
) -> Result<()> {
    let stride = quantized_element_stride(component_type, 3);
    let csize = match component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        COMPONENT_TYPE_FLOAT => 4,
        other => {
            return Err(invalid(format!(
                "KHR_mesh_quantization encode: VEC3 componentType {other} not allowed"
            )))
        }
    };
    let pad = stride - csize * 3;
    for v in data {
        for &f in v {
            if component_type == COMPONENT_TYPE_FLOAT {
                out.extend_from_slice(&f.to_le_bytes());
            } else if normalized {
                let c = quantize_normalized(component_type, f)?;
                write_component(out, component_type, c);
            } else {
                write_component(out, component_type, f.round() as i32);
            }
        }
        for _ in 0..pad {
            out.push(0);
        }
    }
    Ok(())
}

/// Emit a quantised VEC4 array. Same stride / padding rules as
/// [`write_quantized_vec2`].
pub fn write_quantized_vec4(
    out: &mut Vec<u8>,
    data: &[[f32; 4]],
    component_type: u32,
    normalized: bool,
) -> Result<()> {
    let stride = quantized_element_stride(component_type, 4);
    let csize = match component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => 1,
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => 2,
        COMPONENT_TYPE_FLOAT => 4,
        other => {
            return Err(invalid(format!(
                "KHR_mesh_quantization encode: VEC4 componentType {other} not allowed"
            )))
        }
    };
    let pad = stride - csize * 4;
    for v in data {
        for &f in v {
            if component_type == COMPONENT_TYPE_FLOAT {
                out.extend_from_slice(&f.to_le_bytes());
            } else if normalized {
                let c = quantize_normalized(component_type, f)?;
                write_component(out, component_type, c);
            } else {
                write_component(out, component_type, f.round() as i32);
            }
        }
        for _ in 0..pad {
            out.push(0);
        }
    }
    Ok(())
}

/// Check whether a (kind, componentType, normalized) triple is in the
/// extension's spec-allowed set for the named base-mesh attribute.
/// Returns `true` for the standard glTF 2.0 §3.7.2.1 allowed types
/// AS WELL AS the additional types from the
/// `KHR_mesh_quantization.md` §Extending Mesh Attributes table.
///
/// Caller is expected to gate on `extensionsUsed` having
/// [`EXTENSION_NAME`] when checking the extension-only rows.
pub fn is_base_attr_combo_allowed(
    attr_name: &str,
    kind: &str,
    component_type: u32,
    normalized: bool,
) -> bool {
    let key = base_attr_key(attr_name);
    match (key, kind) {
        ("POSITION", "VEC3") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _)
                | (COMPONENT_TYPE_BYTE, _)
                | (COMPONENT_TYPE_UNSIGNED_BYTE, _)
                | (COMPONENT_TYPE_SHORT, _)
                | (COMPONENT_TYPE_UNSIGNED_SHORT, _)
        ),
        ("NORMAL", "VEC3") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _) | (COMPONENT_TYPE_BYTE, true) | (COMPONENT_TYPE_SHORT, true)
        ),
        ("TANGENT", "VEC4") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _) | (COMPONENT_TYPE_BYTE, true) | (COMPONENT_TYPE_SHORT, true)
        ),
        // TEXCOORD: extension allows extra signed/unnormalised variants on
        // top of the base spec table (FLOAT / UBYTE normalised /
        // USHORT normalised). Unsigned-normalised remain allowed
        // un-extension.
        ("TEXCOORD", "VEC2") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _)
                | (COMPONENT_TYPE_UNSIGNED_BYTE, _)
                | (COMPONENT_TYPE_UNSIGNED_SHORT, _)
                | (COMPONENT_TYPE_BYTE, _)
                | (COMPONENT_TYPE_SHORT, _)
        ),
        _ => false,
    }
}

/// Spec §3.7.2.2 morph-target allowed types — the extension's morph
/// section. POSITION / NORMAL / TANGENT morph targets use VEC3 (no W)
/// and TEXCOORD_n uses VEC2 per `KHR_mesh_quantization.md` §Extending
/// Morph Target Attributes.
pub fn is_morph_attr_combo_allowed(
    attr_name: &str,
    kind: &str,
    component_type: u32,
    normalized: bool,
) -> bool {
    let key = base_attr_key(attr_name);
    match (key, kind) {
        ("POSITION", "VEC3") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _) | (COMPONENT_TYPE_BYTE, _) | (COMPONENT_TYPE_SHORT, _)
        ),
        ("NORMAL", "VEC3") | ("TANGENT", "VEC3") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _) | (COMPONENT_TYPE_BYTE, true) | (COMPONENT_TYPE_SHORT, true)
        ),
        ("TEXCOORD", "VEC2") => matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _) | (COMPONENT_TYPE_BYTE, _) | (COMPONENT_TYPE_SHORT, _)
        ),
        _ => false,
    }
}

/// Normalise `POSITION` / `NORMAL` / `TANGENT` / `TEXCOORD_n` /
/// `COLOR_n` / `JOINTS_n` / `WEIGHTS_n` down to the un-indexed key
/// `TEXCOORD`, `COLOR`, etc. so the table lookups don't have to
/// enumerate every set index.
fn base_attr_key(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix("TEXCOORD_") {
        // "TEXCOORD_0", "TEXCOORD_1", ... all match the TEXCOORD row.
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return "TEXCOORD";
        }
    }
    if let Some(rest) = name.strip_prefix("COLOR_") {
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return "COLOR";
        }
    }
    name
}

/// True if the accessor's (componentType, normalized) pair is NOT one
/// of the standard glTF 2.0 §3.7.2.1 types for the given attribute —
/// i.e. usage of the accessor implies the `KHR_mesh_quantization`
/// extension is in play (`extensionsUsed` MUST contain it).
///
/// Standard spec §3.7.2.1 allowed types per attribute (without the
/// extension):
/// * POSITION VEC3 — FLOAT
/// * NORMAL VEC3 — FLOAT
/// * TANGENT VEC4 — FLOAT
/// * TEXCOORD_n VEC2 — FLOAT, UNSIGNED_BYTE normalised, UNSIGNED_SHORT
///   normalised
pub fn requires_extension_for_base_attr(
    attr_name: &str,
    kind: &str,
    component_type: u32,
    normalized: bool,
) -> bool {
    let key = base_attr_key(attr_name);
    match (key, kind) {
        ("POSITION", "VEC3") | ("NORMAL", "VEC3") => component_type != COMPONENT_TYPE_FLOAT,
        ("TANGENT", "VEC4") => component_type != COMPONENT_TYPE_FLOAT,
        ("TEXCOORD", "VEC2") => !matches!(
            (component_type, normalized),
            (COMPONENT_TYPE_FLOAT, _)
                | (COMPONENT_TYPE_UNSIGNED_BYTE, true)
                | (COMPONENT_TYPE_UNSIGNED_SHORT, true)
        ),
        _ => false,
    }
}

/// True if the morph-target accessor's (componentType, normalized) pair
/// is NOT one of the standard glTF 2.0 §3.7.2.2 types for the named
/// morph attribute (POSITION / NORMAL / TANGENT all FLOAT VEC3; the
/// standard spec does not enumerate morph TEXCOORD_n at all so any use
/// of a TEXCOORD morph target implies the extension).
pub fn requires_extension_for_morph_attr(
    attr_name: &str,
    kind: &str,
    component_type: u32,
    _normalized: bool,
) -> bool {
    let key = base_attr_key(attr_name);
    match (key, kind) {
        ("POSITION", "VEC3") | ("NORMAL", "VEC3") | ("TANGENT", "VEC3") => {
            component_type != COMPONENT_TYPE_FLOAT
        }
        ("TEXCOORD", "VEC2") => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dequantize_normalized_byte() {
        // f = max(c / 127.0, -1.0)
        assert!((dequantize_normalized(COMPONENT_TYPE_BYTE, 127).unwrap() - 1.0).abs() < 1e-6);
        assert!((dequantize_normalized(COMPONENT_TYPE_BYTE, -127).unwrap() + 1.0).abs() < 1e-6);
        // -128 clamps to -1.0 (the c / 127 division would land at
        // -128/127 = -1.0078, the max(.., -1.0) clause clamps).
        assert!((dequantize_normalized(COMPONENT_TYPE_BYTE, -128).unwrap() + 1.0).abs() < 1e-6);
        assert!((dequantize_normalized(COMPONENT_TYPE_BYTE, 0).unwrap()).abs() < 1e-6);
    }

    #[test]
    fn dequantize_normalized_ubyte() {
        // f = c / 255.0
        assert!(
            (dequantize_normalized(COMPONENT_TYPE_UNSIGNED_BYTE, 255).unwrap() - 1.0).abs() < 1e-6
        );
        assert!((dequantize_normalized(COMPONENT_TYPE_UNSIGNED_BYTE, 0).unwrap()).abs() < 1e-6);
    }

    #[test]
    fn dequantize_normalized_short() {
        // f = max(c / 32767.0, -1.0)
        assert!((dequantize_normalized(COMPONENT_TYPE_SHORT, 32767).unwrap() - 1.0).abs() < 1e-6);
        assert!((dequantize_normalized(COMPONENT_TYPE_SHORT, -32767).unwrap() + 1.0).abs() < 1e-6);
        // -32768 clamps to -1.0
        assert!((dequantize_normalized(COMPONENT_TYPE_SHORT, -32768).unwrap() + 1.0).abs() < 1e-6);
    }

    #[test]
    fn dequantize_normalized_ushort() {
        assert!(
            (dequantize_normalized(COMPONENT_TYPE_UNSIGNED_SHORT, 65535).unwrap() - 1.0).abs()
                < 1e-6
        );
        assert!((dequantize_normalized(COMPONENT_TYPE_UNSIGNED_SHORT, 0).unwrap()).abs() < 1e-6);
    }

    #[test]
    fn roundtrip_normalized_byte() {
        // f -> c -> f' should land within 1/127 of the original.
        for f in [-1.0, -0.5, 0.0, 0.25, 0.99, 1.0_f32] {
            let c = quantize_normalized(COMPONENT_TYPE_BYTE, f).unwrap();
            let f2 = dequantize_normalized(COMPONENT_TYPE_BYTE, c).unwrap();
            assert!((f - f2).abs() < 1.0 / 127.0 + 1e-7, "f={f} c={c} f2={f2}");
        }
    }

    #[test]
    fn roundtrip_normalized_short() {
        for f in [-1.0, -0.123, 0.0, 0.5, 0.9999_f32] {
            let c = quantize_normalized(COMPONENT_TYPE_SHORT, f).unwrap();
            let f2 = dequantize_normalized(COMPONENT_TYPE_SHORT, c).unwrap();
            assert!((f - f2).abs() < 1.0 / 32767.0 + 1e-7);
        }
    }

    #[test]
    fn quantized_element_stride_matches_spec() {
        // BYTE VEC3 → 3 bytes raw, padded to 4 per §Extending Mesh Attributes.
        assert_eq!(quantized_element_stride(COMPONENT_TYPE_BYTE, 3), 4);
        // UBYTE VEC2 → 2 bytes raw, padded to 4.
        assert_eq!(quantized_element_stride(COMPONENT_TYPE_UNSIGNED_BYTE, 2), 4);
        // SHORT VEC3 → 6 bytes raw, padded to 8.
        assert_eq!(quantized_element_stride(COMPONENT_TYPE_SHORT, 3), 8);
        // SHORT VEC4 → 8 bytes, already aligned.
        assert_eq!(quantized_element_stride(COMPONENT_TYPE_SHORT, 4), 8);
        // BYTE VEC4 → 4 bytes, already aligned.
        assert_eq!(quantized_element_stride(COMPONENT_TYPE_BYTE, 4), 4);
    }

    #[test]
    fn base_attr_allowed_combinations() {
        // Standard FLOAT always allowed.
        assert!(is_base_attr_combo_allowed(
            "POSITION",
            "VEC3",
            COMPONENT_TYPE_FLOAT,
            false
        ));
        // Extension-only: BYTE NORMALIZED POSITION VEC3.
        assert!(is_base_attr_combo_allowed(
            "POSITION",
            "VEC3",
            COMPONENT_TYPE_BYTE,
            true
        ));
        // NORMAL requires normalized for the integer rows.
        assert!(is_base_attr_combo_allowed(
            "NORMAL",
            "VEC3",
            COMPONENT_TYPE_SHORT,
            true
        ));
        assert!(!is_base_attr_combo_allowed(
            "NORMAL",
            "VEC3",
            COMPONENT_TYPE_SHORT,
            false
        ));
        // TEXCOORD_5 routes the same as TEXCOORD_0.
        assert!(is_base_attr_combo_allowed(
            "TEXCOORD_5",
            "VEC2",
            COMPONENT_TYPE_SHORT,
            false
        ));
    }

    #[test]
    fn requires_ext_flag() {
        // POSITION FLOAT is the spec default — no extension required.
        assert!(!requires_extension_for_base_attr(
            "POSITION",
            "VEC3",
            COMPONENT_TYPE_FLOAT,
            false
        ));
        // POSITION BYTE NORMALIZED — extension required.
        assert!(requires_extension_for_base_attr(
            "POSITION",
            "VEC3",
            COMPONENT_TYPE_BYTE,
            true
        ));
        // TEXCOORD UBYTE NORMALIZED — standard spec, no extension.
        assert!(!requires_extension_for_base_attr(
            "TEXCOORD_0",
            "VEC2",
            COMPONENT_TYPE_UNSIGNED_BYTE,
            true
        ));
        // TEXCOORD BYTE UNNORMALIZED — extension required.
        assert!(requires_extension_for_base_attr(
            "TEXCOORD_0",
            "VEC2",
            COMPONENT_TYPE_BYTE,
            false
        ));
    }
}
