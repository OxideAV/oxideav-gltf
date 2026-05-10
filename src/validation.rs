//! Vertex-attribute compression validation per glTF 2.0 §3.6.2.4 +
//! §3.7.2.1 (semantic constraints on attribute accessor data).
//!
//! These checks are run by the decoder against `accessors[]` /
//! `bufferViews[]` / primitive `attributes` after the buffers have been
//! resolved and BEFORE the per-attribute `read_attr_*` paths. They
//! surface MUST-level spec violations the earlier rounds didn't catch.
//!
//! Validations performed:
//!
//! * §3.6.2.4 — `accessor.byteOffset` MUST be a multiple of the
//!   accessor's component-type size; for vertex-attribute accessors
//!   `accessor.byteOffset` MUST also be a multiple of 4 and the
//!   underlying `bufferView.byteStride` (when present) MUST be a
//!   multiple of 4.
//! * §3.6.2.4 — `accessor.byteOffset + bufferView.byteOffset` MUST be a
//!   multiple of the component-type size (alignment of the start of the
//!   accessor data inside the underlying buffer).
//! * §3.7.2.1 — all attribute accessors of a single primitive MUST have
//!   the same `count`.
//! * §3.7.2.1 — indices accessor MUST NOT contain the maximum value for
//!   its component type (255 / 65535 / 4294967295) — those values are
//!   reserved for primitive-restart in some graphics APIs.
//! * §3.7.2.1 — TANGENT.w (handedness) MUST be exactly +1.0 or -1.0.
//! * §3.7.2.1 — every component of every COLOR_0 element MUST be in
//!   `[0.0, 1.0]`.
//!
//! All failures surface as `Error::InvalidData` with a stable
//! `VertexAttribute…` prefix so callers can grep for the specific
//! sub-rule without reaching for a typed enum (the shared
//! `oxideav_core::Error` enum can't gain a new variant from a sibling
//! crate).

use crate::error::{invalid, Result};
use crate::json_model::{
    component_size, Accessor, BufferView, COMPONENT_TYPE_UNSIGNED_BYTE,
    COMPONENT_TYPE_UNSIGNED_INT, COMPONENT_TYPE_UNSIGNED_SHORT,
};
use std::collections::HashMap;

/// Validate `§3.6.2.4` alignment for an accessor that backs a vertex
/// attribute.
///
/// `is_vertex_attribute` enables the stricter 4-byte alignment rule
/// (line 3111 of the spec). Index accessors and animation-keyframe
/// accessors get only the basic component-size alignment check.
pub fn validate_alignment(
    accessor: &Accessor,
    buffer_views: &[BufferView],
    is_vertex_attribute: bool,
    label: &str,
) -> Result<()> {
    let csize = component_size(accessor.component_type).ok_or_else(|| {
        invalid(format!(
            "VertexAttributeAlignment: {label}: unknown componentType {}",
            accessor.component_type
        ))
    })?;
    let acc_off = accessor.byte_offset.unwrap_or(0);
    if acc_off % csize != 0 {
        return Err(invalid(format!(
            "VertexAttributeAlignment: {label}: accessor.byteOffset {acc_off} not multiple of \
             component size {csize} (spec §3.6.2.4)"
        )));
    }
    if is_vertex_attribute && acc_off % 4 != 0 {
        return Err(invalid(format!(
            "VertexAttributeAlignment: {label}: accessor.byteOffset {acc_off} not multiple of 4 \
             for vertex attribute (spec §3.6.2.4)"
        )));
    }
    let Some(bv_idx) = accessor.buffer_view else {
        // Pure-zero or sparse-only accessor: nothing more to check.
        return Ok(());
    };
    let bv = buffer_views.get(bv_idx as usize).ok_or_else(|| {
        invalid(format!(
            "VertexAttributeAlignment: {label}: bufferView {bv_idx} out of range"
        ))
    })?;
    let bv_off = bv.byte_offset.unwrap_or(0);
    let combined = bv_off.checked_add(acc_off).ok_or_else(|| {
        invalid(format!(
            "VertexAttributeAlignment: {label}: offset overflow"
        ))
    })?;
    if combined % csize != 0 {
        return Err(invalid(format!(
            "VertexAttributeAlignment: {label}: bufferView.byteOffset + accessor.byteOffset \
             ({combined}) not multiple of component size {csize} (spec §3.6.2.4)"
        )));
    }
    if is_vertex_attribute {
        if let Some(stride) = bv.byte_stride {
            if stride % 4 != 0 {
                return Err(invalid(format!(
                    "VertexAttributeAlignment: {label}: bufferView.byteStride {stride} not \
                     multiple of 4 (spec §3.6.2.4)"
                )));
            }
        }
    }
    Ok(())
}

/// Validate that all attribute accessors of a primitive carry the same
/// `count` per spec §3.7.2.1.
///
/// `attributes` is the primitive's name → accessor-index map exactly as
/// it appears in the JSON (`POSITION`, `NORMAL`, `TANGENT`,
/// `TEXCOORD_n`, `COLOR_n`, `JOINTS_n`, `WEIGHTS_n`).
pub fn validate_attribute_counts(
    attributes: &HashMap<String, u32>,
    accessors: &[Accessor],
) -> Result<()> {
    // Walk in name order for deterministic error messages — HashMap
    // iteration order is otherwise nondeterministic across runs.
    let mut names: Vec<&String> = attributes.keys().collect();
    names.sort();
    let mut seen: Option<(String, u32)> = None;
    for name in names {
        let idx = attributes[name];
        let acc = accessors.get(idx as usize).ok_or_else(|| {
            invalid(format!(
                "VertexAttributeCount: {name}: accessor {idx} out of range"
            ))
        })?;
        match &seen {
            None => {
                seen = Some((name.clone(), acc.count));
            }
            Some((first_name, first_count)) => {
                if acc.count != *first_count {
                    return Err(invalid(format!(
                        "VertexAttributeCount: {name} count {} != {first_name} count {first_count} \
                         (spec §3.7.2.1: all attribute accessors of a primitive MUST share count)",
                        acc.count
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Spec §3.7.2.1: indices accessor MUST NOT contain the
/// primitive-restart sentinel for its component type (255 / 65535 /
/// 4294967295).
pub fn validate_index_no_restart(accessor: &Accessor, indices: &[u32]) -> Result<()> {
    let sentinel: u32 = match accessor.component_type {
        COMPONENT_TYPE_UNSIGNED_BYTE => 255,
        COMPONENT_TYPE_UNSIGNED_SHORT => 65535,
        COMPONENT_TYPE_UNSIGNED_INT => u32::MAX,
        // Other component types are already rejected upstream by
        // `read_indices_u32`; nothing to do here.
        _ => return Ok(()),
    };
    if let Some(pos) = indices.iter().position(|&i| i == sentinel) {
        return Err(invalid(format!(
            "VertexAttributeIndexRestart: index #{pos} = {sentinel} reserved for primitive-restart \
             (spec §3.7.2.1: indices accessor MUST NOT contain the max value for its componentType)"
        )));
    }
    Ok(())
}

/// Spec §3.7.2.1: each TANGENT element's W component (handedness) MUST
/// be exactly `+1.0` or `-1.0`. Tolerance allows for f32 round-trip
/// drift around the two valid values.
pub fn validate_tangent_w(tangents: &[[f32; 4]]) -> Result<()> {
    for (i, t) in tangents.iter().enumerate() {
        let w = t[3];
        if (w - 1.0).abs() > 1e-5 && (w + 1.0).abs() > 1e-5 {
            return Err(invalid(format!(
                "VertexAttributeTangentW: TANGENT[{i}].w = {w} (spec §3.7.2.1: MUST be ±1.0)"
            )));
        }
    }
    Ok(())
}

/// Spec §3.7.2.1: all components of each `COLOR_0` accessor element
/// MUST be clamped to `[0.0, 1.0]`. Apply only to set 0 (set N≥1 is not
/// constrained by the spec).
pub fn validate_color0_range(colors: &[[f32; 4]]) -> Result<()> {
    for (i, c) in colors.iter().enumerate() {
        for (chan, &v) in c.iter().enumerate() {
            if !(0.0..=1.0).contains(&v) {
                return Err(invalid(format!(
                    "VertexAttributeColor0Range: COLOR_0[{i}][{chan}] = {v} \
                     (spec §3.7.2.1: MUST be in [0.0, 1.0])"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_model::{Accessor, BufferView, COMPONENT_TYPE_FLOAT};
    use std::collections::HashMap;

    fn vec3_float_accessor(byte_offset: u32, count: u32, bv: u32) -> Accessor {
        Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(byte_offset),
            component_type: COMPONENT_TYPE_FLOAT,
            count,
            kind: "VEC3".to_owned(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: None,
        }
    }

    #[test]
    fn alignment_rejects_misaligned_byte_offset() {
        let acc = vec3_float_accessor(2, 3, 0); // 2 not multiple of 4
        let bvs = vec![BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length: 64,
            byte_stride: None,
            target: None,
            name: None,
        }];
        let err = validate_alignment(&acc, &bvs, true, "POSITION").unwrap_err();
        assert!(format!("{err}").contains("VertexAttributeAlignment"));
    }

    #[test]
    fn alignment_accepts_aligned_byte_offset() {
        let acc = vec3_float_accessor(8, 3, 0);
        let bvs = vec![BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length: 64,
            byte_stride: None,
            target: None,
            name: None,
        }];
        validate_alignment(&acc, &bvs, true, "POSITION").unwrap();
    }

    #[test]
    fn alignment_rejects_misaligned_stride() {
        let acc = vec3_float_accessor(0, 3, 0);
        let bvs = vec![BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length: 64,
            byte_stride: Some(13), // not multiple of 4
            target: None,
            name: None,
        }];
        let err = validate_alignment(&acc, &bvs, true, "POSITION").unwrap_err();
        assert!(format!("{err}").contains("byteStride"));
    }

    #[test]
    fn count_validation_passes_when_uniform() {
        let mut attrs = HashMap::new();
        attrs.insert("POSITION".to_owned(), 0u32);
        attrs.insert("NORMAL".to_owned(), 1u32);
        let accs = vec![vec3_float_accessor(0, 12, 0), vec3_float_accessor(0, 12, 1)];
        validate_attribute_counts(&attrs, &accs).unwrap();
    }

    #[test]
    fn count_validation_rejects_mismatch() {
        let mut attrs = HashMap::new();
        attrs.insert("POSITION".to_owned(), 0u32);
        attrs.insert("NORMAL".to_owned(), 1u32);
        let accs = vec![vec3_float_accessor(0, 12, 0), vec3_float_accessor(0, 8, 1)];
        let err = validate_attribute_counts(&attrs, &accs).unwrap_err();
        assert!(format!("{err}").contains("VertexAttributeCount"));
    }

    #[test]
    fn index_sentinel_rejected_for_u16() {
        let acc = Accessor {
            buffer_view: Some(0),
            byte_offset: None,
            component_type: COMPONENT_TYPE_UNSIGNED_SHORT,
            count: 4,
            kind: "SCALAR".to_owned(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: None,
        };
        let err = validate_index_no_restart(&acc, &[0, 1, 65535, 2]).unwrap_err();
        assert!(format!("{err}").contains("VertexAttributeIndexRestart"));
    }

    #[test]
    fn index_sentinel_accepts_safe_u16() {
        let acc = Accessor {
            buffer_view: Some(0),
            byte_offset: None,
            component_type: COMPONENT_TYPE_UNSIGNED_SHORT,
            count: 4,
            kind: "SCALAR".to_owned(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: None,
        };
        validate_index_no_restart(&acc, &[0, 1, 2, 65534]).unwrap();
    }

    #[test]
    fn tangent_w_accepts_signed_unit() {
        validate_tangent_w(&[[0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, -1.0]]).unwrap();
    }

    #[test]
    fn tangent_w_rejects_other() {
        let err = validate_tangent_w(&[[0.0, 0.0, 1.0, 0.5]]).unwrap_err();
        assert!(format!("{err}").contains("VertexAttributeTangentW"));
    }

    #[test]
    fn color0_accepts_unit_range() {
        validate_color0_range(&[[0.0, 0.5, 1.0, 1.0], [0.25, 0.75, 0.0, 0.5]]).unwrap();
    }

    #[test]
    fn color0_rejects_out_of_range() {
        let err = validate_color0_range(&[[0.0, 1.5, 1.0, 1.0]]).unwrap_err();
        assert!(format!("{err}").contains("VertexAttributeColor0Range"));
    }

    #[test]
    fn color0_rejects_negative() {
        let err = validate_color0_range(&[[0.0, -0.1, 0.5, 1.0]]).unwrap_err();
        assert!(format!("{err}").contains("VertexAttributeColor0Range"));
    }
}
