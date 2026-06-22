//! Document-level validation per glTF 2.0 §3.6.2.4 + §3.7.2.1 + §3.11
//! + §3.12.
//!
//! These checks run BEFORE the per-attribute `read_attr_*` paths (or in
//! `convert::resolve_buffers` for the document-level rules) and surface
//! MUST-level spec violations the earlier rounds didn't catch.
//!
//! Validations performed:
//!
//! Vertex-attribute compression (round 6):
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
//! Extension stack (round 7):
//!
//! * §3.12 — `extensionsRequired` MUST be a subset of `extensionsUsed`.
//! * §3.12 — every extension whose object lives somewhere in the
//!   document (root `extensions` / node `extensions`) MUST appear in
//!   `extensionsUsed`. Today this covers `KHR_lights_punctual`,
//!   `KHR_materials_unlit`, `KHR_materials_emissive_strength`,
//!   `KHR_materials_ior`, `KHR_materials_specular`,
//!   `KHR_materials_clearcoat`, `KHR_materials_sheen`,
//!   `KHR_materials_transmission`, `KHR_materials_volume`,
//!   `KHR_materials_iridescence`, `KHR_materials_anisotropy`,
//!   `KHR_materials_dispersion`, `KHR_materials_diffuse_transmission`,
//!   `KHR_texture_transform` (on any of the five core PBR textureInfo
//!   slots), `KHR_node_visibility` (on any node), and
//!   `KHR_materials_variants` (root-level `variants` roster + per-primitive
//!   `mappings`), `KHR_xmp_json_ld` (root-level `packets[]` roster +
//!   per-asset / per-scene / per-node / per-mesh / per-material
//!   `{ packet: N }` indirection), and `KHR_meshopt_compression` (per-bufferView
//!   compression descriptors + per-buffer `{ "fallback": true }` markers
//!   per `docs/3d/gltf/extensions/KHR_meshopt_compression.md`), and
//!   `KHR_gaussian_splatting` per-primitive descriptor blocks per
//!   `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` (kernel +
//!   colorSpace + projection + sortingMethod with the spec's allowed-
//!   value sets and the ellipse-kernel mode-MUST-be-POINTS dependency).
//! * `KHR_meshopt_compression` per-bufferView spec invariants from
//!   §"JSON schema updates" (mode ∈ ATTRIBUTES/TRIANGLES/INDICES,
//!   filter ∈ NONE/OCTAHEDRAL/QUATERNION/EXPONENTIAL/COLOR,
//!   parent.byteLength == byteStride * count, per-mode byteStride
//!   constraints, per-filter byteStride constraints, mode + filter
//!   compatibility for TRIANGLES/INDICES, source buffer index/range
//!   bounds) and §"Fallback buffers" invariants (fallback buffer
//!   referenced only by extension-carrying bufferViews; extension's
//!   own `buffer` MUST NOT be a fallback; uri-less fallback forces
//!   `extensionsRequired`).
//! * `KHR_materials_anisotropy.anisotropyStrength` MUST sit in `[0, 1]`
//!   per the extension spec's "Anisotropy" section ("a dimensionless
//!   number in the range [0, 1]"). The `anisotropyRotation` is
//!   interpreted modulo 2π so no range check is applied.
//! * `KHR_materials_dispersion.dispersion` MUST be finite and `>= 0`
//!   per the extension spec ("Any value zero or larger is considered
//!   to be a valid dispersion value"). Values above `1.0` are allowed
//!   for artistic exaggeration; only negative or non-finite values
//!   are rejected.
//! * `KHR_materials_diffuse_transmission.diffuseTransmissionFactor`
//!   MUST be finite and within `[0, 1]` (it is a percentage of the
//!   non-specularly-reflected light that is diffusely transmitted);
//!   `diffuseTransmissionColorFactor` MUST be finite and within
//!   `[0, 1]^3` (each component is a proportion).
//!
//! Animation channels (round 7):
//!
//! * §3.11 — every animation channel `target.path` MUST be one of
//!   `"translation"` / `"rotation"` / `"scale"` / `"weights"`.
//! * §3.11 — when `target.path == "weights"` the target node MUST point
//!   at a `mesh` AND that mesh's primitives MUST declare at least one
//!   morph target.
//! * §3.11 — every channel's `sampler` index MUST be in range; every
//!   sampler's `input` / `output` accessor indices MUST be in range.
//! * §5.26 — texture-sampler `magFilter` / `minFilter` / `wrapS` /
//!   `wrapT`, when present, MUST hold one of the spec's enumerated
//!   WebGL enum constants.
//!
//! Fuzz hardening (round 7):
//!
//! * Document-byte cap — the decoder rejects JSON payloads above
//!   `MAX_JSON_BYTES` before serde sees them, so a malicious
//!   `byteLength: u32::MAX` allocator pump can't run.
//! * JSON nesting depth — `check_json_depth` rejects payloads nesting
//!   beyond 256 levels (`MAX_JSON_DEPTH`), guarding against malicious
//!   1000-deep-array inputs that would otherwise crash the recursive
//!   serde_json parser on stack overflow.
//!
//! Asset version (round 8):
//!
//! * §3.2 + §5.9.3 — `asset.version` MUST match the
//!   `<major>.<minor>` pattern (JSON schema `^[0-9]+\.[0-9]+$`); this
//!   decoder additionally accepts only `major == 2` because the only
//!   spec edition we implement is 2.x.
//! * §3.2 + §5.9.4 — `asset.minVersion`, when present, MUST also match
//!   the `<major>.<minor>` pattern, MUST NOT be greater than
//!   `asset.version` (a spec MUST), and MUST be `≤ 2.0` because that's
//!   the highest 2.x edition the spec has defined; anything larger
//!   means the asset author requires features this decoder cannot
//!   guarantee.
//!
//! Buffer / bufferView fit (round 8):
//!
//! * §3.6.2.4 + §5.1 — every accessor MUST fit inside the bufferView
//!   that backs it: `accessor.byteOffset + EFFECTIVE_BYTE_STRIDE *
//!   (accessor.count - 1) + SIZE_OF_COMPONENT * NUMBER_OF_COMPONENTS
//!   <= bufferView.byteLength` (spec line 3104). The validator covers
//!   both tightly-packed (effective stride = element size) and
//!   strided (`bufferView.byteStride`) layouts.
//! * §5.11 — every bufferView MUST fit inside the buffer it points
//!   into: `bufferView.byteOffset + bufferView.byteLength <=
//!   buffer.byteLength`.
//! * §5.11.4 — `bufferView.byteStride`, when defined, MUST satisfy
//!   the JSON-schema range `[4, 252]`.
//! * §5.3.1 — `accessor.sparse.indices.bufferView` MUST NOT carry a
//!   `target` or `byteStride` property (the sparse-indices buffer view
//!   is a tightly-packed index array; a stride or target hint would be
//!   semantically nonsensical).
//! * §5.4.1 — `accessor.sparse.values.bufferView` MUST NOT carry a
//!   `target` or `byteStride` property either. The sparse-values block
//!   is a tightly-packed array of element-sized overrides (spec §5.4
//!   "The elements are tightly packed"), so a target or stride hint
//!   on its bufferView is the same shape of violation as the §5.3.1
//!   sparse-indices rule.
//! * §5.11.4 — the bufferView referenced by a
//!   `KHR_draco_mesh_compression` per-primitive descriptor MUST NOT
//!   carry `byteStride`. The descriptor's bufferView holds an opaque
//!   Draco-compressed payload, not vertex attribute data, and the
//!   extension does not enable a strided payload layout. Same shape
//!   as the §5.3.1 sparse-indices rule.
//!
//! Camera properties (round r277):
//!
//! * §5.12 — `camera.perspective` and `camera.orthographic` MUST NOT
//!   both be defined on one camera.
//! * §5.13 — `orthographic.xmag` / `ymag` MUST NOT be zero;
//!   `zfar > 0`, `zfar > znear`, `znear >= 0`.
//! * §5.14 — `perspective.yfov > 0`, `znear > 0`; `aspectRatio`
//!   (when defined) `> 0`; `zfar` (when defined) `> 0` and
//!   `> znear`. Non-finite values are rejected everywhere; the
//!   spec's SHOULD-level advice (non-negative magnification,
//!   `yfov < π`) is NOT enforced.
//!
//! All failures surface as `Error::InvalidData` with a stable
//! `VertexAttribute…` / `ExtensionStack…` / `AnimationChannel…` /
//! `JsonDepthExceeded` / `JsonTooLarge` / `AssetVersion…` /
//! `AccessorFit…` / `BufferViewFit…` / `BufferViewStride…` / `Camera…` /
//! `SparseIndicesBufferView…` / `SparseValuesBufferView…` prefix so callers can grep for the
//! specific sub-rule without reaching for a typed enum (the shared
//! `oxideav_core::Error` enum can't gain a new variant from a sibling
//! crate).

use crate::error::{invalid, Result};
use crate::json_model::{
    component_size, type_components, Accessor, Animation, Buffer, BufferView, Camera, GltfRoot,
    Material, Mesh, Node, Scene, Skin, Texture, COMPONENT_TYPE_FLOAT, COMPONENT_TYPE_UNSIGNED_BYTE,
    COMPONENT_TYPE_UNSIGNED_INT, COMPONENT_TYPE_UNSIGNED_SHORT,
};
use crate::object_model::{pointer_data_type, ObjectModelDataType};
use std::collections::HashMap;

/// Maximum nesting depth a glTF JSON document may declare before the
/// decoder rejects it.  Spec doesn't prescribe a bound; pick 256 — far
/// above any real-world glTF and well below any platform's default
/// stack budget (serde_json's recursive parser would otherwise blow the
/// stack on malicious 1000-deep array inputs).
pub const MAX_JSON_DEPTH: usize = 256;

/// Maximum JSON-document byte length the decoder will admit. Spec
/// doesn't prescribe a bound; this cap (128 MiB) is well above any
/// real-world glTF JSON chunk (binary buffers live in the BIN chunk
/// outside this limit) and prevents allocator pumps when a fuzzer
/// declares a huge top-level array.
pub const MAX_JSON_BYTES: usize = 128 * 1024 * 1024;

/// Reject JSON payloads larger than [`MAX_JSON_BYTES`].
///
/// Run BEFORE `serde_json::from_slice` so the parser never sees the
/// pathological input. The check is a single `len()` comparison — no
/// allocation, no scan.
pub fn check_json_byte_length(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(invalid(format!(
            "JsonTooLarge: glTF JSON is {} bytes > cap {MAX_JSON_BYTES} \
             (set higher via tooling, or split into external buffers)",
            bytes.len()
        )));
    }
    Ok(())
}

/// Reject JSON payloads that nest deeper than [`MAX_JSON_DEPTH`].
///
/// Run BEFORE `serde_json::from_slice` to bound recursion before the
/// parser starts; otherwise nested-array bombs (e.g. 1000-deep `[[[[...`)
/// crash the recursive descent parser on stack overflow.
///
/// The scan walks `bytes` once, in linear time, tracking `{` / `[` /
/// `}` / `]` nesting while honouring JSON string + escape syntax so a
/// `[` inside a `"..."` string doesn't count. Unicode escapes (`\uXXXX`)
/// inside strings pass through without affecting depth — they decode to
/// a single code unit, not a nested structure.
pub fn check_json_depth(bytes: &[u8]) -> Result<()> {
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > max_depth {
                    max_depth = depth;
                }
                if depth > MAX_JSON_DEPTH {
                    return Err(invalid(format!(
                        "JsonDepthExceeded: glTF JSON nests deeper than {MAX_JSON_DEPTH} levels \
                         (reached {depth} at byte offset {i})"
                    )));
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

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

/// Spec §3.7.2.1: the number of vertex indices to render MUST be valid
/// for the topology type used. When `indices` is defined the count is the
/// `indices` accessor's `count`; otherwise it is the (shared) attribute
/// accessors' `count`. Per the spec's bulleted list:
///
/// * POINTS — MUST be non-zero.
/// * LINE_LOOP / LINE_STRIP — MUST be 2 or greater.
/// * TRIANGLE_STRIP / TRIANGLE_FAN — MUST be 3 or greater.
/// * LINES — MUST be divisible by 2 and non-zero.
/// * TRIANGLES — MUST be divisible by 3 and non-zero.
///
/// `mode` is the raw glTF primitive `mode` enum (defaulting to TRIANGLES
/// upstream). An unknown `mode` is rejected before this point by
/// `topology_from_mode`, so it cannot reach here.
pub fn validate_primitive_index_count(mode: u32, num_indices: u64) -> Result<()> {
    use crate::json_model::{
        MODE_LINES, MODE_LINE_LOOP, MODE_LINE_STRIP, MODE_POINTS, MODE_TRIANGLES,
        MODE_TRIANGLE_FAN, MODE_TRIANGLE_STRIP,
    };

    // `name` is the spec's topology label; `rule` is the human-readable
    // MUST text reproduced for the error message.
    let fail = |name: &str, rule: &str| {
        Err(invalid(format!(
            "PrimitiveIndexCount: {name} primitive has {num_indices} vertex indices \
             (spec §3.7.2.1: for {name}, {rule})"
        )))
    };

    match mode {
        MODE_POINTS if num_indices == 0 => fail("points", "the count MUST be non-zero"),
        MODE_LINE_LOOP if num_indices < 2 => fail("line loops", "the count MUST be 2 or greater"),
        MODE_LINE_STRIP if num_indices < 2 => fail("line strips", "the count MUST be 2 or greater"),
        MODE_TRIANGLE_STRIP if num_indices < 3 => {
            fail("triangle strips", "the count MUST be 3 or greater")
        }
        MODE_TRIANGLE_FAN if num_indices < 3 => {
            fail("triangle fans", "the count MUST be 3 or greater")
        }
        MODE_LINES if num_indices == 0 || num_indices % 2 != 0 => {
            fail("lines", "the count MUST be divisible by 2 and non-zero")
        }
        MODE_TRIANGLES if num_indices == 0 || num_indices % 3 != 0 => {
            fail("triangles", "the count MUST be divisible by 3 and non-zero")
        }
        // All conforming counts, plus any other mode (unreachable in
        // practice — `topology_from_mode` rejects unknown modes before a
        // primitive is converted), are accepted.
        _ => Ok(()),
    }
}

/// Spec §3.7.2.1: when the `indices` property is defined, every index
/// value MUST be less than the attribute accessors' `count` (the index
/// accessor's range is the *upper exclusive bound* on addressable
/// vertices). `attr_count` is the shared attribute count established by
/// [`validate_attribute_counts`].
pub fn validate_index_value_bound(indices: &[u32], attr_count: u32) -> Result<()> {
    if let Some(pos) = indices.iter().position(|&i| i >= attr_count) {
        return Err(invalid(format!(
            "PrimitiveIndexBound: indices[{pos}] = {} >= attribute count {attr_count} \
             (spec §3.7.2.1: all index values MUST be less than the attribute accessors' count)",
            indices[pos]
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

/// `KHR_gaussian_splatting` §"Ellipse Kernel" §"Attributes" — validate
/// the per-attribute storage contract of an `"ellipse"` kernel splat
/// primitive against the accessor table.
///
/// The ellipse kernel defines an exact accessor type + component-type +
/// normalized layout for each splat-field semantic
/// (`docs/3d/gltf/extensions/KHR_gaussian_splatting.md`, the Attributes
/// table):
///
/// | Semantic                          | Type   | Component types |
/// |-----------------------------------|--------|-----------------|
/// | `POSITION`                        | VEC3   | inherited       |
/// | `KHR_gaussian_splatting:ROTATION` | VEC4   | float / sbyte-n / sshort-n |
/// | `KHR_gaussian_splatting:SCALE`    | VEC3   | float / ubyte(-n) / ushort(-n) |
/// | `KHR_gaussian_splatting:OPACITY`  | SCALAR | float / ubyte-n / ushort-n |
/// | `KHR_gaussian_splatting:SH_DEGREE_l_COEF_n` | VEC3 | float |
///
/// All five core semantics (`POSITION`, `ROTATION`, `SCALE`, `OPACITY`,
/// `SH_DEGREE_0_COEF_0`) are required; the higher spherical-harmonics
/// degrees are optional but MUST be fully defined — for any degree `l`
/// in 1..=3 that is partially present, all `2l + 1` coefficients of that
/// degree AND every lower degree MUST be defined ("either all
/// coefficients for a given degree and all lower degrees MUST be defined
/// or none").
fn validate_gaussian_splatting_attributes(
    root: &GltfRoot,
    prim: &crate::json_model::Primitive,
    mi: usize,
    pi: usize,
) -> Result<()> {
    use crate::json_model::{
        COMPONENT_TYPE_BYTE, COMPONENT_TYPE_FLOAT, COMPONENT_TYPE_SHORT,
        COMPONENT_TYPE_UNSIGNED_BYTE, COMPONENT_TYPE_UNSIGNED_SHORT,
    };

    const ROTATION: &str = "KHR_gaussian_splatting:ROTATION";
    const SCALE: &str = "KHR_gaussian_splatting:SCALE";
    const OPACITY: &str = "KHR_gaussian_splatting:OPACITY";
    const SH_0_0: &str = "KHR_gaussian_splatting:SH_DEGREE_0_COEF_0";

    // Look up an attribute's accessor and verify its `type` + component
    // layout. `comps` is `(kind, allowed)` where each allowed entry is
    // `(component_type, normalized_requirement)`; a `None` normalized
    // requirement accepts either flag, `Some(b)` requires `normalized == b`.
    let check =
        |name: &str, kind: &str, allowed: &[(u32, Option<bool>)], label: &str| -> Result<()> {
            let idx = match prim.attributes.get(name) {
                Some(&i) => i,
                // Presence of optional attributes is enforced separately; a
                // missing attribute here is not this check's concern.
                None => return Ok(()),
            };
            let acc = root.accessors.get(idx as usize).ok_or_else(|| {
                invalid(format!(
                    "ExtensionStackGaussianSplattingAttributeAccessor: \
                 meshes[{mi}].primitives[{pi}].attributes[{name:?}] = {idx} \
                 is out of range of accessors[] ({label})"
                ))
            })?;
            if acc.kind != kind {
                return Err(invalid(format!(
                    "ExtensionStackGaussianSplattingAttributeType: \
                 meshes[{mi}].primitives[{pi}].attributes[{name:?}] \
                 accessor type = {:?} but the ellipse kernel requires \
                 {kind:?} ({label})",
                    acc.kind
                )));
            }
            let ok = allowed.iter().any(|&(ct, norm)| {
                acc.component_type == ct && norm.map_or(true, |n| acc.normalized == n)
            });
            if !ok {
                return Err(invalid(format!(
                    "ExtensionStackGaussianSplattingAttributeComponent: \
                 meshes[{mi}].primitives[{pi}].attributes[{name:?}] \
                 accessor componentType = {} (normalized = {}) is not a \
                 spec-allowed storage form ({label})",
                    acc.component_type, acc.normalized
                )));
            }
            Ok(())
        };

    // POSITION is required and its storage is governed by the base glTF
    // specification (already validated by the vertex-attribute pass); we
    // only enforce its presence here.
    if !prim.attributes.contains_key("POSITION") {
        return Err(invalid(format!(
            "ExtensionStackGaussianSplattingMissingAttribute: \
             meshes[{mi}].primitives[{pi}] uses the ellipse kernel but is \
             missing the required POSITION attribute (§\"Ellipse Kernel\" \
             §\"Attributes\")"
        )));
    }
    for &name in &[ROTATION, SCALE, OPACITY, SH_0_0] {
        if !prim.attributes.contains_key(name) {
            return Err(invalid(format!(
                "ExtensionStackGaussianSplattingMissingAttribute: \
                 meshes[{mi}].primitives[{pi}] uses the ellipse kernel but \
                 is missing the required attribute {name:?} (§\"Ellipse \
                 Kernel\" §\"Attributes\")"
            )));
        }
    }

    // ROTATION: VEC4 — float / signed byte normalized / signed short
    // normalized.
    check(
        ROTATION,
        "VEC4",
        &[
            (COMPONENT_TYPE_FLOAT, None),
            (COMPONENT_TYPE_BYTE, Some(true)),
            (COMPONENT_TYPE_SHORT, Some(true)),
        ],
        "rotation quaternion",
    )?;
    // SCALE: VEC3 — float / unsigned byte (raw or normalized) / unsigned
    // short (raw or normalized).
    check(
        SCALE,
        "VEC3",
        &[
            (COMPONENT_TYPE_FLOAT, None),
            (COMPONENT_TYPE_UNSIGNED_BYTE, None),
            (COMPONENT_TYPE_UNSIGNED_SHORT, None),
        ],
        "scale",
    )?;
    // OPACITY: SCALAR — float / unsigned byte normalized / unsigned short
    // normalized.
    check(
        OPACITY,
        "SCALAR",
        &[
            (COMPONENT_TYPE_FLOAT, None),
            (COMPONENT_TYPE_UNSIGNED_BYTE, Some(true)),
            (COMPONENT_TYPE_UNSIGNED_SHORT, Some(true)),
        ],
        "opacity",
    )?;

    // Spherical harmonics: every present `SH_DEGREE_l_COEF_n` attribute is
    // a VEC3 of floats.
    for name in prim.attributes.keys() {
        if name.starts_with("KHR_gaussian_splatting:SH_DEGREE_") {
            check(
                name,
                "VEC3",
                &[(COMPONENT_TYPE_FLOAT, None)],
                "spherical-harmonics coefficient",
            )?;
        }
    }

    // §"Spherical Harmonics Attributes" — degree-completeness. For each
    // degree `l` in 1..=3 that is referenced by ANY present coefficient,
    // every coefficient `0..=2l` of that degree AND of all lower degrees
    // MUST be present.
    let present = |l: u32, n: u32| -> bool {
        prim.attributes
            .contains_key(&format!("KHR_gaussian_splatting:SH_DEGREE_{l}_COEF_{n}"))
    };
    // Highest degree any coefficient references.
    let mut max_degree = 0u32;
    for name in prim.attributes.keys() {
        if let Some(rest) = name.strip_prefix("KHR_gaussian_splatting:SH_DEGREE_") {
            if let Some((deg_str, _)) = rest.split_once("_COEF_") {
                if let Ok(l) = deg_str.parse::<u32>() {
                    max_degree = max_degree.max(l);
                }
            }
        }
    }
    for l in 0..=max_degree {
        for n in 0..=(2 * l) {
            if !present(l, n) {
                return Err(invalid(format!(
                    "ExtensionStackGaussianSplattingSHIncomplete: \
                     meshes[{mi}].primitives[{pi}] references spherical \
                     harmonics up to degree {max_degree} but is missing \
                     KHR_gaussian_splatting:SH_DEGREE_{l}_COEF_{n} — each \
                     used degree and all lower degrees MUST be fully \
                     defined (§\"Spherical Harmonics Attributes\")"
                )));
            }
        }
    }

    Ok(())
}

/// `KHR_lights_punctual` per-light property + reference validation.
///
/// Enforces the constraints stated in
/// `docs/3d/gltf/extensions/KHR_lights_punctual.md`:
///
/// * §"Light Types": every light's `type` MUST be one of `directional`,
///   `point`, or `spot`. `color` (default `[1,1,1]`) and `intensity`
///   (default `1.0`) MUST be finite; `intensity` MUST be `>= 0`.
/// * §"Range Property": `range` is "allowed only on point and spot
///   lights" and "Must be > 0"; it is also finite. A `range` on a
///   `directional` light is rejected.
/// * §"Spot": when `type == "spot"` the `spot` property "is required".
///   `innerConeAngle` MUST be `>= 0` and `< outerConeAngle`;
///   `outerConeAngle` MUST be `> innerConeAngle` and `<= PI / 2.0`
///   (defaults: inner `0`, outer `PI / 4.0`). The `spot` property MUST
///   NOT appear on `directional` / `point` lights.
/// * Each node's `KHR_lights_punctual.light` index MUST be in range of
///   the root `lights[]` array.
pub fn validate_khr_lights_punctual(root: &GltfRoot) -> Result<()> {
    use std::f32::consts::PI;

    let lights = root
        .extensions
        .as_ref()
        .and_then(|e| e.khr_lights_punctual.as_ref())
        .map(|r| r.lights.as_slice())
        .unwrap_or(&[]);

    for (li, light) in lights.iter().enumerate() {
        let kind = light.kind.as_str();
        if !matches!(kind, "directional" | "point" | "spot") {
            return Err(invalid(format!(
                "ExtensionStackLightType: extensions.KHR_lights_punctual \
                 .lights[{li}].type = {:?} is not one of \"directional\", \
                 \"point\", \"spot\" (KHR_lights_punctual §Light Types)",
                light.kind
            )));
        }

        if let Some(c) = light.color {
            if let Some(bad) = c.iter().find(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "ExtensionStackLightColorFinite: extensions \
                     .KHR_lights_punctual.lights[{li}].color contains a \
                     non-finite component {bad} (KHR_lights_punctual §Light \
                     Types)"
                )));
            }
        }

        if let Some(i) = light.intensity {
            if !(i.is_finite() && i >= 0.0) {
                return Err(invalid(format!(
                    "ExtensionStackLightIntensity: extensions \
                     .KHR_lights_punctual.lights[{li}].intensity = {i} must be \
                     finite and >= 0 (KHR_lights_punctual §Light Types)"
                )));
            }
        }

        // `range` is allowed only on point/spot lights and MUST be > 0.
        if let Some(r) = light.range {
            if kind == "directional" {
                return Err(invalid(format!(
                    "ExtensionStackLightRange: extensions.KHR_lights_punctual \
                     .lights[{li}].range is set on a \"directional\" light but \
                     range is supported only for \"point\" and \"spot\" lights \
                     (KHR_lights_punctual §Range Property)"
                )));
            }
            if !(r.is_finite() && r > 0.0) {
                return Err(invalid(format!(
                    "ExtensionStackLightRange: extensions.KHR_lights_punctual \
                     .lights[{li}].range = {r} must be finite and > 0 \
                     (KHR_lights_punctual §Range Property)"
                )));
            }
        }

        match kind {
            "spot" => {
                // The `spot` property is required for spot lights.
                let Some(spot) = light.spot.as_ref() else {
                    return Err(invalid(format!(
                        "ExtensionStackLightSpotRequired: extensions \
                         .KHR_lights_punctual.lights[{li}] has type \"spot\" \
                         but the required `spot` property is missing \
                         (KHR_lights_punctual §Spot)"
                    )));
                };
                // Defaults per the §Spot table: inner 0, outer PI / 4.
                let inner = spot.inner_cone_angle.unwrap_or(0.0);
                let outer = spot.outer_cone_angle.unwrap_or(PI / 4.0);
                if !(inner.is_finite() && inner >= 0.0) {
                    return Err(invalid(format!(
                        "ExtensionStackLightInnerCone: extensions \
                         .KHR_lights_punctual.lights[{li}].spot.innerConeAngle \
                         = {inner} must be finite and >= 0 \
                         (KHR_lights_punctual §Spot)"
                    )));
                }
                if !(outer.is_finite() && outer <= PI / 2.0) {
                    return Err(invalid(format!(
                        "ExtensionStackLightOuterCone: extensions \
                         .KHR_lights_punctual.lights[{li}].spot.outerConeAngle \
                         = {outer} must be finite and <= PI / 2 \
                         (KHR_lights_punctual §Spot)"
                    )));
                }
                // Both angles are finite (checked above), so a `>=` test
                // is well-defined and equivalent to `!(inner < outer)`.
                if inner >= outer {
                    return Err(invalid(format!(
                        "ExtensionStackLightConeOrder: extensions \
                         .KHR_lights_punctual.lights[{li}].spot.innerConeAngle \
                         ({inner}) must be strictly less than outerConeAngle \
                         ({outer}) (KHR_lights_punctual §Spot)"
                    )));
                }
            }
            _ => {
                // `spot` only applies to spot lights.
                if light.spot.is_some() {
                    return Err(invalid(format!(
                        "ExtensionStackLightSpotMisplaced: extensions \
                         .KHR_lights_punctual.lights[{li}] has type {:?} but \
                         carries a `spot` property, which applies only to \
                         \"spot\" lights (KHR_lights_punctual §Spot)",
                        light.kind
                    )));
                }
            }
        }
    }

    // Every node's light reference MUST index a declared light.
    for (ni, node) in root.nodes.iter().enumerate() {
        let Some(reference) = node
            .extensions
            .as_ref()
            .and_then(|e| e.khr_lights_punctual.as_ref())
        else {
            continue;
        };
        if reference.light as usize >= lights.len() {
            return Err(invalid(format!(
                "ExtensionStackLightRef: nodes[{ni}].extensions \
                 .KHR_lights_punctual.light = {} is out of range of the {} \
                 declared lights (KHR_lights_punctual §Adding Light Instances \
                 to Nodes)",
                reference.light,
                lights.len()
            )));
        }
    }

    Ok(())
}

/// Spec §3.12: `extensionsRequired` MUST be a subset of `extensionsUsed`.
///
/// In addition, any extension whose object actually appears in the
/// document MUST be declared in `extensionsUsed` — today the decoder
/// understands `KHR_lights_punctual` at root scope and on nodes, plus
/// `KHR_materials_unlit` on materials; the check fires when an
/// emitter put the data block in but forgot the declaration.
pub fn validate_extension_stack(root: &GltfRoot) -> Result<()> {
    // 1. extensionsRequired ⊆ extensionsUsed.
    for required in &root.extensions_required {
        if !root.extensions_used.iter().any(|u| u == required) {
            return Err(invalid(format!(
                "ExtensionStackRequiredNotListed: {required:?} is in \
                 extensionsRequired but missing from extensionsUsed (spec §3.12)"
            )));
        }
    }

    // 2. Any extension actually present in the JSON must appear in
    //    extensionsUsed.
    let used = |name: &str| root.extensions_used.iter().any(|u| u == name);

    let has_root_lights = root
        .extensions
        .as_ref()
        .and_then(|e| e.khr_lights_punctual.as_ref())
        .is_some();
    let has_node_lights = root.nodes.iter().any(|n| {
        n.extensions
            .as_ref()
            .and_then(|e| e.khr_lights_punctual.as_ref())
            .is_some()
    });
    if (has_root_lights || has_node_lights) && !used("KHR_lights_punctual") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_lights_punctual data is present \
             but the extension is not listed in extensionsUsed (spec §3.12)",
        ));
    }
    // Per-light property + node-reference constraints (light type, range,
    // spot cone angles, light-index bounds) per
    // `docs/3d/gltf/extensions/KHR_lights_punctual.md`.
    validate_khr_lights_punctual(root)?;

    // KHR_materials_unlit — per-material extension. Same §3.12 rule:
    // the extension MUST be declared in `extensionsUsed` if any
    // material carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_unlit.md`.
    let has_material_unlit = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_unlit.as_ref())
            .is_some()
    });
    if has_material_unlit && !used("KHR_materials_unlit") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_unlit data is present \
             on a material but the extension is not listed in extensionsUsed \
             (spec §3.12)",
        ));
    }

    // KHR_materials_emissive_strength — per-material extension. Same
    // §3.12 rule: the extension MUST be declared in `extensionsUsed` if
    // any material carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_emissive_strength.md`.
    let has_emissive_strength = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_emissive_strength.as_ref())
            .is_some()
    });
    if has_emissive_strength && !used("KHR_materials_emissive_strength") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_emissive_strength data \
             is present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_ior — per-material extension. Same §3.12 rule: the
    // extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_ior.md`.
    let has_ior = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_ior.as_ref())
            .is_some()
    });
    if has_ior && !used("KHR_materials_ior") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_ior data is present \
             on a material but the extension is not listed in extensionsUsed \
             (spec §3.12)",
        ));
    }

    // KHR_materials_specular — per-material extension. Same §3.12 rule:
    // the extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_specular.md`.
    let has_specular = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_specular.as_ref())
            .is_some()
    });
    if has_specular && !used("KHR_materials_specular") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_specular data is \
             present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_clearcoat — per-material extension. Same §3.12 rule:
    // the extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_clearcoat.md`.
    let has_clearcoat = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_clearcoat.as_ref())
            .is_some()
    });
    if has_clearcoat && !used("KHR_materials_clearcoat") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_clearcoat data is \
             present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_sheen — per-material extension. Same §3.12 rule: the
    // extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_sheen.md`.
    let has_sheen = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_sheen.as_ref())
            .is_some()
    });
    if has_sheen && !used("KHR_materials_sheen") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_sheen data is \
             present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_transmission — per-material extension. Same §3.12
    // rule: the extension MUST be declared in `extensionsUsed` if any
    // material carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_transmission.md`.
    let has_transmission = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_transmission.as_ref())
            .is_some()
    });
    if has_transmission && !used("KHR_materials_transmission") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_transmission data \
             is present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_volume — per-material extension. Same §3.12 rule: the
    // extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_volume.md`.
    let has_volume = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_volume.as_ref())
            .is_some()
    });
    if has_volume && !used("KHR_materials_volume") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_volume data is \
             present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_iridescence — per-material extension. Same §3.12 rule:
    // the extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. See
    // `docs/3d/gltf/extensions/KHR_materials_iridescence.md`.
    let has_iridescence = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_iridescence.as_ref())
            .is_some()
    });
    if has_iridescence && !used("KHR_materials_iridescence") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_iridescence data \
             is present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_anisotropy — per-material extension. Same §3.12 rule:
    // the extension MUST be declared in `extensionsUsed` if any material
    // carries the data block. Also enforce the spec's range constraint
    // on `anisotropyStrength` here (the spec says it is "a dimensionless
    // number in the range [0, 1]"). See
    // `docs/3d/gltf/extensions/KHR_materials_anisotropy.md`.
    let has_anisotropy = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_anisotropy.as_ref())
            .is_some()
    });
    if has_anisotropy && !used("KHR_materials_anisotropy") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_anisotropy data \
             is present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }
    for (mi, m) in root.materials.iter().enumerate() {
        let Some(an) = m
            .extensions
            .as_ref()
            .and_then(|e| e.khr_materials_anisotropy.as_ref())
        else {
            continue;
        };
        if let Some(s) = an.anisotropy_strength {
            if !(s.is_finite() && (0.0..=1.0).contains(&s)) {
                return Err(invalid(format!(
                    "ExtensionStackAnisotropyStrengthRange: materials[{mi}] \
                     .extensions.KHR_materials_anisotropy.anisotropyStrength = \
                     {s} outside [0, 1] (KHR_materials_anisotropy §Anisotropy)"
                )));
            }
        }
        if let Some(r) = an.anisotropy_rotation {
            if !r.is_finite() {
                return Err(invalid(format!(
                    "ExtensionStackAnisotropyRotationFinite: materials[{mi}] \
                     .extensions.KHR_materials_anisotropy.anisotropyRotation = \
                     {r} is not finite \
                     (KHR_materials_anisotropy §Extending Materials)"
                )));
            }
        }
    }

    // KHR_materials_dispersion — per-material extension. Same §3.12
    // rule: the extension MUST be declared in `extensionsUsed` if any
    // material carries the data block. Also enforce the spec's
    // non-negativity constraint on `dispersion` here — per
    // `docs/3d/gltf/extensions/KHR_materials_dispersion.md` §Extending
    // Materials, "Any value zero or larger is considered to be a valid
    // dispersion value". Values above `1.0` are explicitly allowed for
    // artistic exaggeration; only negative or non-finite values are
    // rejected.
    let has_dispersion = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_dispersion.as_ref())
            .is_some()
    });
    if has_dispersion && !used("KHR_materials_dispersion") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_dispersion data \
             is present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }
    for (mi, m) in root.materials.iter().enumerate() {
        let Some(dp) = m
            .extensions
            .as_ref()
            .and_then(|e| e.khr_materials_dispersion.as_ref())
        else {
            continue;
        };
        if let Some(d) = dp.dispersion {
            if !(d.is_finite() && d >= 0.0) {
                return Err(invalid(format!(
                    "ExtensionStackDispersionRange: materials[{mi}] \
                     .extensions.KHR_materials_dispersion.dispersion = \
                     {d} is not finite and >= 0 \
                     (KHR_materials_dispersion §Extending Materials)"
                )));
            }
        }
    }

    // KHR_materials_diffuse_transmission — per-material extension.
    // Same §3.12 rule: the extension MUST be declared in
    // `extensionsUsed` if any material carries the data block. Also
    // enforce the spec's implicit range constraints — per
    // `docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md`
    // §Properties / §Diffuse Transmission, `diffuseTransmissionFactor`
    // is a "percentage" with a normative reading of `1.0 indicates
    // that 100% of the light that penetrates the surface is
    // transmitted", and `diffuseTransmissionColorFactor` is a
    // "proportion of light at each color channel". Both must be
    // finite and within `[0, 1]` (resp. `[0, 1]^3` per channel).
    let has_diffuse_transmission = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_materials_diffuse_transmission.as_ref())
            .is_some()
    });
    if has_diffuse_transmission && !used("KHR_materials_diffuse_transmission") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_diffuse_transmission \
             data is present on a material but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }
    for (mi, m) in root.materials.iter().enumerate() {
        let Some(dt) = m
            .extensions
            .as_ref()
            .and_then(|e| e.khr_materials_diffuse_transmission.as_ref())
        else {
            continue;
        };
        if let Some(f) = dt.diffuse_transmission_factor {
            if !(f.is_finite() && (0.0..=1.0).contains(&f)) {
                return Err(invalid(format!(
                    "ExtensionStackDiffuseTransmissionFactorRange: \
                     materials[{mi}].extensions.\
                     KHR_materials_diffuse_transmission.diffuseTransmissionFactor \
                     = {f} is not finite and within [0, 1] \
                     (KHR_materials_diffuse_transmission §Diffuse Transmission)"
                )));
            }
        }
        if let Some(cf) = dt.diffuse_transmission_color_factor {
            for (ci, c) in cf.iter().enumerate() {
                if !(c.is_finite() && (0.0..=1.0).contains(c)) {
                    return Err(invalid(format!(
                        "ExtensionStackDiffuseTransmissionColorRange: \
                         materials[{mi}].extensions.\
                         KHR_materials_diffuse_transmission.\
                         diffuseTransmissionColorFactor[{ci}] = {c} is not \
                         finite and within [0, 1] \
                         (KHR_materials_diffuse_transmission \
                         §Diffuse Transmission Color)"
                    )));
                }
            }
        }
    }

    // KHR_texture_transform — per-textureInfo extension. Per
    // `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
    // Updates the extension "may be defined on `textureInfo`
    // structures" — *any* textureInfo, not just the five core PBR
    // slots. So the §3.12 rule (the extension MUST be declared in
    // `extensionsUsed` if any textureInfo carries the data block) and
    // the finite-value MUSTs apply to the core PBR slots AND every
    // textureInfo nested inside a material extension
    // (`KHR_materials_specular.specularTexture`,
    // `KHR_materials_clearcoat.clearcoatNormalTexture`, …) — see
    // `material_texture_transforms`.
    let mut has_texture_transform = false;
    for (mat_idx, m) in root.materials.iter().enumerate() {
        for (slot, t) in material_texture_transforms(m) {
            has_texture_transform = true;
            // Finite-value MUSTs are slot-local and independent of the
            // declaration, so check them as we walk.
            validate_texture_transform(mat_idx, &slot, t)?;
        }
    }
    if has_texture_transform && !used("KHR_texture_transform") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_texture_transform data is \
             present on a textureInfo but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_node_visibility — per-node extension. Same §3.12 rule: the
    // extension MUST be declared in `extensionsUsed` if any node
    // carries the data block. The extension defines a single optional
    // boolean `visible` flag per `docs/3d/gltf/extensions/
    // KHR_node_visibility.md` §Extending Nodes (default `true`); the
    // boolean has no out-of-range case so no value check is needed.
    let has_node_visibility = root.nodes.iter().any(|n| {
        n.extensions
            .as_ref()
            .and_then(|e| e.khr_node_visibility.as_ref())
            .is_some()
    });
    if has_node_visibility && !used("KHR_node_visibility") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_node_visibility data is \
             present on a node but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_texture_basisu — per-texture extension carrying an
    // alternative `source` indirection to a KTX v2 image per
    // `docs/3d/gltf/extensions/KHR_texture_basisu.md` §glTF Schema
    // Updates. Three §3.12 + spec-explicit rules:
    //
    //   1. §3.12 — any document carrying the data block on any
    //      texture MUST declare the extension in `extensionsUsed`.
    //   2. §"Using Without a Fallback" — if any texture omits the
    //      base `texture.source` and relies on the extension's
    //      `source` only, `KHR_texture_basisu` MUST appear in
    //      `extensionsRequired` (the spec's "Without a Fallback"
    //      example shows it in both arrays).
    //   3. The image index in `KHR_texture_basisu.source` MUST
    //      resolve into the document's `images[]` array.
    //   4. §Overview + §"glTF Schema Updates" — the image referenced
    //      by `KHR_texture_basisu.source` is a KTX v2 resource. When
    //      that image declares a `mimeType` it MUST be `image/ktx2`
    //      ("the image that points to the KTX v2 resource uses the
    //      mimeType value of image/ktx2"). A non-`image/ktx2`
    //      `mimeType` on the basisu target image is rejected.
    let mut has_texture_basisu = false;
    let mut basisu_without_fallback = false;
    for (ti, t) in root.textures.iter().enumerate() {
        if let Some(b) = t
            .extensions
            .as_ref()
            .and_then(|e| e.khr_texture_basisu.as_ref())
        {
            has_texture_basisu = true;
            if t.source.is_none() {
                basisu_without_fallback = true;
            }
            if let Some(src) = b.source {
                if (src as usize) >= root.images.len() {
                    return Err(invalid(format!(
                        "ExtensionStackTextureBasisuSource: texture {ti} \
                         KHR_texture_basisu.source = {src} is out of range \
                         (images[].len = {})",
                        root.images.len()
                    )));
                }
                // Rule 4: when the targeted image declares a mimeType
                // it MUST be `image/ktx2` (spec §Overview + §"glTF
                // Schema Updates"). A bare image (uri-only, no mimeType)
                // is permitted — the spec only constrains the value
                // *when present*.
                if let Some(mime) = root.images[src as usize].mime_type.as_deref() {
                    if mime != "image/ktx2" {
                        return Err(invalid(format!(
                            "ExtensionStackTextureBasisuMimeType: texture {ti} \
                             KHR_texture_basisu.source = {src} targets image \
                             {src} whose mimeType is {mime:?}, but a KTX v2 \
                             Basis Universal image MUST declare mimeType \
                             \"image/ktx2\" when a mimeType is present (spec \
                             §\"glTF Schema Updates\")"
                        )));
                    }
                }
            }
        }
    }
    if has_texture_basisu && !used("KHR_texture_basisu") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_texture_basisu data is \
             present on a texture but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }
    if basisu_without_fallback
        && !root
            .extensions_required
            .iter()
            .any(|r| r == "KHR_texture_basisu")
    {
        return Err(invalid(
            "ExtensionStackTextureBasisuRequired: a texture omits the \
             base `source` and relies on KHR_texture_basisu.source, so \
             the extension MUST appear in extensionsRequired (spec \
             §\"Using Without a Fallback\")",
        ));
    }

    // KHR_meshopt_compression — per-bufferView compression descriptor
    // per `docs/3d/gltf/extensions/KHR_meshopt_compression.md`.
    //
    //   §3.12 — any document carrying the data block (on any
    //   bufferView or on any buffer) MUST declare the extension in
    //   `extensionsUsed`.
    //
    //   §"Fallback buffers" — when a fallback buffer is uri-less and
    //   doesn't refer to the GLB binary chunk, the extension MUST be
    //   listed in `extensionsRequired` ("If a fallback buffer doesn't
    //   have a URI and doesn't refer to the GLB binary chunk, it
    //   follows that KHR_meshopt_compression must be a required
    //   extension."). A fallback buffer's references must come only
    //   from bufferViews carrying the extension; the buffer index
    //   referenced by the extension JSON itself must NOT be a
    //   fallback buffer.
    //
    //   §"JSON schema updates" — per-bufferView invariants:
    //     * `mode` ∈ {ATTRIBUTES, TRIANGLES, INDICES}
    //     * `filter` ∈ {NONE, OCTAHEDRAL, QUATERNION, EXPONENTIAL,
    //                   COLOR}
    //     * parent.byteLength == byteStride * count
    //     * mode == ATTRIBUTES  ⇒ byteStride % 4 == 0 ∧
    //                              4 ≤ byteStride ≤ 256
    //     * mode == TRIANGLES   ⇒ count % 3 == 0
    //     * mode ∈ {TRIANGLES, INDICES} ⇒ byteStride ∈ {2, 4}
    //     * mode ∈ {TRIANGLES, INDICES} ⇒ filter ∈ {NONE, omitted}
    //     * filter == OCTAHEDRAL ⇒ byteStride ∈ {4, 8}
    //     * filter == QUATERNION ⇒ byteStride == 8
    //     * filter == EXPONENTIAL⇒ byteStride % 4 == 0
    //     * filter == COLOR      ⇒ byteStride ∈ {4, 8}
    //     * extension's `buffer` index resolves into `buffers[]`
    //     * extension's compressed range fits within the source
    //       buffer's declared `byteLength`
    //
    //   §"Exclusions" — `KHR_meshopt_compression` MUST NOT appear on
    //   a bufferView (or buffer) that also uses
    //   `EXT_meshopt_compression`. We currently don't surface
    //   `EXT_meshopt_compression` (it's a pre-ratification name), so
    //   we don't attempt to detect the collision; if the JSON carries
    //   an `EXT_meshopt_compression` block alongside, it stays in the
    //   bufferView's `extras` and round-trips through there.
    let mut has_meshopt = false;
    let mut fallback_buffers: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (bi, buf) in root.buffers.iter().enumerate() {
        if buf
            .extensions
            .as_ref()
            .and_then(|e| e.khr_meshopt_compression.as_ref())
            .map(|m| m.fallback)
            .unwrap_or(false)
        {
            has_meshopt = true;
            fallback_buffers.insert(bi as u32);
        }
    }
    // A fallback buffer with no URI is the "required" trigger per the
    // spec; a fallback buffer with a URI is permitted but doesn't
    // force `extensionsRequired`.
    let mut fallback_without_uri = false;
    for &bi in &fallback_buffers {
        if root.buffers[bi as usize].uri.is_none() {
            fallback_without_uri = true;
        }
    }
    for (bvi, bv) in root.buffer_views.iter().enumerate() {
        let Some(mc) = bv
            .extensions
            .as_ref()
            .and_then(|e| e.khr_meshopt_compression.as_ref())
        else {
            continue;
        };
        has_meshopt = true;
        // Parent layout must equal byteStride * count.
        let declared = mc.byte_stride.checked_mul(mc.count).ok_or_else(|| {
            invalid(format!(
                "ExtensionStackMeshoptLayout: bufferView[{bvi}] \
                     KHR_meshopt_compression: byteStride * count overflows u32"
            ))
        })?;
        if bv.byte_length != declared {
            return Err(invalid(format!(
                "ExtensionStackMeshoptLayout: bufferView[{bvi}].byteLength = {} \
                 != byteStride ({}) * count ({}) = {} \
                 (spec §\"JSON schema updates\")",
                bv.byte_length, mc.byte_stride, mc.count, declared
            )));
        }
        // Mode + per-mode byteStride / count / filter invariants.
        match mc.mode.as_str() {
            "ATTRIBUTES" => {
                if mc.byte_stride % 4 != 0 || !(4..=256).contains(&mc.byte_stride) {
                    return Err(invalid(format!(
                        "ExtensionStackMeshoptStride: bufferView[{bvi}] mode \"ATTRIBUTES\" \
                         requires byteStride divisible by 4 in [4, 256] (got {})",
                        mc.byte_stride
                    )));
                }
            }
            "TRIANGLES" => {
                if mc.count % 3 != 0 {
                    return Err(invalid(format!(
                        "ExtensionStackMeshoptCount: bufferView[{bvi}] mode \"TRIANGLES\" \
                         requires count divisible by 3 (got {})",
                        mc.count
                    )));
                }
                if mc.byte_stride != 2 && mc.byte_stride != 4 {
                    return Err(invalid(format!(
                        "ExtensionStackMeshoptStride: bufferView[{bvi}] mode \"TRIANGLES\" \
                         requires byteStride ∈ {{2, 4}} (got {})",
                        mc.byte_stride
                    )));
                }
                if let Some(f) = mc.filter.as_deref() {
                    if f != "NONE" {
                        return Err(invalid(format!(
                            "ExtensionStackMeshoptFilter: bufferView[{bvi}] mode \"TRIANGLES\" \
                             requires filter omitted or \"NONE\" (got {:?})",
                            f
                        )));
                    }
                }
            }
            "INDICES" => {
                if mc.byte_stride != 2 && mc.byte_stride != 4 {
                    return Err(invalid(format!(
                        "ExtensionStackMeshoptStride: bufferView[{bvi}] mode \"INDICES\" \
                         requires byteStride ∈ {{2, 4}} (got {})",
                        mc.byte_stride
                    )));
                }
                if let Some(f) = mc.filter.as_deref() {
                    if f != "NONE" {
                        return Err(invalid(format!(
                            "ExtensionStackMeshoptFilter: bufferView[{bvi}] mode \"INDICES\" \
                             requires filter omitted or \"NONE\" (got {:?})",
                            f
                        )));
                    }
                }
            }
            other => {
                return Err(invalid(format!(
                    "ExtensionStackMeshoptMode: bufferView[{bvi}] mode {:?} not one of \
                     \"ATTRIBUTES\", \"TRIANGLES\", \"INDICES\" \
                     (spec §\"JSON schema updates\")",
                    other
                )));
            }
        }
        // Filter-specific byteStride invariants (already partially
        // policed above for TRIANGLES / INDICES; here we cover
        // ATTRIBUTES + omitted-mode-aware checks).
        if let Some(f) = mc.filter.as_deref() {
            match f {
                "NONE" => {}
                "OCTAHEDRAL" => {
                    if mc.byte_stride != 4 && mc.byte_stride != 8 {
                        return Err(invalid(format!(
                            "ExtensionStackMeshoptFilter: bufferView[{bvi}] filter \"OCTAHEDRAL\" \
                             requires byteStride ∈ {{4, 8}} (got {})",
                            mc.byte_stride
                        )));
                    }
                }
                "QUATERNION" => {
                    if mc.byte_stride != 8 {
                        return Err(invalid(format!(
                            "ExtensionStackMeshoptFilter: bufferView[{bvi}] filter \"QUATERNION\" \
                             requires byteStride == 8 (got {})",
                            mc.byte_stride
                        )));
                    }
                }
                "EXPONENTIAL" => {
                    if mc.byte_stride % 4 != 0 {
                        return Err(invalid(format!(
                            "ExtensionStackMeshoptFilter: bufferView[{bvi}] filter \"EXPONENTIAL\" \
                             requires byteStride divisible by 4 (got {})",
                            mc.byte_stride
                        )));
                    }
                }
                "COLOR" => {
                    if mc.byte_stride != 4 && mc.byte_stride != 8 {
                        return Err(invalid(format!(
                            "ExtensionStackMeshoptFilter: bufferView[{bvi}] filter \"COLOR\" \
                             requires byteStride ∈ {{4, 8}} (got {})",
                            mc.byte_stride
                        )));
                    }
                }
                other => {
                    return Err(invalid(format!(
                        "ExtensionStackMeshoptFilter: bufferView[{bvi}] filter {:?} not one of \
                         \"NONE\", \"OCTAHEDRAL\", \"QUATERNION\", \"EXPONENTIAL\", \"COLOR\" \
                         (spec §\"JSON schema updates\")",
                        other
                    )));
                }
            }
        }
        // Source buffer + range bounds: the compressed payload must
        // resolve into an existing buffer and fit within its declared
        // `byteLength`.
        let src_buf = mc.buffer as usize;
        if src_buf >= root.buffers.len() {
            return Err(invalid(format!(
                "ExtensionStackMeshoptBuffer: bufferView[{bvi}] \
                 KHR_meshopt_compression.buffer = {} is out of range \
                 (buffers[].len = {})",
                mc.buffer,
                root.buffers.len()
            )));
        }
        let src_offset = mc.byte_offset.unwrap_or(0) as u64;
        let end = src_offset + mc.byte_length as u64;
        if end > root.buffers[src_buf].byte_length as u64 {
            return Err(invalid(format!(
                "ExtensionStackMeshoptRange: bufferView[{bvi}] \
                 KHR_meshopt_compression compressed range [{}, {}) \
                 overruns buffers[{}].byteLength = {}",
                src_offset, end, src_buf, root.buffers[src_buf].byte_length
            )));
        }
        // §"Fallback buffers": the buffer index referenced by the
        // extension JSON itself MUST NOT be a fallback buffer ("No
        // references to the buffer may come from KHR_meshopt_compression
        // extension JSON").
        if fallback_buffers.contains(&mc.buffer) {
            return Err(invalid(format!(
                "ExtensionStackMeshoptFallbackSource: bufferView[{bvi}] \
                 KHR_meshopt_compression.buffer = {} resolves to a buffer \
                 marked `fallback: true` (spec §\"Fallback buffers\": \
                 \"No references to the buffer may come from \
                 KHR_meshopt_compression extension JSON\")",
                mc.buffer
            )));
        }
        // §"Fallback buffers": when the parent bufferView's `buffer`
        // index points at a fallback buffer, the bufferView MUST itself
        // carry the extension. Since we already are inside an
        // `if mc.is_some()` branch, this direction is satisfied. The
        // converse — references to a fallback buffer from a
        // bufferView WITHOUT the extension — is policed in a separate
        // post-loop scan below.
    }
    // Second pass: every reference to a fallback buffer must come
    // from a bufferView carrying KHR_meshopt_compression.
    if !fallback_buffers.is_empty() {
        for (bvi, bv) in root.buffer_views.iter().enumerate() {
            if !fallback_buffers.contains(&bv.buffer) {
                continue;
            }
            let has_ext = bv
                .extensions
                .as_ref()
                .and_then(|e| e.khr_meshopt_compression.as_ref())
                .is_some();
            if !has_ext {
                return Err(invalid(format!(
                    "ExtensionStackMeshoptFallbackRef: bufferView[{bvi}] references \
                     buffers[{}] which is marked `fallback: true`, but the bufferView \
                     does NOT carry KHR_meshopt_compression (spec §\"Fallback buffers\": \
                     \"All references to the buffer must come from bufferViews that \
                     have a KHR_meshopt_compression extension specified\")",
                    bv.buffer
                )));
            }
        }
    }
    if has_meshopt && !used("KHR_meshopt_compression") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_meshopt_compression data is \
             present (on a bufferView or buffer) but the extension is not \
             listed in extensionsUsed (spec §3.12)",
        ));
    }
    if fallback_without_uri
        && !root
            .extensions_required
            .iter()
            .any(|r| r == "KHR_meshopt_compression")
    {
        return Err(invalid(
            "ExtensionStackMeshoptRequired: a fallback buffer is uri-less \
             (no URI and not the GLB binary chunk), so KHR_meshopt_compression \
             MUST appear in extensionsRequired (spec §\"Fallback buffers\")",
        ));
    }

    // KHR_animation_pointer — per-channel-target extension. Per
    // `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Extension
    // Usage": when used the channel's `target.path` MUST be
    // `"pointer"`, `target.node` MUST NOT be set, and the JSON Pointer
    // string lives at `target.extensions.KHR_animation_pointer.pointer`.
    // §3.12 rule: any document carrying the data block MUST declare the
    // extension in `extensionsUsed`.
    let mut has_animation_pointer = false;
    for (ai, anim) in root.animations.iter().enumerate() {
        for (ci, ch) in anim.channels.iter().enumerate() {
            let ptr = ch
                .target
                .extensions
                .as_ref()
                .and_then(|e| e.khr_animation_pointer.as_ref());
            let path_is_pointer = ch.target.path == "pointer";
            if ptr.is_some() || path_is_pointer {
                has_animation_pointer = true;
            }
            // Consistency: data block iff `path == "pointer"`. These
            // are spec §"Extension Usage" rules — surfaced as
            // ExtensionStackAnimationPointer<…> for grep-ability with
            // the existing extension-stack error vocabulary.
            if ptr.is_some() && !path_is_pointer {
                return Err(invalid(format!(
                    "ExtensionStackAnimationPointerPath: animations[{ai}].channels[{ci}] \
                     carries KHR_animation_pointer data but target.path = {:?} \
                     (must be \"pointer\")",
                    ch.target.path
                )));
            }
            if path_is_pointer && ptr.is_none() {
                return Err(invalid(format!(
                    "ExtensionStackAnimationPointerData: animations[{ai}].channels[{ci}] \
                     has target.path = \"pointer\" but no KHR_animation_pointer \
                     extension data is attached"
                )));
            }
            if ptr.is_some() && ch.target.node.is_some() {
                return Err(invalid(format!(
                    "ExtensionStackAnimationPointerNode: animations[{ai}].channels[{ci}] \
                     carries KHR_animation_pointer data but target.node is set \
                     (the spec forbids combining the two — \"animation channel `node` \
                     property MUST NOT be set\")"
                )));
            }
            // Pointer-string sanity per RFC 6901: an empty string is
            // valid (it references the whole document), but a non-empty
            // pointer MUST start with `/`. The spec §Operation says the
            // pointer MUST point to a property defined in the asset; we
            // can't validate the resolution itself without the full
            // Object Model, but the syntactic prefix check rejects the
            // clearly-malformed values that no glTF asset can satisfy.
            if let Some(p) = ptr {
                if !p.pointer.is_empty() && !p.pointer.starts_with('/') {
                    return Err(invalid(format!(
                        "ExtensionStackAnimationPointerSyntax: animations[{ai}].channels[{ci}] \
                         .target.extensions.KHR_animation_pointer.pointer = {:?} — \
                         non-empty JSON Pointers MUST start with '/' (RFC 6901 §3)",
                        p.pointer
                    )));
                }
                // Object-Model data-type rules — when the pointer
                // resolves through the registry of staged pointer
                // templates (`object_model::pointer_data_type`) to a
                // `bool` property, three MUSTs from
                // `docs/3d/gltf/extensions/KHR_animation_pointer.md`
                // bind the channel's sampler + output accessor:
                // §Operation data-type table pins `bool` → SCALAR;
                // §"Output Accessor Component Types": "the output
                // accessor component type MUST be unsigned byte"; and
                // "Animation samplers used with `int` or `bool` Object
                // Model Data Types MUST use STEP interpolation".
                // Out-of-range sampler / accessor indices are skipped
                // here — `validate_animation_channels` owns those.
                if pointer_data_type(&p.pointer) == Some(ObjectModelDataType::Bool) {
                    if let Some(s) = anim.samplers.get(ch.sampler as usize) {
                        if s.interpolation.as_deref() != Some("STEP") {
                            return Err(invalid(format!(
                                "ExtensionStackAnimationPointerBoolInterpolation: \
                                 animations[{ai}].channels[{ci}] targets the bool-typed \
                                 property {:?} but its sampler interpolation is {:?} — \
                                 samplers used with `int` or `bool` Object Model Data \
                                 Types MUST use STEP interpolation \
                                 (KHR_animation_pointer §\"Output Accessor Component Types\")",
                                p.pointer,
                                s.interpolation.as_deref().unwrap_or("LINEAR")
                            )));
                        }
                        if let Some(out_acc) = root.accessors.get(s.output as usize) {
                            if out_acc.kind != "SCALAR" {
                                return Err(invalid(format!(
                                    "ExtensionStackAnimationPointerBoolType: \
                                     animations[{ai}].channels[{ci}] targets the bool-typed \
                                     property {:?} but the output accessor type is {:?} — \
                                     the §Operation data-type table pins `bool` to SCALAR \
                                     (KHR_animation_pointer)",
                                    p.pointer, out_acc.kind
                                )));
                            }
                            if out_acc.component_type
                                != crate::json_model::COMPONENT_TYPE_UNSIGNED_BYTE
                            {
                                return Err(invalid(format!(
                                    "ExtensionStackAnimationPointerBoolComponentType: \
                                     animations[{ai}].channels[{ci}] targets the bool-typed \
                                     property {:?} but the output accessor componentType is \
                                     {} — \"the output accessor component type MUST be \
                                     unsigned byte\" (5121) \
                                     (KHR_animation_pointer §\"Output Accessor Component Types\")",
                                    p.pointer, out_acc.component_type
                                )));
                            }
                        }
                    }
                }
            }
        }
        // Per spec §"Extension Usage" (re-stating the §3.11 rule for
        // pointer-targeted channels): "The same property MUST NOT be
        // targeted more than once in one animation". Enforce uniqueness
        // of pointer strings within a single animation.
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (ci, ch) in anim.channels.iter().enumerate() {
            if let Some(p) = ch
                .target
                .extensions
                .as_ref()
                .and_then(|e| e.khr_animation_pointer.as_ref())
            {
                if !seen.insert(p.pointer.as_str()) {
                    return Err(invalid(format!(
                        "ExtensionStackAnimationPointerDuplicate: animations[{ai}].channels[{ci}] \
                         .target.extensions.KHR_animation_pointer.pointer = {:?} — \
                         the same pointer appears on more than one channel in this animation \
                         (spec §\"Operation\": \"different channels of the same animation MUST NOT \
                         have identical pointers\")",
                        p.pointer
                    )));
                }
            }
        }
    }
    if has_animation_pointer && !used("KHR_animation_pointer") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_animation_pointer data \
             is present on an animation channel but the extension is not \
             listed in extensionsUsed (spec §3.12)",
        ));
    }

    // KHR_materials_variants — both a root-level `variants` roster and
    // per-primitive `mappings` arrays surface this extension. Same
    // §3.12 rule: presence of either data block requires the extension
    // to be listed in `extensionsUsed`. See
    // `docs/3d/gltf/extensions/KHR_materials_variants.md`.
    let has_root_variants = root
        .extensions
        .as_ref()
        .and_then(|e| e.khr_materials_variants.as_ref())
        .is_some();
    let has_primitive_variants = root.meshes.iter().any(|m| {
        m.primitives.iter().any(|p| {
            p.extensions
                .as_ref()
                .and_then(|e| e.khr_materials_variants.as_ref())
                .is_some()
        })
    });
    if (has_root_variants || has_primitive_variants) && !used("KHR_materials_variants") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_materials_variants data is \
             present but the extension is not listed in extensionsUsed \
             (spec §3.12)",
        ));
    }

    // KHR_gaussian_splatting — per-primitive descriptor block per
    // `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Extending
    // Mesh Primitives". Spec §3.12 stack rule: presence of the data
    // block requires the extension to be listed in `extensionsUsed`.
    // Additional spec invariants (from the same section, §"Color Space",
    // §"Projection", §"Sorting Method", and §"Ellipse Kernel" §"Dependencies
    // on glTF"):
    //
    //  * `kernel` MUST be one of the spec-defined strings — the base
    //    spec defines `"ellipse"` and notes that additional kernels
    //    can be introduced by separate extensions. The validator
    //    therefore accepts the base value and any string prefixed
    //    `KHR_` / `EXT_` / vendor-prefixed (`MSFT_`, `ADOBE_`, …) to
    //    allow forward-compatible kernel extensions to layer on top.
    //  * `colorSpace` MUST be one of `"srgb_rec709_display"` or
    //    `"lin_rec709_display"`, with the same forward-compat carve-out
    //    for vendor/extension-prefixed strings.
    //  * `projection` (when present) MUST be `"perspective"` with the
    //    same forward-compat carve-out.
    //  * `sortingMethod` (when present) MUST be `"cameraDistance"` with
    //    the same forward-compat carve-out.
    //  * §"Ellipse Kernel" §"Dependencies on glTF" — when `kernel ==
    //    "ellipse"` the primitive's `mode` MUST be `POINTS` (0). For
    //    other kernels this validator defers to the kernel-defining
    //    extension.
    let has_splatting = root.meshes.iter().any(|m| {
        m.primitives.iter().any(|p| {
            p.extensions
                .as_ref()
                .and_then(|e| e.khr_gaussian_splatting.as_ref())
                .is_some()
        })
    });
    if has_splatting && !used("KHR_gaussian_splatting") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_gaussian_splatting data is \
             present on a primitive but the extension is not listed in \
             extensionsUsed (spec §3.12)",
        ));
    }
    // Helper: a string is "spec-known or vendor-prefixed". The two
    // prefixes that count as vendor-extension namespaces in the glTF
    // ecosystem are anything containing an underscore-separated prefix
    // followed by the body. We accept the spec strings and conservatively
    // accept any non-spec value containing `_` (treating it as a vendor
    // namespace handshake forwarded from a layered extension), while
    // rejecting bare unknown identifiers that have no namespace marker.
    fn spec_or_extension(name: &str, spec_values: &[&str]) -> bool {
        if spec_values.contains(&name) {
            return true;
        }
        // Vendor-extension strings carry an underscore-separated
        // prefix per the glTF naming convention (`KHR_`, `EXT_`,
        // `MSFT_`, `ADOBE_`, etc.). A non-empty prefix terminated by
        // `_` plus a non-empty body suffices.
        if let Some((prefix, body)) = name.split_once('_') {
            !prefix.is_empty() && !body.is_empty()
        } else {
            false
        }
    }
    const SPLAT_KERNELS: &[&str] = &["ellipse"];
    const SPLAT_COLOR_SPACES: &[&str] = &["srgb_rec709_display", "lin_rec709_display"];
    const SPLAT_PROJECTIONS: &[&str] = &["perspective"];
    const SPLAT_SORTING: &[&str] = &["cameraDistance"];
    for (mi, mesh) in root.meshes.iter().enumerate() {
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            let splat = match prim
                .extensions
                .as_ref()
                .and_then(|e| e.khr_gaussian_splatting.as_ref())
            {
                Some(s) => s,
                None => continue,
            };
            if !spec_or_extension(&splat.kernel, SPLAT_KERNELS) {
                return Err(invalid(format!(
                    "ExtensionStackGaussianSplattingKernel: meshes[{mi}].primitives[{pi}] \
                     .extensions.KHR_gaussian_splatting.kernel = {:?} is not the spec-defined \
                     value \"ellipse\" or a vendor-extension-prefixed identifier",
                    splat.kernel
                )));
            }
            if !spec_or_extension(&splat.color_space, SPLAT_COLOR_SPACES) {
                return Err(invalid(format!(
                    "ExtensionStackGaussianSplattingColorSpace: meshes[{mi}].primitives[{pi}] \
                     .extensions.KHR_gaussian_splatting.colorSpace = {:?} is not one of \
                     \"srgb_rec709_display\" / \"lin_rec709_display\" or a vendor-extension-prefixed \
                     identifier",
                    splat.color_space
                )));
            }
            if let Some(proj) = &splat.projection {
                if !spec_or_extension(proj, SPLAT_PROJECTIONS) {
                    return Err(invalid(format!(
                        "ExtensionStackGaussianSplattingProjection: meshes[{mi}].primitives[{pi}] \
                         .extensions.KHR_gaussian_splatting.projection = {:?} is not \
                         \"perspective\" or a vendor-extension-prefixed identifier",
                        proj
                    )));
                }
            }
            if let Some(sort) = &splat.sorting_method {
                if !spec_or_extension(sort, SPLAT_SORTING) {
                    return Err(invalid(format!(
                        "ExtensionStackGaussianSplattingSortingMethod: meshes[{mi}] \
                         .primitives[{pi}].extensions.KHR_gaussian_splatting.sortingMethod = \
                         {:?} is not \"cameraDistance\" or a vendor-extension-prefixed \
                         identifier",
                        sort
                    )));
                }
            }
            // §"Ellipse Kernel" §"Dependencies on glTF" — for the
            // `"ellipse"` kernel the primitive MUST be drawn as POINTS.
            // Default `mode` per spec §3.7.2 is 4 (TRIANGLES); only an
            // explicit 0 satisfies the ellipse-kernel rule.
            if splat.kernel == "ellipse" {
                let mode = prim.mode.unwrap_or(crate::json_model::MODE_TRIANGLES);
                if mode != crate::json_model::MODE_POINTS {
                    return Err(invalid(format!(
                        "ExtensionStackGaussianSplattingMode: meshes[{mi}].primitives[{pi}] \
                         carries KHR_gaussian_splatting with kernel \"ellipse\" but mode = \
                         {mode} (the ellipse kernel requires mode = 0 / POINTS per \
                         §\"Ellipse Kernel\" §\"Dependencies on glTF\")"
                    )));
                }
                // §"Ellipse Kernel" §"Attributes" — the ellipse kernel
                // defines an exact per-attribute storage contract for the
                // splat-field semantics carried on the primitive. Validate
                // the accessor type + component-type + normalized layout of
                // every kernel-defined attribute that is present, the
                // presence of every required attribute, and the
                // spherical-harmonics degree-completeness rule.
                validate_gaussian_splatting_attributes(root, prim, mi, pi)?;
            }
        }
    }

    // KHR_draco_mesh_compression — per-primitive Draco-compressed
    // geometry descriptor per
    // `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md`.
    //
    //   §3.12 — any document carrying the data block on any primitive
    //   MUST declare the extension in `extensionsUsed`.
    //
    //   §"glTF Schema Updates" — per-primitive invariants:
    //     * extension `bufferView` resolves into `buffer_views[]`
    //     * extension `attributes` keys MUST be a subset of the parent
    //       primitive's `attributes` keys ("The `attributes` defined in
    //       the extension must be a subset of the attributes of the
    //       primitive")
    //     * extension `attributes` values (Draco-side attribute IDs)
    //       MUST be unique within one descriptor ("each attribute is
    //       associated with an attribute id which is its unique id in
    //       the compressed data")
    //
    //   §"Restrictions on geometry type" — the primitive's `mode` MUST
    //   be `TRIANGLES` (4) or `TRIANGLE_STRIP` (5) when this extension
    //   is used. Per spec §3.7.2 the default `mode` is 4 (TRIANGLES) —
    //   compliant with the restriction.
    //
    //   §Conformance — when the uncompressed-fallback accessors are
    //   absent the extension MUST appear in `extensionsRequired` ("If
    //   the uncompressed version of the asset is not provided, then
    //   KHR_draco_mesh_compression must be added to extensionsRequired").
    //   Our crate's encoder always emits uncompressed accessors
    //   alongside the descriptor, so the parent primitive's
    //   `attributes` map is always populated; documents that ship the
    //   compressed-only shape land here from external sources. We
    //   detect that shape by checking whether every parent attribute
    //   key the descriptor names corresponds to an accessor index that
    //   actually points at the compressed bufferView (i.e. the
    //   accessor.bufferView == the descriptor's bufferView). When the
    //   parent primitive carries NO uncompressed `attributes` at all
    //   (an empty map) we treat the document as compressed-only and
    //   require the declaration in `extensionsRequired`. The
    //   spec-permitted partial-shape (some attributes uncompressed,
    //   some compressed-only) is not modelled here; it lives in a
    //   layered profile.
    let mut has_draco = false;
    let mut compressed_only = false;
    for (mi, mesh) in root.meshes.iter().enumerate() {
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            let draco = match prim
                .extensions
                .as_ref()
                .and_then(|e| e.khr_draco_mesh_compression.as_ref())
            {
                Some(d) => d,
                None => continue,
            };
            has_draco = true;
            // bufferView index in range.
            let bvi = draco.buffer_view as usize;
            if bvi >= root.buffer_views.len() {
                return Err(invalid(format!(
                    "ExtensionStackDracoBufferView: meshes[{mi}].primitives[{pi}] \
                     .extensions.KHR_draco_mesh_compression.bufferView = {} is out of \
                     range (bufferViews[].len = {})",
                    draco.buffer_view,
                    root.buffer_views.len()
                )));
            }
            // glTF 2.0 §5.11.4 — `bufferView.byteStride`, when defined,
            // is reserved for vertex attribute data layouts ("Buffer
            // views with other types of data MUST NOT define byteStride
            // (unless such layout is explicitly enabled by an
            // extension)"). The Draco descriptor's bufferView carries
            // an opaque compressed payload — neither vertex attribute
            // data nor an indexed array — and `KHR_draco_mesh_compression`
            // does not enable a strided layout for the payload. So
            // `byteStride` MUST NOT be defined on the referenced
            // bufferView. The same reasoning is already enforced for
            // sparse-indices / sparse-values bufferViews by spec
            // §5.3.1 (the sparse-indices and sparse-values checks above
            // in this file).
            let draco_bv = &root.buffer_views[bvi];
            if let Some(stride) = draco_bv.byte_stride {
                return Err(invalid(format!(
                    "ExtensionStackDracoByteStride: meshes[{mi}].primitives[{pi}] \
                     .extensions.KHR_draco_mesh_compression.bufferView = {bvi} \
                     -> bufferViews[{bvi}].byteStride = {stride} — MUST NOT be \
                     defined on a Draco-compressed payload bufferView (glTF 2.0 \
                     §5.11.4: byteStride is reserved for vertex attribute data \
                     layouts; the Draco payload is opaque compressed bytes, not \
                     vertex attributes, and KHR_draco_mesh_compression does not \
                     enable a strided payload layout)"
                )));
            }
            // attributes keys ⊆ parent primitive attributes keys.
            for parent_attr in draco.attributes.keys() {
                if !prim.attributes.contains_key(parent_attr) {
                    return Err(invalid(format!(
                        "ExtensionStackDracoAttributes: meshes[{mi}].primitives[{pi}] \
                         .extensions.KHR_draco_mesh_compression.attributes lists key \
                         {:?} that is not present in the parent primitive's attributes \
                         map (spec §\"attributes\": \"The `attributes` defined in the \
                         extension must be a subset of the attributes of the primitive\")",
                        parent_attr
                    )));
                }
            }
            // attributes values (Draco-side IDs) MUST be unique.
            let mut seen_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
            for (k, &id) in &draco.attributes {
                if !seen_ids.insert(id) {
                    return Err(invalid(format!(
                        "ExtensionStackDracoAttributeId: meshes[{mi}].primitives[{pi}] \
                         .extensions.KHR_draco_mesh_compression.attributes assigns the \
                         same Draco attribute id {id} to more than one parent attribute \
                         (last seen at {:?}); each Draco-side id must be unique per spec \
                         §\"attributes\"",
                        k
                    )));
                }
            }
            // §"Restrictions on geometry type" — mode MUST be TRIANGLES (4)
            // or TRIANGLE_STRIP (5).
            let mode = prim.mode.unwrap_or(crate::json_model::MODE_TRIANGLES);
            if mode != crate::json_model::MODE_TRIANGLES
                && mode != crate::json_model::MODE_TRIANGLE_STRIP
            {
                return Err(invalid(format!(
                    "ExtensionStackDracoMode: meshes[{mi}].primitives[{pi}] carries \
                     KHR_draco_mesh_compression with mode = {mode}; spec \
                     §\"Restrictions on geometry type\" requires mode ∈ {{4 (TRIANGLES), \
                     5 (TRIANGLE_STRIP)}}"
                )));
            }
            // Compressed-only detection (parent primitive has no
            // uncompressed attributes alongside the descriptor) — when
            // this is true on any primitive, the extension MUST also
            // appear in `extensionsRequired` per §Conformance.
            if prim.attributes.is_empty() {
                compressed_only = true;
            }
        }
    }
    if has_draco && !used("KHR_draco_mesh_compression") {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_draco_mesh_compression data is \
             present on a primitive but the extension is not listed in \
             extensionsUsed (spec §3.12 + §Conformance)",
        ));
    }
    if compressed_only
        && !root
            .extensions_required
            .iter()
            .any(|r| r == "KHR_draco_mesh_compression")
    {
        return Err(invalid(
            "ExtensionStackDracoRequired: a primitive carries \
             KHR_draco_mesh_compression with no uncompressed fallback attributes; \
             the extension MUST appear in extensionsRequired (spec §Conformance: \
             \"If the uncompressed version of the asset is not provided, then \
             KHR_draco_mesh_compression must be added to extensionsRequired\")",
        ));
    }

    // KHR_xmp_json_ld — both a root-level `packets[]` roster and
    // per-object packet refs surface this extension. Per spec §3.12
    // any presence of the data block requires the extension to be
    // listed in `extensionsUsed`. See
    // `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Defining XMP
    // Metadata" + §"Instantiating XMP metadata".
    let has_root_xmp = root
        .extensions
        .as_ref()
        .and_then(|e| e.khr_xmp_json_ld.as_ref())
        .is_some();
    let has_asset_xmp = root
        .asset
        .extensions
        .as_ref()
        .and_then(|e| e.khr_xmp_json_ld.as_ref())
        .is_some();
    let has_scene_xmp = root.scenes.iter().any(|s| {
        s.extensions
            .as_ref()
            .and_then(|e| e.khr_xmp_json_ld.as_ref())
            .is_some()
    });
    let has_node_xmp = root.nodes.iter().any(|n| {
        n.extensions
            .as_ref()
            .and_then(|e| e.khr_xmp_json_ld.as_ref())
            .is_some()
    });
    let has_mesh_xmp = root.meshes.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_xmp_json_ld.as_ref())
            .is_some()
    });
    let has_material_xmp_data = root.materials.iter().any(|m| {
        m.extensions
            .as_ref()
            .and_then(|e| e.khr_xmp_json_ld.as_ref())
            .is_some()
    });
    if (has_root_xmp
        || has_asset_xmp
        || has_scene_xmp
        || has_node_xmp
        || has_mesh_xmp
        || has_material_xmp_data)
        && !used("KHR_xmp_json_ld")
    {
        return Err(invalid(
            "ExtensionStackUsedNotDeclared: KHR_xmp_json_ld data is \
             present but the extension is not listed in extensionsUsed \
             (spec §3.12)",
        ));
    }
    // Value-range check: every per-object `packet` reference MUST
    // resolve to a slot in `root.extensions.KHR_xmp_json_ld.packets[]`
    // per the spec's indirection model. See
    // `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating
    // XMP metadata".
    let packet_count = root
        .extensions
        .as_ref()
        .and_then(|e| e.khr_xmp_json_ld.as_ref())
        .map(|r| r.packets.len())
        .unwrap_or(0);
    let mut refs: Vec<(String, u32)> = Vec::new();
    if let Some(aext) = &root.asset.extensions {
        if let Some(x) = &aext.khr_xmp_json_ld {
            refs.push(("asset".to_owned(), x.packet));
        }
    }
    for (i, s) in root.scenes.iter().enumerate() {
        if let Some(sext) = &s.extensions {
            if let Some(x) = &sext.khr_xmp_json_ld {
                refs.push((format!("scenes[{i}]"), x.packet));
            }
        }
    }
    for (i, n) in root.nodes.iter().enumerate() {
        if let Some(next) = &n.extensions {
            if let Some(x) = &next.khr_xmp_json_ld {
                refs.push((format!("nodes[{i}]"), x.packet));
            }
        }
    }
    for (i, mh) in root.meshes.iter().enumerate() {
        if let Some(mext) = &mh.extensions {
            if let Some(x) = &mext.khr_xmp_json_ld {
                refs.push((format!("meshes[{i}]"), x.packet));
            }
        }
    }
    for (i, mt) in root.materials.iter().enumerate() {
        if let Some(mext) = &mt.extensions {
            if let Some(x) = &mext.khr_xmp_json_ld {
                refs.push((format!("materials[{i}]"), x.packet));
            }
        }
    }
    for (scope, packet) in refs {
        if (packet as usize) >= packet_count {
            return Err(invalid(format!(
                "ExtensionStackXmpPacketIndex: {scope}.extensions.KHR_xmp_json_ld.packet = \
                 {packet} out of range (have {packet_count} packets)"
            )));
        }
    }
    // Value-range checks for KHR_materials_variants per
    // `docs/3d/gltf/extensions/KHR_materials_variants.md`:
    //
    // * Each variant index in a primitive mapping MUST resolve to a
    //   slot in the root-level `variants[]` array
    //   (`ExtensionStackVariantsIndex`).
    // * Each material index in a primitive mapping MUST resolve to a
    //   slot in the root-level `materials[]` array
    //   (`ExtensionStackVariantsMaterialIndex`).
    // * Across all mappings on a single primitive, each variant index
    //   MUST appear no more than once
    //   (`ExtensionStackVariantsDuplicate`) — quoting the spec, "Across
    //   the entire mappings array, each variant index must be used no
    //   more than one time."
    let variant_count = root
        .extensions
        .as_ref()
        .and_then(|e| e.khr_materials_variants.as_ref())
        .map(|r| r.variants.len())
        .unwrap_or(0);
    let material_count = root.materials.len();
    for (mi, mesh) in root.meshes.iter().enumerate() {
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            let vmap = match prim
                .extensions
                .as_ref()
                .and_then(|e| e.khr_materials_variants.as_ref())
            {
                Some(v) => v,
                None => continue,
            };
            let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
            for (li, line) in vmap.mappings.iter().enumerate() {
                if (line.material as usize) >= material_count {
                    return Err(invalid(format!(
                        "ExtensionStackVariantsMaterialIndex: meshes[{mi}].primitives[{pi}]\
                         .extensions.KHR_materials_variants.mappings[{li}].material = {} out \
                         of range (have {} materials)",
                        line.material, material_count
                    )));
                }
                for &v in &line.variants {
                    if (v as usize) >= variant_count {
                        return Err(invalid(format!(
                            "ExtensionStackVariantsIndex: meshes[{mi}].primitives[{pi}]\
                             .extensions.KHR_materials_variants.mappings[{li}].variants \
                             contains {v} which is out of range (have {variant_count} variants)"
                        )));
                    }
                    if !seen.insert(v) {
                        return Err(invalid(format!(
                            "ExtensionStackVariantsDuplicate: meshes[{mi}].primitives[{pi}]\
                             .extensions.KHR_materials_variants.mappings reuses variant \
                             index {v} across multiple entries (spec — \"each variant index \
                             must be used no more than one time\")"
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Collect every `KHR_texture_transform` block carried by any
/// `textureInfo` of one material — the five core PBR slots AND every
/// textureInfo nested inside a material extension
/// (`KHR_materials_specular.specularTexture`,
/// `KHR_materials_clearcoat.clearcoatNormalTexture`, …). Each yielded
/// entry pairs a human-readable slot label (for diagnostics) with a
/// reference to the parsed transform.
///
/// Per `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
/// Updates the extension "may be defined on `textureInfo` structures" —
/// **any** textureInfo, not just the core PBR ones — so the §3.12
/// extension-stack rule and the finite-value checks below must reach
/// the material-extension texture slots too.
pub(crate) fn material_texture_transforms(
    m: &crate::json_model::Material,
) -> Vec<(String, &crate::json_model::TextureTransform)> {
    use crate::json_model::{NormalTextureInfo, TextureInfo, TextureTransform};

    // A `textureInfo` and a `normalTextureInfo` carry the same
    // `extensions` block; these free helpers reach the optional
    // `KHR_texture_transform` on either and label it with its slot.
    fn ti<'a>(slot: &str, info: Option<&'a TextureInfo>) -> Option<(String, &'a TextureTransform)> {
        info.and_then(|i| i.extensions.as_ref())
            .and_then(|e| e.khr_texture_transform.as_ref())
            .map(|t| (slot.to_owned(), t))
    }
    fn nti<'a>(
        slot: &str,
        info: Option<&'a NormalTextureInfo>,
    ) -> Option<(String, &'a TextureTransform)> {
        info.and_then(|i| i.extensions.as_ref())
            .and_then(|e| e.khr_texture_transform.as_ref())
            .map(|t| (slot.to_owned(), t))
    }

    let mut out: Vec<(String, &TextureTransform)> = Vec::new();

    // Core PBR slots (§3.9 material).
    if let Some(p) = m.pbr_metallic_roughness.as_ref() {
        out.extend(ti("baseColorTexture", p.base_color_texture.as_ref()));
        out.extend(ti(
            "metallicRoughnessTexture",
            p.metallic_roughness_texture.as_ref(),
        ));
    }
    out.extend(nti("normalTexture", m.normal_texture.as_ref()));
    if let Some(t) = m
        .occlusion_texture
        .as_ref()
        .and_then(|t| t.extensions.as_ref())
        .and_then(|e| e.khr_texture_transform.as_ref())
    {
        out.push(("occlusionTexture".to_owned(), t));
    }
    out.extend(ti("emissiveTexture", m.emissive_texture.as_ref()));

    // Material-extension texture slots — per the spec the transform may
    // ride any of these textureInfos.
    if let Some(ext) = m.extensions.as_ref() {
        if let Some(s) = ext.khr_materials_specular.as_ref() {
            out.extend(ti(
                "KHR_materials_specular.specularTexture",
                s.specular_texture.as_ref(),
            ));
            out.extend(ti(
                "KHR_materials_specular.specularColorTexture",
                s.specular_color_texture.as_ref(),
            ));
        }
        if let Some(c) = ext.khr_materials_clearcoat.as_ref() {
            out.extend(ti(
                "KHR_materials_clearcoat.clearcoatTexture",
                c.clearcoat_texture.as_ref(),
            ));
            out.extend(ti(
                "KHR_materials_clearcoat.clearcoatRoughnessTexture",
                c.clearcoat_roughness_texture.as_ref(),
            ));
            out.extend(nti(
                "KHR_materials_clearcoat.clearcoatNormalTexture",
                c.clearcoat_normal_texture.as_ref(),
            ));
        }
        if let Some(s) = ext.khr_materials_sheen.as_ref() {
            out.extend(ti(
                "KHR_materials_sheen.sheenColorTexture",
                s.sheen_color_texture.as_ref(),
            ));
            out.extend(ti(
                "KHR_materials_sheen.sheenRoughnessTexture",
                s.sheen_roughness_texture.as_ref(),
            ));
        }
        if let Some(t) = ext.khr_materials_transmission.as_ref() {
            out.extend(ti(
                "KHR_materials_transmission.transmissionTexture",
                t.transmission_texture.as_ref(),
            ));
        }
        if let Some(v) = ext.khr_materials_volume.as_ref() {
            out.extend(ti(
                "KHR_materials_volume.thicknessTexture",
                v.thickness_texture.as_ref(),
            ));
        }
        if let Some(i) = ext.khr_materials_iridescence.as_ref() {
            out.extend(ti(
                "KHR_materials_iridescence.iridescenceTexture",
                i.iridescence_texture.as_ref(),
            ));
            out.extend(ti(
                "KHR_materials_iridescence.iridescenceThicknessTexture",
                i.iridescence_thickness_texture.as_ref(),
            ));
        }
        if let Some(a) = ext.khr_materials_anisotropy.as_ref() {
            out.extend(ti(
                "KHR_materials_anisotropy.anisotropyTexture",
                a.anisotropy_texture.as_ref(),
            ));
        }
        if let Some(d) = ext.khr_materials_diffuse_transmission.as_ref() {
            out.extend(ti(
                "KHR_materials_diffuse_transmission.diffuseTransmissionTexture",
                d.diffuse_transmission_texture.as_ref(),
            ));
            out.extend(ti(
                "KHR_materials_diffuse_transmission.diffuseTransmissionColorTexture",
                d.diffuse_transmission_color_texture.as_ref(),
            ));
        }
    }

    out
}

/// Spec validation for one `KHR_texture_transform` block. The schema
/// fields (`offset` / `scale` as `array[2]`, `texCoord` as a
/// non-negative integer) are already pinned by their typed
/// representation, so the only runtime-checkable MUST left is
/// finiteness: a NaN / ±∞ `rotation`, `offset`, or `scale` would make
/// the affine UV `mat3` (§Overview) non-finite, mapping every sampled
/// texel to an undefined coordinate. Reject those.
fn validate_texture_transform(
    mat_idx: usize,
    slot: &str,
    t: &crate::json_model::TextureTransform,
) -> Result<()> {
    if let Some(r) = t.rotation {
        if !r.is_finite() {
            return Err(invalid(format!(
                "ExtensionStackTextureTransformRotationFinite: materials[{mat_idx}] \
                 {slot} KHR_texture_transform.rotation = {r} is not finite \
                 (spec §Overview affine UV transform)"
            )));
        }
    }
    if let Some(o) = t.offset {
        if !o[0].is_finite() || !o[1].is_finite() {
            return Err(invalid(format!(
                "ExtensionStackTextureTransformOffsetFinite: materials[{mat_idx}] \
                 {slot} KHR_texture_transform.offset = {o:?} has a non-finite \
                 component (spec §Overview affine UV transform)"
            )));
        }
    }
    if let Some(s) = t.scale {
        if !s[0].is_finite() || !s[1].is_finite() {
            return Err(invalid(format!(
                "ExtensionStackTextureTransformScaleFinite: materials[{mat_idx}] \
                 {slot} KHR_texture_transform.scale = {s:?} has a non-finite \
                 component (spec §Overview affine UV transform)"
            )));
        }
    }
    Ok(())
}

/// Spec §3.11: every animation channel must point at a known
/// `target.path` (`"translation"` / `"rotation"` / `"scale"` /
/// `"weights"`); each channel's `sampler` index must be in range; each
/// sampler's input/output accessor indices must be in range; and
/// `"weights"` channels MUST target a node whose `mesh` declares at
/// least one morph target.
pub fn validate_animation_channels(
    anim_idx: usize,
    anim: &Animation,
    nodes: &[crate::json_model::Node],
    meshes: &[Mesh],
    accessors: &[Accessor],
) -> Result<()> {
    for (ci, ch) in anim.channels.iter().enumerate() {
        // sampler index in range
        let sampler = anim.samplers.get(ch.sampler as usize).ok_or_else(|| {
            invalid(format!(
                "AnimationChannelSampler: animations[{anim_idx}].channels[{ci}].sampler = \
                 {} out of range (have {} samplers, spec §3.11)",
                ch.sampler,
                anim.samplers.len()
            ))
        })?;
        // input / output accessor indices in range
        if accessors.get(sampler.input as usize).is_none() {
            return Err(invalid(format!(
                "AnimationChannelSamplerInput: animations[{anim_idx}].samplers[{}] \
                 .input = {} out of range (have {} accessors, spec §3.11)",
                ch.sampler,
                sampler.input,
                accessors.len()
            )));
        }
        if accessors.get(sampler.output as usize).is_none() {
            return Err(invalid(format!(
                "AnimationChannelSamplerOutput: animations[{anim_idx}].samplers[{}] \
                 .output = {} out of range (have {} accessors, spec §3.11)",
                ch.sampler,
                sampler.output,
                accessors.len()
            )));
        }

        // path is one of the four base-spec strings (§3.11) or the
        // `"pointer"` sentinel introduced by KHR_animation_pointer
        // (see `docs/3d/gltf/extensions/KHR_animation_pointer.md`
        // §"Extension Usage"). The pointer case is checked in detail
        // by `validate_extension_stack`.
        match ch.target.path.as_str() {
            "translation" | "rotation" | "scale" | "weights" | "pointer" => {}
            other => {
                return Err(invalid(format!(
                    "AnimationChannelPath: animations[{anim_idx}].channels[{ci}].target.path \
                     = {other:?} — must be one of \"translation\" / \"rotation\" / \
                     \"scale\" / \"weights\" / \"pointer\" (spec §3.11 + KHR_animation_pointer)"
                )));
            }
        }

        // weights channels require the target node to have a mesh
        // declaring morph targets.
        if ch.target.path == "weights" {
            let Some(target_node_idx) = ch.target.node else {
                // §3.11 — a channel with no node is ignored at decode
                // time; nothing to validate here.
                continue;
            };
            let node = nodes.get(target_node_idx as usize).ok_or_else(|| {
                invalid(format!(
                    "AnimationChannelTarget: animations[{anim_idx}].channels[{ci}] \
                     .target.node = {target_node_idx} out of range (have {} nodes, \
                     spec §3.11)",
                    nodes.len()
                ))
            })?;
            let mesh_idx = node.mesh.ok_or_else(|| {
                invalid(format!(
                    "AnimationChannelWeightsNoMesh: animations[{anim_idx}].channels[{ci}] \
                     targets node {target_node_idx} with path=\"weights\" but the node \
                     has no mesh (spec §3.11: morph-weight channels MUST point at a node \
                     bound to a mesh)"
                ))
            })?;
            let mesh = meshes.get(mesh_idx as usize).ok_or_else(|| {
                invalid(format!(
                    "AnimationChannelWeightsMeshIdx: animations[{anim_idx}].channels[{ci}] \
                     -> node {target_node_idx} -> mesh {mesh_idx} out of range \
                     (have {} meshes)",
                    meshes.len()
                ))
            })?;
            // Spec §3.7.2.2: all primitives in a mesh have the same
            // number of targets, so checking the first is enough; an
            // empty `primitives` array is rejected by §3.7.1 elsewhere.
            let target_count = mesh
                .primitives
                .first()
                .map(|p| p.targets.len())
                .unwrap_or(0);
            if target_count == 0 {
                return Err(invalid(format!(
                    "AnimationChannelWeightsNoTargets: animations[{anim_idx}].channels[{ci}] \
                     -> node {target_node_idx} -> mesh {mesh_idx} has no morph targets \
                     (spec §3.11: a \"weights\" channel requires the mesh to declare \
                     primitive.targets)"
                )));
            }
        }
    }
    Ok(())
}

/// Highest 2.x edition of the glTF spec this decoder implements.
///
/// `asset.version` carries the *target* version of the asset (the
/// version the asset author wrote against); `asset.minVersion`, when
/// present, carries the *minimum* version a client implementation MUST
/// support to load the asset (spec §3.2). Both are compared against
/// this constant so we reject 2.1 / 2.5 / 3.0 assets even when the
/// schema pattern matches.
const MAX_SUPPORTED_MAJOR: u32 = 2;
const MAX_SUPPORTED_MINOR: u32 = 0;

/// Parse a glTF `<major>.<minor>` version string into `(major, minor)`.
///
/// Returns `Err` when the input does not match the JSON-schema pattern
/// `^[0-9]+\.[0-9]+$` (one or more ASCII digits, a single dot, one or
/// more ASCII digits). The error message is opaque so the caller can
/// wrap it with a per-field prefix (`AssetVersionFormat` vs
/// `AssetMinVersionFormat`).
fn parse_version(s: &str) -> std::result::Result<(u32, u32), &'static str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    // major: one or more ASCII digits
    let major_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == major_start {
        return Err("missing major");
    }
    if i >= bytes.len() || bytes[i] != b'.' {
        return Err("missing dot");
    }
    let dot = i;
    i += 1;
    let minor_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == minor_start {
        return Err("missing minor");
    }
    if i != bytes.len() {
        return Err("trailing characters");
    }
    let major: u32 = s[..dot].parse().map_err(|_| "major overflow")?;
    let minor: u32 = s[dot + 1..].parse().map_err(|_| "minor overflow")?;
    Ok((major, minor))
}

/// Spec §3.2 + §5.9.3 / §5.9.4: validate `asset.version` and
/// `asset.minVersion` against the schema pattern and the highest
/// version this decoder implements.
///
/// Stable error prefixes (all `Error::InvalidData`):
///
/// * `AssetVersionFormat` — `asset.version` does not match
///   `<major>.<minor>`.
/// * `AssetVersionUnsupported` — `asset.version` major.minor exceeds
///   the highest 2.x edition we implement.
/// * `AssetMinVersionFormat` — `asset.minVersion` does not match
///   `<major>.<minor>`.
/// * `AssetMinVersionGreaterThanVersion` — `asset.minVersion >
///   asset.version` (spec MUST: §5.9.4).
/// * `AssetMinVersionUnsupported` — `asset.minVersion` exceeds the
///   highest edition this decoder can guarantee.
///
/// `asset.version` major is checked rather than compared exact-string
/// against `"2.0"`: a forward-compatible 2.1 asset that only uses 2.0
/// features still loads, matching the spec's own guidance in §3.2
/// ("clients should check the version property and ensure the major
/// version is supported").
pub fn check_asset_version(asset: &crate::json_model::Asset) -> Result<()> {
    let (av_major, av_minor) = parse_version(&asset.version).map_err(|why| {
        invalid(format!(
            "AssetVersionFormat: asset.version = {:?} does not match \
             <major>.<minor> ({why}, spec §5.9.3)",
            asset.version
        ))
    })?;
    if av_major != MAX_SUPPORTED_MAJOR {
        return Err(invalid(format!(
            "AssetVersionUnsupported: asset.version = {:?} — only major \
             {MAX_SUPPORTED_MAJOR} (glTF 2.x) is supported (spec §5.9.3)",
            asset.version
        )));
    }
    // 2.x is forward-compatible *enough* that we accept the minor freely
    // for asset.version. minVersion is where the hard upper-bound lives.

    if let Some(min_v) = asset.min_version.as_ref() {
        let (mv_major, mv_minor) = parse_version(min_v).map_err(|why| {
            invalid(format!(
                "AssetMinVersionFormat: asset.minVersion = {min_v:?} does not match \
                 <major>.<minor> ({why}, spec §5.9.4)"
            ))
        })?;
        // Spec §5.9.4 (MUST): minVersion <= version.
        if (mv_major, mv_minor) > (av_major, av_minor) {
            return Err(invalid(format!(
                "AssetMinVersionGreaterThanVersion: asset.minVersion = {min_v:?} > \
                 asset.version = {:?} (spec §5.9.4: minVersion MUST NOT be greater \
                 than version)",
                asset.version
            )));
        }
        // This decoder only implements up to 2.0; reject anything beyond.
        if (mv_major, mv_minor) > (MAX_SUPPORTED_MAJOR, MAX_SUPPORTED_MINOR) {
            return Err(invalid(format!(
                "AssetMinVersionUnsupported: asset.minVersion = {min_v:?} exceeds \
                 the highest supported edition {MAX_SUPPORTED_MAJOR}.{MAX_SUPPORTED_MINOR} \
                 (spec §3.2: clients SHOULD NOT load assets whose minVersion they \
                 cannot guarantee)"
            )));
        }
    }
    Ok(())
}

/// Spec §3.6.2.4 + §5.1: every accessor MUST fit inside the bufferView
/// that backs it. The fit expression from line 3104 of the 2.0 spec is
///
/// ```text
/// accessor.byteOffset
///     + EFFECTIVE_BYTE_STRIDE * (accessor.count - 1)
///     + SIZE_OF_COMPONENT * NUMBER_OF_COMPONENTS
///     <= bufferView.byteLength
/// ```
///
/// where `EFFECTIVE_BYTE_STRIDE` is `bufferView.byteStride` when defined,
/// else `SIZE_OF_COMPONENT * NUMBER_OF_COMPONENTS` (tightly packed).
///
/// The bufferView's own `byteOffset` cancels out of this check because
/// the bound is against `bufferView.byteLength`; bufferView fit inside
/// its backing buffer is the separate `validate_bufferview_fits_buffer`
/// check.
///
/// Accessors with no `bufferView` (pure-sparse base-zero accessors) are
/// skipped — the spec only requires fit when a bufferView is referenced.
/// Accessor `count == 0` is also skipped (no data to fit).
pub fn validate_accessor_fits_bufferview(
    accessor_idx: usize,
    accessor: &Accessor,
    buffer_views: &[BufferView],
) -> Result<()> {
    let Some(bv_idx) = accessor.buffer_view else {
        return Ok(());
    };
    if accessor.count == 0 {
        return Ok(());
    }
    let bv = buffer_views.get(bv_idx as usize).ok_or_else(|| {
        invalid(format!(
            "AccessorFitBufferView: accessors[{accessor_idx}].bufferView = {bv_idx} \
             out of range (have {} buffer views, spec §3.6.2.4)",
            buffer_views.len()
        ))
    })?;
    let csize = component_size(accessor.component_type).ok_or_else(|| {
        invalid(format!(
            "AccessorFitComponentType: accessors[{accessor_idx}].componentType = {} \
             unknown (spec §3.6.2.2 enumerates 5120/5121/5122/5123/5125/5126)",
            accessor.component_type
        ))
    })?;
    let components = type_components(&accessor.kind).ok_or_else(|| {
        invalid(format!(
            "AccessorFitElementType: accessors[{accessor_idx}].type = {:?} \
             unknown (spec §3.6.2.2 enumerates SCALAR/VEC2/VEC3/VEC4/MAT2/MAT3/MAT4)",
            accessor.kind
        ))
    })?;
    let element_size = (csize as u64) * (components as u64);
    let effective_stride: u64 = bv.byte_stride.map(u64::from).unwrap_or(element_size);
    if effective_stride < element_size {
        return Err(invalid(format!(
            "AccessorFitStride: accessors[{accessor_idx}] -> bufferViews[{bv_idx}] \
             byteStride {effective_stride} < element size {element_size} \
             (spec §3.6.2.4: stride MUST fit the element)"
        )));
    }
    let acc_off = accessor.byte_offset.unwrap_or(0) as u64;
    let last_element_start = effective_stride
        .checked_mul(accessor.count as u64 - 1)
        .and_then(|v| v.checked_add(acc_off))
        .ok_or_else(|| {
            invalid(format!(
                "AccessorFitOverflow: accessors[{accessor_idx}].byteOffset + stride * (count-1) \
                 overflowed u64 (spec §3.6.2.4)"
            ))
        })?;
    let end = last_element_start
        .checked_add(element_size)
        .ok_or_else(|| {
            invalid(format!(
                "AccessorFitOverflow: accessors[{accessor_idx}] element-end offset overflowed u64 \
             (spec §3.6.2.4)"
            ))
        })?;
    let bv_len = bv.byte_length as u64;
    if end > bv_len {
        return Err(invalid(format!(
            "AccessorFitBufferView: accessors[{accessor_idx}] needs {end} bytes inside \
             bufferViews[{bv_idx}] (byteLength {bv_len}) — \
             byteOffset {acc_off} + stride {effective_stride} * (count {} - 1) + \
             elementSize {element_size} (spec §3.6.2.4 line 3104)",
            accessor.count
        )));
    }
    Ok(())
}

/// Spec §5.11: every bufferView MUST fit inside the buffer it points
/// into. The check is
///
/// ```text
/// bufferView.byteOffset + bufferView.byteLength <= buffer.byteLength
/// ```
///
/// plus the JSON-schema range `[4, 252]` for `byteStride` from §5.11.4
/// (the schema also limits stride to `[4, 252]`; values outside that
/// window are violations even when no accessor references them).
pub fn validate_bufferview_fits_buffer(
    bv_idx: usize,
    bv: &BufferView,
    buffers: &[Buffer],
) -> Result<()> {
    let buf = buffers.get(bv.buffer as usize).ok_or_else(|| {
        invalid(format!(
            "BufferViewFitBuffer: bufferViews[{bv_idx}].buffer = {} out of range \
             (have {} buffers, spec §5.11.1)",
            bv.buffer,
            buffers.len()
        ))
    })?;
    let off = bv.byte_offset.unwrap_or(0) as u64;
    let len = bv.byte_length as u64;
    let end = off.checked_add(len).ok_or_else(|| {
        invalid(format!(
            "BufferViewFitOverflow: bufferViews[{bv_idx}].byteOffset + byteLength overflowed u64 \
             (spec §5.11)"
        ))
    })?;
    let buf_len = buf.byte_length as u64;
    if end > buf_len {
        return Err(invalid(format!(
            "BufferViewFitBuffer: bufferViews[{bv_idx}] spans bytes [{off}, {end}) \
             but buffers[{}] is only {buf_len} bytes long (spec §5.11)",
            bv.buffer
        )));
    }
    if let Some(stride) = bv.byte_stride {
        if !(4..=252).contains(&stride) {
            return Err(invalid(format!(
                "BufferViewStrideRange: bufferViews[{bv_idx}].byteStride = {stride} \
                 outside JSON-schema range [4, 252] (spec §5.11.4)"
            )));
        }
    }
    Ok(())
}

/// Spec §5.3.1: an `accessor.sparse.indices.bufferView` MUST NOT carry a
/// `target` or `byteStride` property. The sparse-indices array is always
/// tightly packed and not a vertex-attribute / element-array buffer view
/// in the GPU-pipeline sense, so any such hint is a spec violation.
///
/// We walk every accessor's sparse block (when present) and check the
/// referenced bufferView. Out-of-range `bufferView` indices are reported
/// with `SparseIndicesBufferViewIndex`; the byteOffset alignment rule
/// from the same paragraph reuses `validate_alignment` upstream.
pub fn validate_sparse_indices_buffer_views(
    accessors: &[Accessor],
    buffer_views: &[BufferView],
) -> Result<()> {
    for (ai, acc) in accessors.iter().enumerate() {
        let Some(sparse) = acc.sparse.as_ref() else {
            continue;
        };
        let bv_idx = sparse.indices.buffer_view as usize;
        let bv = buffer_views.get(bv_idx).ok_or_else(|| {
            invalid(format!(
                "SparseIndicesBufferViewIndex: accessors[{ai}].sparse.indices.bufferView \
                 = {bv_idx} out of range (have {} buffer views, spec §5.3.1)",
                buffer_views.len()
            ))
        })?;
        if bv.target.is_some() {
            return Err(invalid(format!(
                "SparseIndicesBufferViewTarget: accessors[{ai}].sparse.indices.bufferView \
                 -> bufferViews[{bv_idx}].target = {:?} — MUST NOT be defined (spec §5.3.1)",
                bv.target
            )));
        }
        if bv.byte_stride.is_some() {
            return Err(invalid(format!(
                "SparseIndicesBufferViewStride: accessors[{ai}].sparse.indices.bufferView \
                 -> bufferViews[{bv_idx}].byteStride = {:?} — MUST NOT be defined (spec §5.3.1)",
                bv.byte_stride
            )));
        }
    }
    Ok(())
}

/// Spec §5.4.1: an `accessor.sparse.values.bufferView` MUST NOT carry a
/// `target` or `byteStride` property — symmetric to the §5.3.1 rule on
/// `sparse.indices.bufferView`. The §5.4 paragraph states "The elements
/// are tightly packed", so a strided layout on the values bufferView is
/// a spec violation; the bufferView is also not a vertex-attribute /
/// element-array buffer in the GPU-pipeline sense, so a `target` hint is
/// equally nonsensical.
///
/// We walk every accessor's sparse block (when present) and check the
/// referenced bufferView. Out-of-range `bufferView` indices are reported
/// with `SparseValuesBufferViewIndex`; the alignment rule "Data MUST be
/// aligned following the same rules as the base accessor" is enforced
/// upstream by `validate_alignment` against the base accessor's
/// component type.
pub fn validate_sparse_values_buffer_views(
    accessors: &[Accessor],
    buffer_views: &[BufferView],
) -> Result<()> {
    for (ai, acc) in accessors.iter().enumerate() {
        let Some(sparse) = acc.sparse.as_ref() else {
            continue;
        };
        let bv_idx = sparse.values.buffer_view as usize;
        let bv = buffer_views.get(bv_idx).ok_or_else(|| {
            invalid(format!(
                "SparseValuesBufferViewIndex: accessors[{ai}].sparse.values.bufferView \
                 = {bv_idx} out of range (have {} buffer views, spec §5.4.1)",
                buffer_views.len()
            ))
        })?;
        if bv.target.is_some() {
            return Err(invalid(format!(
                "SparseValuesBufferViewTarget: accessors[{ai}].sparse.values.bufferView \
                 -> bufferViews[{bv_idx}].target = {:?} — MUST NOT be defined (spec §5.4.1)",
                bv.target
            )));
        }
        if bv.byte_stride.is_some() {
            return Err(invalid(format!(
                "SparseValuesBufferViewStride: accessors[{ai}].sparse.values.bufferView \
                 -> bufferViews[{bv_idx}].byteStride = {:?} — MUST NOT be defined (spec §5.4.1)",
                bv.byte_stride
            )));
        }
    }
    Ok(())
}

/// Validate every `cameras[i]` entry per spec §5.12 + §5.13 + §5.14.
///
/// MUST-level rules enforced (SHOULDs — negative `xmag` / `ymag`,
/// `yfov >= π` — are deliberately allowed through):
///
/// * §5.12 — `camera.perspective` MUST NOT be defined when
///   `camera.orthographic` is defined, and vice versa
///   (`CameraProjectionExclusive`).
/// * §5.13.1 / §5.13.2 — `orthographic.xmag` / `orthographic.ymag`
///   MUST NOT be zero (`CameraOrthographicXmag` /
///   `CameraOrthographicYmag`).
/// * §5.13.3 — `orthographic.zfar` MUST NOT be zero and its JSON
///   schema minimum is `> 0` (`CameraOrthographicZfar`); it MUST be
///   greater than `znear` (`CameraOrthographicZRange`).
/// * §5.13.4 — `orthographic.znear` schema minimum is `>= 0`
///   (`CameraOrthographicZnear`).
/// * §5.14.1 — `perspective.aspectRatio`, when defined, MUST be `> 0`
///   (`CameraPerspectiveAspectRatio`).
/// * §5.14.2 — `perspective.yfov` MUST be `> 0`
///   (`CameraPerspectiveYfov`).
/// * §5.14.3 — `perspective.zfar`, when defined, MUST be `> 0`
///   (`CameraPerspectiveZfar`) and MUST be greater than `znear`
///   (`CameraPerspectiveZRange`); an undefined `zfar` means an
///   infinite projection and is valid.
/// * §5.14.4 — `perspective.znear` MUST be `> 0`
///   (`CameraPerspectiveZnear`).
///
/// Non-finite values (NaN / ±∞) are rejected by the same rules — a NaN
/// `znear` would otherwise slip through every comparison.
pub fn validate_cameras(cameras: &[Camera]) -> Result<()> {
    for (ci, cam) in cameras.iter().enumerate() {
        if cam.perspective.is_some() && cam.orthographic.is_some() {
            return Err(invalid(format!(
                "CameraProjectionExclusive: cameras[{ci}] defines BOTH perspective and \
                 orthographic — each MUST NOT be defined when the other is (spec §5.12)"
            )));
        }
        if let Some(o) = &cam.orthographic {
            if !o.xmag.is_finite() || o.xmag == 0.0 {
                return Err(invalid(format!(
                    "CameraOrthographicXmag: cameras[{ci}].orthographic.xmag = {} \
                     — MUST be finite and MUST NOT be zero (spec §5.13.1)",
                    o.xmag
                )));
            }
            if !o.ymag.is_finite() || o.ymag == 0.0 {
                return Err(invalid(format!(
                    "CameraOrthographicYmag: cameras[{ci}].orthographic.ymag = {} \
                     — MUST be finite and MUST NOT be zero (spec §5.13.2)",
                    o.ymag
                )));
            }
            if !o.znear.is_finite() || o.znear < 0.0 {
                return Err(invalid(format!(
                    "CameraOrthographicZnear: cameras[{ci}].orthographic.znear = {} \
                     — MUST be finite and >= 0 (spec §5.13.4)",
                    o.znear
                )));
            }
            if !o.zfar.is_finite() || o.zfar <= 0.0 {
                return Err(invalid(format!(
                    "CameraOrthographicZfar: cameras[{ci}].orthographic.zfar = {} \
                     — MUST be finite and > 0 (spec §5.13.3)",
                    o.zfar
                )));
            }
            if o.zfar <= o.znear {
                return Err(invalid(format!(
                    "CameraOrthographicZRange: cameras[{ci}].orthographic.zfar = {} \
                     MUST be greater than znear = {} (spec §5.13.3)",
                    o.zfar, o.znear
                )));
            }
        }
        if let Some(p) = &cam.perspective {
            if !p.yfov.is_finite() || p.yfov <= 0.0 {
                return Err(invalid(format!(
                    "CameraPerspectiveYfov: cameras[{ci}].perspective.yfov = {} \
                     — MUST be finite and > 0 (spec §5.14.2)",
                    p.yfov
                )));
            }
            if !p.znear.is_finite() || p.znear <= 0.0 {
                return Err(invalid(format!(
                    "CameraPerspectiveZnear: cameras[{ci}].perspective.znear = {} \
                     — MUST be finite and > 0 (spec §5.14.4)",
                    p.znear
                )));
            }
            if let Some(ar) = p.aspect_ratio {
                if !ar.is_finite() || ar <= 0.0 {
                    return Err(invalid(format!(
                        "CameraPerspectiveAspectRatio: cameras[{ci}].perspective.aspectRatio \
                         = {ar} — when defined, MUST be finite and > 0 (spec §5.14.1)"
                    )));
                }
            }
            if let Some(zfar) = p.zfar {
                if !zfar.is_finite() || zfar <= 0.0 {
                    return Err(invalid(format!(
                        "CameraPerspectiveZfar: cameras[{ci}].perspective.zfar = {zfar} \
                         — when defined, MUST be finite and > 0 (spec §5.14.3)"
                    )));
                }
                if zfar <= p.znear {
                    return Err(invalid(format!(
                        "CameraPerspectiveZRange: cameras[{ci}].perspective.zfar = {zfar} \
                         MUST be greater than znear = {} (spec §5.14.3)",
                        p.znear
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Validate core accessor properties against the glTF 2.0 spec §3.6.2
/// (Accessor Data) + §5.1 (Accessor) — the document-level MUSTs that
/// apply to every `accessors[i]` entry independent of which bufferView
/// it references (the bufferView-fit / sparse-bufferView restrictions
/// already live in `validate_accessor_fits_bufferview` /
/// `validate_sparse_*_buffer_views`).
///
/// Three hard rules are policed, each with a stable `Accessor…` error
/// prefix so callers can grep the specific sub-rule:
///
/// * **§5.1 (`accessor.count`, "Minimum: >= 1")** — `count` MUST be at
///   least 1 (`AccessorCount`). A zero-element accessor is meaningless
///   and the JSON schema pins the minimum at 1.
/// * **§5.1.6 / §3.6.2.1 (`accessor.normalized`)** — "This property
///   MUST NOT be set to `true` for accessors with `FLOAT` or
///   `UNSIGNED_INT` component type" (`AccessorNormalizedComponentType`).
///   Normalization is the integer→[0,1]/[-1,1] decode, which is
///   undefined for a float (already real-valued) and for a 32-bit
///   unsigned int (no §3.6.2.2 dequantisation row exists for it).
/// * **§3.6.2.5 (Accessor Bounds)** — "The length of these arrays MUST
///   be equal to the number of accessor's components." Both `min` and
///   `max`, when present, MUST carry exactly `type_components(type)`
///   entries (`AccessorMinMaxLength`). The length set is therefore one
///   of 1/2/3/4/9/16, matching the `type` value.
///
/// `componentType` / `type` enum-membership itself is checked lazily by
/// the bufferView-fit pass (`AccessorFitComponentType` /
/// `AccessorFitElementType`); here we resolve `type` only to obtain the
/// component count for the bounds-length rule and skip the bounds check
/// when the `type` string is unknown (the fit pass surfaces that error).
pub fn validate_accessors(accessors: &[Accessor]) -> Result<()> {
    for (ai, acc) in accessors.iter().enumerate() {
        // §5.1 — count Minimum: >= 1.
        if acc.count == 0 {
            return Err(invalid(format!(
                "AccessorCount: accessors[{ai}].count = 0 — MUST be >= 1 \
                 (spec §5.1, JSON schema Minimum: >= 1)"
            )));
        }

        // §5.1.6 / §3.6.2.1 — normalized MUST NOT be true for FLOAT or
        // UNSIGNED_INT component types.
        if acc.normalized
            && (acc.component_type == COMPONENT_TYPE_FLOAT
                || acc.component_type == COMPONENT_TYPE_UNSIGNED_INT)
        {
            return Err(invalid(format!(
                "AccessorNormalizedComponentType: accessors[{ai}].normalized = true with \
                 componentType = {} — MUST NOT be set to true for FLOAT (5126) or \
                 UNSIGNED_INT (5125) component type (spec §5.1.6 / §3.6.2.1)",
                acc.component_type
            )));
        }

        // §3.6.2.5 — min / max array length MUST equal the component
        // count derived from `type`. Skip when `type` is unknown (the
        // bufferView-fit pass rejects that separately).
        if let Some(components) = type_components(&acc.kind) {
            let components = components as usize;
            if let Some(min) = &acc.min {
                if min.len() != components {
                    return Err(invalid(format!(
                        "AccessorMinMaxLength: accessors[{ai}].min has {} entries but type {} \
                         has {} components — the arrays MUST be equal length (spec §3.6.2.5)",
                        min.len(),
                        acc.kind,
                        components
                    )));
                }
            }
            if let Some(max) = &acc.max {
                if max.len() != components {
                    return Err(invalid(format!(
                        "AccessorMinMaxLength: accessors[{ai}].max has {} entries but type {} \
                         has {} components — the arrays MUST be equal length (spec §3.6.2.5)",
                        max.len(),
                        acc.kind,
                        components
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Validate texture sampler filter / wrap modes against the glTF 2.0
/// spec §5.26 (Sampler).
///
/// Each of `magFilter`, `minFilter`, `wrapS`, `wrapT` is an OPTIONAL
/// integer, but when present its value is constrained to a closed set
/// of WebGL enum constants (Table 25 plus §5.26.1–§5.26.4 "Allowed
/// values"). The spec lists no other legal values, so any out-of-set
/// integer is a hard violation:
///
/// * §5.26.1 `magFilter` ∈ { 9728 NEAREST, 9729 LINEAR }
///   (`SamplerMagFilter`).
/// * §5.26.2 `minFilter` ∈ { 9728 NEAREST, 9729 LINEAR,
///   9984 NEAREST_MIPMAP_NEAREST, 9985 LINEAR_MIPMAP_NEAREST,
///   9986 NEAREST_MIPMAP_LINEAR, 9987 LINEAR_MIPMAP_LINEAR }
///   (`SamplerMinFilter`).
/// * §5.26.3 `wrapS` ∈ { 33071 CLAMP_TO_EDGE, 33648 MIRRORED_REPEAT,
///   10497 REPEAT } (`SamplerWrapS`).
/// * §5.26.4 `wrapT` — same set as `wrapS` (`SamplerWrapT`).
///
/// Absent properties are not policed here: `wrapS`/`wrapT` carry a
/// spec default of 10497 (applied at read time / left to the consumer),
/// and `magFilter`/`minFilter` have no default — an absent filter means
/// "implementation choice", which is conformant.
pub fn validate_samplers(samplers: &[crate::json_model::Sampler]) -> Result<()> {
    use crate::json_model::{
        MAG_FILTER_LINEAR, MAG_FILTER_NEAREST, MIN_FILTER_LINEAR, MIN_FILTER_LINEAR_MIPMAP_LINEAR,
        MIN_FILTER_LINEAR_MIPMAP_NEAREST, MIN_FILTER_NEAREST, MIN_FILTER_NEAREST_MIPMAP_LINEAR,
        MIN_FILTER_NEAREST_MIPMAP_NEAREST, WRAP_CLAMP_TO_EDGE, WRAP_MIRRORED_REPEAT, WRAP_REPEAT,
    };
    for (si, s) in samplers.iter().enumerate() {
        if let Some(v) = s.mag_filter {
            if !matches!(v, MAG_FILTER_NEAREST | MAG_FILTER_LINEAR) {
                return Err(invalid(format!(
                    "SamplerMagFilter: samplers[{si}].magFilter = {v} — MUST be one of \
                     9728 (NEAREST) or 9729 (LINEAR) (spec §5.26.1)"
                )));
            }
        }
        if let Some(v) = s.min_filter {
            if !matches!(
                v,
                MIN_FILTER_NEAREST
                    | MIN_FILTER_LINEAR
                    | MIN_FILTER_NEAREST_MIPMAP_NEAREST
                    | MIN_FILTER_LINEAR_MIPMAP_NEAREST
                    | MIN_FILTER_NEAREST_MIPMAP_LINEAR
                    | MIN_FILTER_LINEAR_MIPMAP_LINEAR
            ) {
                return Err(invalid(format!(
                    "SamplerMinFilter: samplers[{si}].minFilter = {v} — MUST be one of \
                     9728 (NEAREST), 9729 (LINEAR), 9984 (NEAREST_MIPMAP_NEAREST), \
                     9985 (LINEAR_MIPMAP_NEAREST), 9986 (NEAREST_MIPMAP_LINEAR), or \
                     9987 (LINEAR_MIPMAP_LINEAR) (spec §5.26.2)"
                )));
            }
        }
        if let Some(v) = s.wrap_s {
            if !matches!(v, WRAP_CLAMP_TO_EDGE | WRAP_MIRRORED_REPEAT | WRAP_REPEAT) {
                return Err(invalid(format!(
                    "SamplerWrapS: samplers[{si}].wrapS = {v} — MUST be one of \
                     33071 (CLAMP_TO_EDGE), 33648 (MIRRORED_REPEAT), or 10497 (REPEAT) \
                     (spec §5.26.3)"
                )));
            }
        }
        if let Some(v) = s.wrap_t {
            if !matches!(v, WRAP_CLAMP_TO_EDGE | WRAP_MIRRORED_REPEAT | WRAP_REPEAT) {
                return Err(invalid(format!(
                    "SamplerWrapT: samplers[{si}].wrapT = {v} — MUST be one of \
                     33071 (CLAMP_TO_EDGE), 33648 (MIRRORED_REPEAT), or 10497 (REPEAT) \
                     (spec §5.26.4)"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the node graph + per-node transforms against the
/// glTF 2.0 spec §3.5.2 (node hierarchy) and §3.5.3 (transformations).
///
/// The rules enforced are all hard MUST constraints:
///
/// * §3.5.2 "The node hierarchy MUST be a set of disjoint strict
///   trees. That is node hierarchy MUST NOT contain cycles and each
///   node MUST have zero or one parent node." We police this in three
///   parts: every `children[]` index MUST resolve into `nodes[]`
///   (`NodeChildIndex`); a node MUST NOT appear in the `children` of
///   more than one node (`NodeMultipleParents`); and following the
///   parent links MUST NOT close a cycle (`NodeHierarchyCycle`,
///   which also catches a node listing itself as a child).
/// * §3.5.3 "Any node MAY define a local space transform either by
///   supplying a `matrix` property, or any of `translation`,
///   `rotation`, and `scale` properties." The "either … or" wording
///   (mirrored by the JSON schema's `not`/`required` clauses) makes
///   `matrix` mutually exclusive with every TRS property
///   (`NodeMatrixTRSExclusive`).
/// * §3.5.3 "When a node is targeted for animation (referenced by an
///   `animation.channel.target`), only TRS properties MAY be present;
///   `matrix` MUST NOT be present." (`NodeAnimatedMatrix`).
/// * §3.5.3 "`rotation` is a unit quaternion value" — a present
///   `rotation` MUST be finite and (approximately) normalized
///   (`NodeRotationUnitQuaternion`).
/// * §3.5.3 "When `matrix` is defined, it MUST be decomposable to TRS
///   properties." A matrix with a zero or non-finite determinant
///   cannot be decomposed into a `T * R * S` product (the rotation /
///   scale factorisation requires an invertible upper-left 3×3), so
///   it is rejected (`NodeMatrixDecompose`). The conservative
///   determinant test avoids false positives on the shear/skew
///   sub-case (an Implementation Note, not a MUST).
/// * All transform components MUST be finite numbers
///   (`NodeMatrixFinite` / `NodeTranslationFinite` / `NodeScaleFinite`);
///   the rotation finiteness is folded into the unit-quaternion check.
pub fn validate_nodes(nodes: &[crate::json_model::Node], animations: &[Animation]) -> Result<()> {
    let n = nodes.len();

    // --- §3.5.2 hierarchy: child-index range + single parent ---
    // `parent_of[child]` records the first parent that claimed `child`;
    // a second claim is the "more than one parent" violation.
    let mut parent_of: Vec<Option<usize>> = vec![None; n];
    for (pi, node) in nodes.iter().enumerate() {
        for &c in &node.children {
            let ci = c as usize;
            if ci >= n {
                return Err(invalid(format!(
                    "NodeChildIndex: nodes[{pi}].children references node {c}, out of range \
                     (document has {n} nodes) (spec §3.5.2)"
                )));
            }
            if let Some(existing) = parent_of[ci] {
                return Err(invalid(format!(
                    "NodeMultipleParents: node {ci} appears in the children of both nodes \
                     {existing} and {pi} — each node MUST have zero or one parent (spec §3.5.2)"
                )));
            }
            parent_of[ci] = Some(pi);
        }
    }

    // --- §3.5.2 hierarchy: no cycles ---
    // With single-parent already enforced, a cycle is detectable by
    // walking parent links upward from every node; a strict tree
    // terminates at a root (parent None). Any node whose upward walk
    // revisits its own start has closed a cycle (this also catches a
    // node that lists itself as a child, which makes it its own parent).
    for start in 0..n {
        let mut steps = 0usize;
        let mut cur = parent_of[start];
        while let Some(p) = cur {
            if p == start {
                return Err(invalid(format!(
                    "NodeHierarchyCycle: following parent links from node {start} returns to \
                     itself — the node hierarchy MUST NOT contain cycles (spec §3.5.2)"
                )));
            }
            steps += 1;
            if steps > n {
                // Defensive: with single-parent enforced this cannot
                // exceed n without revisiting `start` first, but bound
                // the walk so a malformed graph can never loop forever.
                return Err(invalid(format!(
                    "NodeHierarchyCycle: parent-link walk from node {start} exceeded {n} steps \
                     — the node hierarchy MUST NOT contain cycles (spec §3.5.2)"
                )));
            }
            cur = parent_of[p];
        }
    }

    // --- §3.5.3 per-node transform rules ---
    // A node is "targeted for animation" when any animation channel's
    // target.node points at it.
    let mut animated = vec![false; n];
    for anim in animations {
        for ch in &anim.channels {
            if let Some(ni) = ch.target.node {
                if (ni as usize) < n {
                    animated[ni as usize] = true;
                }
            }
        }
    }

    for (i, node) in nodes.iter().enumerate() {
        let has_trs = node.translation.is_some() || node.rotation.is_some() || node.scale.is_some();

        if let Some(m) = &node.matrix {
            // matrix ⊥ TRS.
            if has_trs {
                return Err(invalid(format!(
                    "NodeMatrixTRSExclusive: nodes[{i}] defines BOTH `matrix` and one or more \
                     TRS properties — they are mutually exclusive (spec §3.5.3)"
                )));
            }
            // Animated nodes MUST NOT carry matrix.
            if animated[i] {
                return Err(invalid(format!(
                    "NodeAnimatedMatrix: nodes[{i}] is targeted by an animation channel but \
                     defines `matrix` — animated nodes MUST use TRS only (spec §3.5.3)"
                )));
            }
            // All 16 components finite.
            if let Some(bad) = m.iter().position(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "NodeMatrixFinite: nodes[{i}].matrix[{bad}] = {} is not finite \
                     (spec §3.5.3)",
                    m[bad]
                )));
            }
            // Must be decomposable to TRS → upper-left 3×3 invertible
            // (non-zero, finite determinant). The matrix is stored
            // column-major (spec §3.5.2.1), so column k spans
            // m[4k..4k+4]; the upper-left 3×3 takes rows 0..3 of the
            // first three columns.
            let c0 = [m[0], m[1], m[2]];
            let c1 = [m[4], m[5], m[6]];
            let c2 = [m[8], m[9], m[10]];
            let det = c0[0] * (c1[1] * c2[2] - c1[2] * c2[1])
                - c1[0] * (c0[1] * c2[2] - c0[2] * c2[1])
                + c2[0] * (c0[1] * c1[2] - c0[2] * c1[1]);
            if !det.is_finite() || det == 0.0 {
                return Err(invalid(format!(
                    "NodeMatrixDecompose: nodes[{i}].matrix has a {} upper-left 3×3 \
                     determinant ({det}) and is not decomposable to TRS (spec §3.5.3)",
                    if det == 0.0 { "zero" } else { "non-finite" }
                )));
            }
        }

        if let Some(t) = &node.translation {
            if let Some(bad) = t.iter().position(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "NodeTranslationFinite: nodes[{i}].translation[{bad}] = {} is not finite \
                     (spec §3.5.3)",
                    t[bad]
                )));
            }
        }

        if let Some(s) = &node.scale {
            if let Some(bad) = s.iter().position(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "NodeScaleFinite: nodes[{i}].scale[{bad}] = {} is not finite (spec §3.5.3)",
                    s[bad]
                )));
            }
        }

        if let Some(q) = &node.rotation {
            if let Some(bad) = q.iter().position(|v| !v.is_finite()) {
                return Err(invalid(format!(
                    "NodeRotationUnitQuaternion: nodes[{i}].rotation[{bad}] = {} is not finite \
                     — `rotation` MUST be a unit quaternion (spec §3.5.3)",
                    q[bad]
                )));
            }
            let len2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
            // Unit length within a tolerance that comfortably absorbs
            // f32 round-trip error from normalized-integer storage
            // (the §3.6.2.2 quantisation grid is no coarser than
            // 1/32767, so a unit quaternion stays within ~1e-3 of
            // length 1 after a normalize/round-trip).
            if (len2 - 1.0).abs() > 2e-3 {
                return Err(invalid(format!(
                    "NodeRotationUnitQuaternion: nodes[{i}].rotation has squared length {len2} \
                     (expected ≈ 1.0) — `rotation` MUST be a unit quaternion (spec §3.5.3)"
                )));
            }
        }
    }

    Ok(())
}

/// Document-level skin validation per glTF 2.0 §5.28 (Skin), §3.7.3
/// (Skins) and §5.25.3 (node.skin).
///
/// The MUST-level rules enforced here:
///
/// * **§5.28.3** — `skin.joints` MUST contain at least one element
///   (`integer [1-*]`); every joint index MUST be a valid node index
///   (`>= 0` and within the node array); every joint index MUST be
///   unique within the array.
/// * **§5.28.2** — `skin.skeleton`, when present, MUST be a valid node
///   index.
/// * **§5.28.1 / §3.7.3.1** — the accessor referenced by
///   `skin.inverseBindMatrices`, when present, MUST be a valid accessor
///   index, MUST have `"MAT4"` type with floating-point (`FLOAT`)
///   components, MUST NOT be `normalized`, and its `count` MUST be
///   greater than or equal to the number of `joints` elements.
/// * **§5.25.3** — when a node defines `skin`, the skin index MUST be a
///   valid skin index AND the node MUST also define `mesh`.
/// * **§3.7.3.2** — when a skin is referenced by a node within a scene,
///   all joints used by the skin MUST belong to that same scene (the
///   common root MUST belong to the same scene). A node belongs to a
///   scene iff its tree root is one of the scene's listed root nodes.
///
/// The §3.7.3.2 *common-root* clause ("each skin's joints MUST have a
/// common parent node ... which may or may not be a joint node itself")
/// is intentionally **not** enforced as a document-node-ancestry MUST:
/// the spec explicitly allows the common root to be a node that "may or
/// may not be a joint node itself", and this crate's own encoder emits
/// skins whose joints are distinct scene-root nodes (the scene is their
/// implicit common root). The enforceable part is the scene-membership
/// rule above. The deformation-quality SHOULDs (weight-sum
/// renormalisation, "unused joint values SHOULD be zero") are likewise
/// left to the reader/engine.
pub fn validate_skins(
    skins: &[Skin],
    nodes: &[Node],
    accessors: &[Accessor],
    scenes: &[Scene],
) -> Result<()> {
    let node_count = nodes.len();
    let accessor_count = accessors.len();

    // Parent map for the node forest (single-parent already guaranteed
    // by validate_nodes). `parent_of[child] = Some(parent)`.
    let mut parent_of: Vec<Option<usize>> = vec![None; node_count];
    for (pi, node) in nodes.iter().enumerate() {
        for &c in &node.children {
            let ci = c as usize;
            if ci < node_count {
                parent_of[ci] = Some(pi);
            }
        }
    }
    // Is `anc` an ancestor-or-self of `node`?
    let is_ancestor_or_self = |anc: usize, node: usize| -> bool {
        let mut cur = node;
        let mut steps = 0usize;
        loop {
            if cur == anc {
                return true;
            }
            match parent_of[cur] {
                Some(p) => {
                    cur = p;
                    steps += 1;
                    if steps > node_count {
                        return false;
                    }
                }
                None => return false,
            }
        }
    };

    for (si, skin) in skins.iter().enumerate() {
        // §5.28.3 — at least one joint.
        if skin.joints.is_empty() {
            return Err(invalid(format!(
                "SkinJointsEmpty: skins[{si}].joints is empty — `joints` MUST contain at least \
                 one element (spec §5.28.3)"
            )));
        }
        // §5.28.3 — joint indices in range + unique.
        let mut seen = std::collections::HashSet::with_capacity(skin.joints.len());
        for &j in &skin.joints {
            let ji = j as usize;
            if ji >= node_count {
                return Err(invalid(format!(
                    "SkinJointIndex: skins[{si}].joints references node {j}, out of range \
                     (document has {node_count} nodes) (spec §5.28.3)"
                )));
            }
            if !seen.insert(j) {
                return Err(invalid(format!(
                    "SkinJointDuplicate: skins[{si}].joints lists node {j} more than once — each \
                     element MUST be unique (spec §5.28.3)"
                )));
            }
        }

        // §5.28.2 — skeleton node index in range.
        if let Some(sk) = skin.skeleton {
            if (sk as usize) >= node_count {
                return Err(invalid(format!(
                    "SkinSkeletonIndex: skins[{si}].skeleton references node {sk}, out of range \
                     (document has {node_count} nodes) (spec §5.28.2)"
                )));
            }
        }

        // §5.28.1 / §3.7.3.1 — inverseBindMatrices accessor format.
        if let Some(ibm) = skin.inverse_bind_matrices {
            let ai = ibm as usize;
            if ai >= accessor_count {
                return Err(invalid(format!(
                    "SkinIbmIndex: skins[{si}].inverseBindMatrices references accessor {ibm}, out \
                     of range (document has {accessor_count} accessors) (spec §5.28.1)"
                )));
            }
            let acc = &accessors[ai];
            if acc.kind != "MAT4" {
                return Err(invalid(format!(
                    "SkinIbmAccessorType: skins[{si}].inverseBindMatrices accessor {ibm} has type \
                     {:?} — an inverse-bind-matrices accessor MUST be \"MAT4\" (spec §3.7.3.1)",
                    acc.kind
                )));
            }
            if acc.component_type != COMPONENT_TYPE_FLOAT {
                return Err(invalid(format!(
                    "SkinIbmAccessorComponentType: skins[{si}].inverseBindMatrices accessor {ibm} \
                     has componentType {} — inverse-bind matrices MUST have floating-point (FLOAT, \
                     5126) components (spec §3.7.3.1)",
                    acc.component_type
                )));
            }
            if acc.normalized {
                return Err(invalid(format!(
                    "SkinIbmAccessorNormalized: skins[{si}].inverseBindMatrices accessor {ibm} is \
                     `normalized` — inverse-bind matrices are floating-point and MUST NOT be \
                     normalized (spec §3.7.3.1)"
                )));
            }
            if (acc.count as usize) < skin.joints.len() {
                return Err(invalid(format!(
                    "SkinIbmCount: skins[{si}].inverseBindMatrices accessor {ibm} has count {} but \
                     the skin lists {} joints — the accessor count MUST be >= the number of joints \
                     (spec §5.28.1)",
                    acc.count,
                    skin.joints.len()
                )));
            }
        }
    }

    // §5.25.3 — node.skin coupling.
    for (ni, node) in nodes.iter().enumerate() {
        if let Some(s) = node.skin {
            if (s as usize) >= skins.len() {
                return Err(invalid(format!(
                    "NodeSkinIndex: nodes[{ni}].skin references skin {s}, out of range (document \
                     has {} skins) (spec §5.25.3)",
                    skins.len()
                )));
            }
            if node.mesh.is_none() {
                return Err(invalid(format!(
                    "NodeSkinWithoutMesh: nodes[{ni}] defines `skin` but no `mesh` — when `skin` \
                     is defined, `mesh` MUST also be defined (spec §5.25.3)"
                )));
            }
            // §3.7.3.2 — when a skin is referenced by a node within a
            // scene, all joints used by the skin MUST belong to the same
            // scene (the common root MUST belong to the same scene). A
            // node belongs to a scene iff its tree root is one of the
            // scene's root nodes.
            let skin = &skins[s as usize];
            // Find the scene(s) that (transitively) contain `ni`.
            for (sci, scene) in scenes.iter().enumerate() {
                let mut node_in_scene = false;
                for &sr in &scene.nodes {
                    if (sr as usize) < node_count && is_ancestor_or_self(sr as usize, ni) {
                        node_in_scene = true;
                        break;
                    }
                }
                if !node_in_scene {
                    continue;
                }
                // The skinned node is in this scene → every joint MUST be
                // reachable from one of this scene's roots.
                for &j in &skin.joints {
                    let ji = j as usize;
                    let joint_in_scene = scene.nodes.iter().any(|&sr| {
                        (sr as usize) < node_count && is_ancestor_or_self(sr as usize, ji)
                    });
                    if !joint_in_scene {
                        return Err(invalid(format!(
                            "SkinJointWrongScene: nodes[{ni}] (in scenes[{sci}]) references \
                             skins[{s}] whose joint node {j} does not belong to scenes[{sci}] — \
                             all joints used by a skin referenced within a scene MUST belong to \
                             that scene (spec §3.7.3.2)"
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Document-level texture-reference validation per glTF 2.0 §5.29
/// (Texture), §5.30 (Texture Info) and §5.22 (Material PBR Metallic
/// Roughness).
///
/// Every index a texture or material carries into another top-level
/// array MUST resolve to a real entry. The structs already pin the
/// indices to `u32` (so the `>= 0` minimum is automatic); the missing
/// MUST is the upper bound. Enforced here:
///
/// * **§5.29.1** — `texture.source`, when present, MUST be a valid index
///   into `images[]` (`TextureSourceIndex`).
/// * **§5.29.2** — `texture.sampler`, when present, MUST be a valid index
///   into `samplers[]` (`TextureSamplerIndex`).
/// * **§5.30.1** — every material `textureInfo.index` (across
///   `pbrMetallicRoughness.baseColorTexture` /
///   `metallicRoughnessTexture`, `normalTexture`, `occlusionTexture`,
///   `emissiveTexture`) MUST be a valid index into `textures[]`
///   (`MaterialTextureIndex`).
///
/// The `KHR_texture_basisu` per-texture `source` indirection has its own
/// in-range check in `validate_extension_stack`; this pass covers the
/// core (non-extension) references only.
pub fn validate_textures(
    textures: &[Texture],
    images: &[crate::json_model::Image],
    samplers: &[crate::json_model::Sampler],
    materials: &[Material],
) -> Result<()> {
    let image_count = images.len();
    let sampler_count = samplers.len();
    let texture_count = textures.len();

    for (ti, tex) in textures.iter().enumerate() {
        if let Some(src) = tex.source {
            if (src as usize) >= image_count {
                return Err(invalid(format!(
                    "TextureSourceIndex: textures[{ti}].source = {src} is out of range (document \
                     has {image_count} images) (spec §5.29.1)"
                )));
            }
        }
        if let Some(samp) = tex.sampler {
            if (samp as usize) >= sampler_count {
                return Err(invalid(format!(
                    "TextureSamplerIndex: textures[{ti}].sampler = {samp} is out of range (document \
                     has {sampler_count} samplers) (spec §5.29.2)"
                )));
            }
        }
    }

    for (mi, mat) in materials.iter().enumerate() {
        // Collect every core textureInfo.index this material references,
        // each with a spec-named slot for the diagnostic.
        let mut refs: Vec<(&str, u32)> = Vec::new();
        if let Some(pbr) = &mat.pbr_metallic_roughness {
            if let Some(ti) = &pbr.base_color_texture {
                refs.push(("pbrMetallicRoughness.baseColorTexture", ti.index));
            }
            if let Some(ti) = &pbr.metallic_roughness_texture {
                refs.push(("pbrMetallicRoughness.metallicRoughnessTexture", ti.index));
            }
        }
        if let Some(ti) = &mat.normal_texture {
            refs.push(("normalTexture", ti.index));
        }
        if let Some(ti) = &mat.occlusion_texture {
            refs.push(("occlusionTexture", ti.index));
        }
        if let Some(ti) = &mat.emissive_texture {
            refs.push(("emissiveTexture", ti.index));
        }
        for (slot, idx) in refs {
            if (idx as usize) >= texture_count {
                return Err(invalid(format!(
                    "MaterialTextureIndex: materials[{mi}].{slot}.index = {idx} is out of range \
                     (document has {texture_count} textures) (spec §5.30.1)"
                )));
            }
        }
    }

    Ok(())
}

/// Validate every top-level index reference that maps one object into a
/// sibling root array, per the glTF 2.0 schema MUSTs the decoder parsed
/// but never policed structurally. The field types already pin the
/// non-negative minimum; the missing rule in each case is the upper
/// bound — an index MUST resolve into the referenced array.
///
/// Rules enforced (each citing the spec property table):
///
/// * §3.3 / Table "glTF Properties" — the root `scene` (default scene)
///   index, when present, MUST resolve into `scenes[]`
///   (`DefaultSceneIndex`).
/// * §5.27.1 — every `scenes[i].nodes[j]` MUST resolve into `nodes[]`
///   (`SceneNodeIndex`).
/// * §5.25.5 — `nodes[i].mesh`, when present, MUST resolve into
///   `meshes[]` (`NodeMeshIndex`).
/// * §5.25.1 — `nodes[i].camera`, when present, MUST resolve into
///   `cameras[]` (`NodeCameraIndex`).
/// * §5.24.3 — every `meshes[i].primitives[j].material`, when present,
///   MUST resolve into `materials[]` (`PrimitiveMaterialIndex`).
///
/// `node.skin` is policed by `validate_skins` (it also needs the
/// mesh-bearing co-requisite from §5.25.3); `node.children` are policed
/// by `validate_nodes` (alongside the no-cycle / single-parent tree
/// MUSTs); textureInfo / texture references are policed by
/// `validate_textures`; animation-channel target nodes by
/// `validate_animation_channels`. This pass covers the remaining
/// top-level index edges.
pub fn validate_index_references(root: &GltfRoot) -> Result<()> {
    let scene_count = root.scenes.len();
    let node_count = root.nodes.len();
    let mesh_count = root.meshes.len();
    let camera_count = root.cameras.len();
    let material_count = root.materials.len();

    if let Some(scene) = root.scene {
        if (scene as usize) >= scene_count {
            return Err(invalid(format!(
                "DefaultSceneIndex: glTF.scene = {scene} is out of range (document has \
                 {scene_count} scenes) (spec §3.3)"
            )));
        }
    }

    for (si, scene) in root.scenes.iter().enumerate() {
        for &n in &scene.nodes {
            if (n as usize) >= node_count {
                return Err(invalid(format!(
                    "SceneNodeIndex: scenes[{si}].nodes references node {n} which is out of \
                     range (document has {node_count} nodes) (spec §5.27.1)"
                )));
            }
        }
    }

    for (ni, node) in root.nodes.iter().enumerate() {
        if let Some(mesh) = node.mesh {
            if (mesh as usize) >= mesh_count {
                return Err(invalid(format!(
                    "NodeMeshIndex: nodes[{ni}].mesh = {mesh} is out of range (document has \
                     {mesh_count} meshes) (spec §5.25.5)"
                )));
            }
        }
        if let Some(camera) = node.camera {
            if (camera as usize) >= camera_count {
                return Err(invalid(format!(
                    "NodeCameraIndex: nodes[{ni}].camera = {camera} is out of range (document \
                     has {camera_count} cameras) (spec §5.25.1)"
                )));
            }
        }
    }

    for (mi, mesh) in root.meshes.iter().enumerate() {
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            if let Some(material) = prim.material {
                if (material as usize) >= material_count {
                    return Err(invalid(format!(
                        "PrimitiveMaterialIndex: meshes[{mi}].primitives[{pi}].material = \
                         {material} is out of range (document has {material_count} materials) \
                         (spec §5.24.3)"
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Validate the structural-minimum MUSTs the JSON schema pins on the
/// buffer / bufferView byte-length and on the `accessor.sparse.count`,
/// per the glTF 2.0 property tables. These are independent of any
/// buffer materialisation — they hold on the declared integers alone.
///
/// Rules enforced:
///
/// * §5.10.2 — `buffers[i].byteLength` MUST be `>= 1`
///   (`BufferByteLength`, schema "Minimum: >= 1").
/// * §5.11.3 — `bufferViews[i].byteLength` MUST be `>= 1`
///   (`BufferViewByteLength`, schema "Minimum: >= 1").
/// * §5.2.1 — `accessors[i].sparse.count` MUST be `>= 1`
///   (`SparseCountMin`, schema "Minimum: >= 1").
/// * §3.6.2.3 / §5.2.1 — `accessors[i].sparse.count` MUST NOT be greater
///   than the base accessor's element `count` (`SparseCountRange` — "This
///   number MUST NOT be greater than the number of the base accessor
///   elements").
///
/// The companion value-level sparse MUSTs (the indices MUST form a
/// strictly increasing sequence and MUST be `< count`) are enforced at
/// decode time in `accessor::read_sparse_indices` once the index bytes
/// are read; here we police only the structural bound that needs no
/// buffer access so a never-materialised accessor still fails fast.
pub fn validate_structural_minimums(root: &GltfRoot) -> Result<()> {
    for (bi, buf) in root.buffers.iter().enumerate() {
        if buf.byte_length < 1 {
            return Err(invalid(format!(
                "BufferByteLength: buffers[{bi}].byteLength = {} MUST be >= 1 (spec §5.10.2)",
                buf.byte_length
            )));
        }
    }

    for (bvi, bv) in root.buffer_views.iter().enumerate() {
        if bv.byte_length < 1 {
            return Err(invalid(format!(
                "BufferViewByteLength: bufferViews[{bvi}].byteLength = {} MUST be >= 1 \
                 (spec §5.11.3)",
                bv.byte_length
            )));
        }
    }

    for (ai, acc) in root.accessors.iter().enumerate() {
        if let Some(sparse) = acc.sparse.as_ref() {
            if sparse.count < 1 {
                return Err(invalid(format!(
                    "SparseCountMin: accessors[{ai}].sparse.count = {} MUST be >= 1 (spec §5.2.1)",
                    sparse.count
                )));
            }
            if sparse.count > acc.count {
                return Err(invalid(format!(
                    "SparseCountRange: accessors[{ai}].sparse.count = {} MUST NOT be greater than \
                     the base accessor element count {} (spec §3.6.2.3)",
                    sparse.count, acc.count
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_model::{
        Accessor, AccessorSparse, AccessorSparseIndices, AccessorSparseValues, Animation,
        AnimationChannel, AnimationChannelTarget, AnimationSampler, Asset, Buffer, BufferView,
        KhrLightsPunctualRoot, Material, MaterialAnisotropy, MaterialClearcoat,
        MaterialDiffuseTransmission, MaterialDispersion, MaterialEmissiveStrength,
        MaterialExtensions, MaterialIor, MaterialIridescence, MaterialSheen, MaterialSpecular,
        MaterialTransmission, MaterialUnlit, MaterialVolume, Mesh, Node, NodeExtensions,
        NodeLightRef, Primitive, RootExtensions, COMPONENT_TYPE_FLOAT,
    };
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
            extensions: None,
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
            extensions: None,
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
            extensions: None,
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

    // --- §3.7.2.1 topology vertex-count rules -----------------------

    #[test]
    fn index_count_points_requires_nonzero() {
        use crate::json_model::MODE_POINTS;
        validate_primitive_index_count(MODE_POINTS, 1).unwrap();
        validate_primitive_index_count(MODE_POINTS, 7).unwrap();
        let err = validate_primitive_index_count(MODE_POINTS, 0).unwrap_err();
        assert!(format!("{err}").contains("PrimitiveIndexCount"));
    }

    #[test]
    fn index_count_lines_divisible_by_two() {
        use crate::json_model::MODE_LINES;
        validate_primitive_index_count(MODE_LINES, 2).unwrap();
        validate_primitive_index_count(MODE_LINES, 6).unwrap();
        assert!(validate_primitive_index_count(MODE_LINES, 0).is_err());
        assert!(validate_primitive_index_count(MODE_LINES, 3).is_err());
    }

    #[test]
    fn index_count_line_loop_and_strip_min_two() {
        use crate::json_model::{MODE_LINE_LOOP, MODE_LINE_STRIP};
        validate_primitive_index_count(MODE_LINE_LOOP, 2).unwrap();
        validate_primitive_index_count(MODE_LINE_STRIP, 5).unwrap();
        assert!(validate_primitive_index_count(MODE_LINE_LOOP, 1).is_err());
        assert!(validate_primitive_index_count(MODE_LINE_STRIP, 0).is_err());
    }

    #[test]
    fn index_count_triangles_divisible_by_three() {
        use crate::json_model::MODE_TRIANGLES;
        validate_primitive_index_count(MODE_TRIANGLES, 3).unwrap();
        validate_primitive_index_count(MODE_TRIANGLES, 9).unwrap();
        assert!(validate_primitive_index_count(MODE_TRIANGLES, 0).is_err());
        assert!(validate_primitive_index_count(MODE_TRIANGLES, 4).is_err());
        assert!(validate_primitive_index_count(MODE_TRIANGLES, 5).is_err());
    }

    #[test]
    fn index_count_triangle_strip_and_fan_min_three() {
        use crate::json_model::{MODE_TRIANGLE_FAN, MODE_TRIANGLE_STRIP};
        validate_primitive_index_count(MODE_TRIANGLE_STRIP, 3).unwrap();
        // strips/fans need not be divisible by 3 — only >= 3.
        validate_primitive_index_count(MODE_TRIANGLE_STRIP, 4).unwrap();
        validate_primitive_index_count(MODE_TRIANGLE_FAN, 5).unwrap();
        assert!(validate_primitive_index_count(MODE_TRIANGLE_STRIP, 2).is_err());
        assert!(validate_primitive_index_count(MODE_TRIANGLE_FAN, 0).is_err());
    }

    #[test]
    fn index_value_bound_rejects_out_of_range() {
        // attribute count 4 → valid indices are 0..=3.
        validate_index_value_bound(&[0, 1, 2, 3, 0, 2], 4).unwrap();
        let err = validate_index_value_bound(&[0, 1, 4], 4).unwrap_err();
        assert!(format!("{err}").contains("PrimitiveIndexBound"));
        // equal to count is out of range (upper bound is exclusive).
        assert!(validate_index_value_bound(&[3], 3).is_err());
    }

    // --- JSON byte-length cap ---------------------------------------

    #[test]
    fn json_byte_length_accepts_normal_doc() {
        check_json_byte_length(br#"{"asset":{"version":"2.0"}}"#).unwrap();
    }

    #[test]
    fn json_byte_length_rejects_oversized() {
        // Just over the cap.
        let big = vec![b'x'; MAX_JSON_BYTES + 1];
        let err = check_json_byte_length(&big).unwrap_err();
        assert!(format!("{err}").contains("JsonTooLarge"));
    }

    // --- JSON depth cap ---------------------------------------------

    #[test]
    fn json_depth_accepts_shallow_doc() {
        let s = br#"{"asset":{"version":"2.0"}, "nodes": [{"name":"a"}]}"#;
        check_json_depth(s).unwrap();
    }

    #[test]
    fn json_depth_rejects_deep_array_bomb() {
        // 300 layers of `[` followed by 300 layers of `]` — well over the
        // 256-level cap.
        let mut s: Vec<u8> = vec![b'['; 300];
        s.extend(std::iter::repeat_n(b']', 300));
        let err = check_json_depth(&s).unwrap_err();
        assert!(format!("{err}").contains("JsonDepthExceeded"));
    }

    #[test]
    fn json_depth_ignores_brackets_in_strings() {
        // 300 `[`s but ALL inside a string literal — depth should stay
        // at 1 (the outer object).
        let mut s: Vec<u8> = b"{\"k\":\"".to_vec();
        s.extend(std::iter::repeat_n(b'[', 300));
        s.extend_from_slice(b"\"}");
        check_json_depth(&s).unwrap();
    }

    #[test]
    fn json_depth_ignores_escaped_quote_in_string() {
        // The escaped quote must NOT close the string prematurely.
        let s = br#"{"k":"foo\"bar","arr":[1,2,3]}"#;
        check_json_depth(s).unwrap();
    }

    #[test]
    fn json_depth_accepts_exactly_at_limit() {
        let mut s: Vec<u8> = vec![b'['; MAX_JSON_DEPTH];
        s.extend(std::iter::repeat_n(b']', MAX_JSON_DEPTH));
        check_json_depth(&s).unwrap();
    }

    // --- Extension stack consistency --------------------------------

    fn empty_root() -> GltfRoot {
        GltfRoot {
            asset: crate::json_model::Asset {
                version: "2.0".into(),
                generator: None,
                copyright: None,
                min_version: None,
                extensions: None,
                extras: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_passes_when_clean() {
        let root = empty_root();
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_rejects_required_not_in_used() {
        let mut root = empty_root();
        root.extensions_required = vec!["KHR_materials_ior".into()];
        // extensions_used stays empty.
        let err = validate_extension_stack(&root).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackRequiredNotListed"));
    }

    #[test]
    fn extension_stack_accepts_required_in_used() {
        let mut root = empty_root();
        root.extensions_required = vec!["KHR_materials_ior".into()];
        root.extensions_used = vec!["KHR_materials_ior".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_rejects_root_lights_missing_used() {
        let mut root = empty_root();
        root.extensions = Some(RootExtensions {
            khr_lights_punctual: Some(KhrLightsPunctualRoot { lights: vec![] }),
            ..Default::default()
        });
        let err = validate_extension_stack(&root).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackUsedNotDeclared"));
    }

    #[test]
    fn extension_stack_rejects_node_lights_missing_used() {
        let mut root = empty_root();
        root.nodes.push(Node {
            extensions: Some(NodeExtensions {
                khr_lights_punctual: Some(NodeLightRef { light: 0 }),
                khr_node_visibility: None,
                ..Default::default()
            }),
            ..Default::default()
        });
        let err = validate_extension_stack(&root).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackUsedNotDeclared"));
    }

    #[test]
    fn extension_stack_rejects_node_visibility_missing_used() {
        let mut root = empty_root();
        root.nodes.push(Node {
            extensions: Some(NodeExtensions {
                khr_lights_punctual: None,
                khr_node_visibility: Some(crate::json_model::NodeVisibility {
                    visible: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_node_visibility"),
            "expected ExtensionStackUsedNotDeclared for KHR_node_visibility, got: {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_node_visibility_declared() {
        let mut root = empty_root();
        root.extensions_used.push("KHR_node_visibility".to_owned());
        root.nodes.push(Node {
            extensions: Some(NodeExtensions {
                khr_lights_punctual: None,
                khr_node_visibility: Some(crate::json_model::NodeVisibility {
                    visible: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        validate_extension_stack(&root).expect("declared extension must pass");
    }

    #[test]
    fn extension_stack_accepts_lights_declared() {
        let mut root = empty_root();
        root.extensions = Some(RootExtensions {
            khr_lights_punctual: Some(KhrLightsPunctualRoot { lights: vec![] }),
            ..Default::default()
        });
        root.extensions_used = vec!["KHR_lights_punctual".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_variants — docs/3d/gltf/extensions/KHR_materials_variants.md.
    fn variants_root() -> RootExtensions {
        RootExtensions {
            khr_materials_variants: Some(crate::json_model::KhrMaterialsVariantsRoot {
                variants: vec![
                    crate::json_model::MaterialVariant {
                        name: "Red".into(),
                        extras: None,
                    },
                    crate::json_model::MaterialVariant {
                        name: "Blue".into(),
                        extras: None,
                    },
                ],
            }),
            ..Default::default()
        }
    }

    fn mesh_with_mappings(mappings: Vec<crate::json_model::VariantMapping>) -> Mesh {
        Mesh {
            primitives: vec![Primitive {
                attributes: HashMap::new(),
                indices: None,
                material: None,
                mode: None,
                targets: vec![],
                extensions: Some(crate::json_model::PrimitiveExtensions {
                    khr_materials_variants: Some(crate::json_model::PrimitiveVariantMappings {
                        mappings,
                    }),
                    khr_gaussian_splatting: None,
                    khr_draco_mesh_compression: None,
                }),
                extras: None,
            }],
            name: None,
            weights: None,
            extensions: None,
            extras: None,
        }
    }

    #[test]
    fn extension_stack_rejects_root_variants_missing_used() {
        let mut root = empty_root();
        root.extensions = Some(variants_root());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_variants"),
            "expected ExtensionStackUsedNotDeclared for KHR_materials_variants, got: {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_primitive_variants_missing_used() {
        let mut root = empty_root();
        root.extensions = Some(variants_root());
        // Need at least one material so the mapping's material index resolves.
        root.materials.push(Material::default());
        root.extensions_used.push("KHR_materials_variants".into());
        root.meshes.push(mesh_with_mappings(vec![
            crate::json_model::VariantMapping {
                material: 0,
                variants: vec![0],
                name: None,
                extras: None,
            },
        ]));
        // Sanity: when used is declared, the doc validates.
        validate_extension_stack(&root).unwrap();
        // Now drop the declaration — must reject.
        root.extensions_used.clear();
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_variants"),
            "expected ExtensionStackUsedNotDeclared for KHR_materials_variants, got: {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_variant_index_out_of_range() {
        let mut root = empty_root();
        root.extensions = Some(variants_root());
        root.materials.push(Material::default());
        root.extensions_used.push("KHR_materials_variants".into());
        // variant index 2 is out of range (root has only 2 variants → 0..1)
        root.meshes.push(mesh_with_mappings(vec![
            crate::json_model::VariantMapping {
                material: 0,
                variants: vec![2],
                name: None,
                extras: None,
            },
        ]));
        let err = validate_extension_stack(&root).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackVariantsIndex"));
    }

    #[test]
    fn extension_stack_rejects_material_index_out_of_range() {
        let mut root = empty_root();
        root.extensions = Some(variants_root());
        // no materials at all, but mapping points at material 0
        root.extensions_used.push("KHR_materials_variants".into());
        root.meshes.push(mesh_with_mappings(vec![
            crate::json_model::VariantMapping {
                material: 0,
                variants: vec![0],
                name: None,
                extras: None,
            },
        ]));
        let err = validate_extension_stack(&root).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackVariantsMaterialIndex"));
    }

    #[test]
    fn extension_stack_rejects_duplicate_variant_in_primitive_mappings() {
        // Per the spec: "Across the entire mappings array, each variant
        // index must be used no more than one time."
        let mut root = empty_root();
        root.extensions = Some(variants_root());
        root.materials.push(Material::default());
        root.materials.push(Material::default());
        root.extensions_used.push("KHR_materials_variants".into());
        root.meshes.push(mesh_with_mappings(vec![
            crate::json_model::VariantMapping {
                material: 0,
                variants: vec![0],
                name: None,
                extras: None,
            },
            crate::json_model::VariantMapping {
                material: 1,
                variants: vec![0], // duplicate
                name: None,
                extras: None,
            },
        ]));
        let err = validate_extension_stack(&root).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackVariantsDuplicate"));
    }

    #[test]
    fn extension_stack_accepts_variants_declared_in_range() {
        let mut root = empty_root();
        root.extensions = Some(variants_root());
        root.materials.push(Material::default());
        root.materials.push(Material::default());
        root.extensions_used.push("KHR_materials_variants".into());
        root.meshes.push(mesh_with_mappings(vec![
            crate::json_model::VariantMapping {
                material: 0,
                variants: vec![0],
                name: None,
                extras: None,
            },
            crate::json_model::VariantMapping {
                material: 1,
                variants: vec![1],
                name: None,
                extras: None,
            },
        ]));
        validate_extension_stack(&root).expect("in-range mappings must pass");
    }

    // KHR_materials_unlit — docs/3d/gltf/extensions/KHR_materials_unlit.md.
    fn unlit_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_unlit: Some(MaterialUnlit {}),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_material_unlit_missing_used() {
        let mut root = empty_root();
        root.materials.push(unlit_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_unlit"),
            "expected ExtensionStackUsedNotDeclared for KHR_materials_unlit, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_material_unlit_declared() {
        let mut root = empty_root();
        root.materials.push(unlit_material());
        root.extensions_used = vec!["KHR_materials_unlit".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_emissive_strength —
    // docs/3d/gltf/extensions/KHR_materials_emissive_strength.md.
    fn emissive_strength_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_emissive_strength: Some(MaterialEmissiveStrength {
                    emissive_strength: Some(5.0),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_emissive_strength_missing_used() {
        let mut root = empty_root();
        root.materials.push(emissive_strength_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_emissive_strength"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_emissive_strength, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_emissive_strength_declared() {
        let mut root = empty_root();
        root.materials.push(emissive_strength_material());
        root.extensions_used = vec!["KHR_materials_emissive_strength".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_ior — docs/3d/gltf/extensions/KHR_materials_ior.md.
    fn ior_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_ior: Some(MaterialIor { ior: Some(1.4) }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_ior_missing_used() {
        let mut root = empty_root();
        root.materials.push(ior_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_ior"),
            "expected ExtensionStackUsedNotDeclared for KHR_materials_ior, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_ior_declared() {
        let mut root = empty_root();
        root.materials.push(ior_material());
        root.extensions_used = vec!["KHR_materials_ior".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_specular —
    // docs/3d/gltf/extensions/KHR_materials_specular.md.
    fn specular_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_specular: Some(MaterialSpecular {
                    specular_factor: Some(0.5),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_specular_missing_used() {
        let mut root = empty_root();
        root.materials.push(specular_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_specular"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_specular, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_specular_declared() {
        let mut root = empty_root();
        root.materials.push(specular_material());
        root.extensions_used = vec!["KHR_materials_specular".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_clearcoat —
    // docs/3d/gltf/extensions/KHR_materials_clearcoat.md.
    fn clearcoat_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_clearcoat: Some(MaterialClearcoat {
                    clearcoat_factor: Some(1.0),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_clearcoat_missing_used() {
        let mut root = empty_root();
        root.materials.push(clearcoat_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_clearcoat"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_clearcoat, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_clearcoat_declared() {
        let mut root = empty_root();
        root.materials.push(clearcoat_material());
        root.extensions_used = vec!["KHR_materials_clearcoat".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_sheen —
    // docs/3d/gltf/extensions/KHR_materials_sheen.md.
    fn sheen_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_sheen: Some(MaterialSheen {
                    sheen_color_factor: Some([0.9, 0.9, 0.9]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_sheen_missing_used() {
        let mut root = empty_root();
        root.materials.push(sheen_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_sheen"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_sheen, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_sheen_declared() {
        let mut root = empty_root();
        root.materials.push(sheen_material());
        root.extensions_used = vec!["KHR_materials_sheen".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_transmission —
    // docs/3d/gltf/extensions/KHR_materials_transmission.md.
    fn transmission_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_transmission: Some(MaterialTransmission {
                    transmission_factor: Some(0.8),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_transmission_missing_used() {
        let mut root = empty_root();
        root.materials.push(transmission_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_transmission"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_transmission, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_transmission_declared() {
        let mut root = empty_root();
        root.materials.push(transmission_material());
        root.extensions_used = vec!["KHR_materials_transmission".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_volume —
    // docs/3d/gltf/extensions/KHR_materials_volume.md.
    fn volume_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_volume: Some(MaterialVolume {
                    thickness_factor: Some(0.4),
                    attenuation_distance: Some(2.5),
                    attenuation_color: Some([0.7, 0.2, 0.3]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_volume_missing_used() {
        let mut root = empty_root();
        root.materials.push(volume_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_materials_volume"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_volume, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_volume_declared() {
        let mut root = empty_root();
        root.materials.push(volume_material());
        root.extensions_used = vec!["KHR_materials_volume".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_iridescence —
    // docs/3d/gltf/extensions/KHR_materials_iridescence.md.
    fn iridescence_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_iridescence: Some(MaterialIridescence {
                    iridescence_factor: Some(0.6),
                    iridescence_ior: Some(1.3),
                    iridescence_thickness_minimum: Some(100.0),
                    iridescence_thickness_maximum: Some(400.0),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_iridescence_missing_used() {
        let mut root = empty_root();
        root.materials.push(iridescence_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_iridescence"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_iridescence, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_iridescence_declared() {
        let mut root = empty_root();
        root.materials.push(iridescence_material());
        root.extensions_used = vec!["KHR_materials_iridescence".into()];
        validate_extension_stack(&root).unwrap();
    }

    // KHR_materials_anisotropy —
    // docs/3d/gltf/extensions/KHR_materials_anisotropy.md.
    fn anisotropy_material() -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_anisotropy: Some(MaterialAnisotropy {
                    anisotropy_strength: Some(0.6),
                    anisotropy_rotation: Some(1.57),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_anisotropy_missing_used() {
        let mut root = empty_root();
        root.materials.push(anisotropy_material());
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_anisotropy"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_anisotropy, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_anisotropy_declared() {
        let mut root = empty_root();
        root.materials.push(anisotropy_material());
        root.extensions_used = vec!["KHR_materials_anisotropy".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_rejects_anisotropy_strength_above_one() {
        let mut root = empty_root();
        let mat = Material {
            extensions: Some(MaterialExtensions {
                khr_materials_anisotropy: Some(MaterialAnisotropy {
                    anisotropy_strength: Some(1.5),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        root.materials.push(mat);
        root.extensions_used = vec!["KHR_materials_anisotropy".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackAnisotropyStrengthRange"),
            "expected ExtensionStackAnisotropyStrengthRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_anisotropy_strength_below_zero() {
        let mut root = empty_root();
        let mat = Material {
            extensions: Some(MaterialExtensions {
                khr_materials_anisotropy: Some(MaterialAnisotropy {
                    anisotropy_strength: Some(-0.1),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        root.materials.push(mat);
        root.extensions_used = vec!["KHR_materials_anisotropy".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackAnisotropyStrengthRange"),
            "expected ExtensionStackAnisotropyStrengthRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_anisotropy_rotation_not_finite() {
        let mut root = empty_root();
        let mat = Material {
            extensions: Some(MaterialExtensions {
                khr_materials_anisotropy: Some(MaterialAnisotropy {
                    anisotropy_rotation: Some(f32::NAN),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        root.materials.push(mat);
        root.extensions_used = vec!["KHR_materials_anisotropy".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackAnisotropyRotationFinite"),
            "expected ExtensionStackAnisotropyRotationFinite, got {msg}"
        );
    }

    // KHR_materials_dispersion —
    // docs/3d/gltf/extensions/KHR_materials_dispersion.md.
    fn dispersion_material(value: f32) -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_dispersion: Some(MaterialDispersion {
                    dispersion: Some(value),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_dispersion_missing_used() {
        let mut root = empty_root();
        root.materials.push(dispersion_material(0.5));
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_dispersion"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_dispersion, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_dispersion_declared() {
        let mut root = empty_root();
        root.materials.push(dispersion_material(0.5));
        root.extensions_used = vec!["KHR_materials_dispersion".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_accepts_dispersion_zero() {
        // Zero is the spec default and explicitly valid (means "no
        // dispersion").
        let mut root = empty_root();
        root.materials.push(dispersion_material(0.0));
        root.extensions_used = vec!["KHR_materials_dispersion".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_accepts_dispersion_above_one() {
        // The spec says values above 1.0 are valid for artists wanting
        // to exaggerate the effect (Rutile = 2.04 is the listed example).
        let mut root = empty_root();
        root.materials.push(dispersion_material(2.04));
        root.extensions_used = vec!["KHR_materials_dispersion".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_rejects_dispersion_negative() {
        let mut root = empty_root();
        root.materials.push(dispersion_material(-0.1));
        root.extensions_used = vec!["KHR_materials_dispersion".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDispersionRange"),
            "expected ExtensionStackDispersionRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_dispersion_not_finite() {
        let mut root = empty_root();
        root.materials.push(dispersion_material(f32::NAN));
        root.extensions_used = vec!["KHR_materials_dispersion".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDispersionRange"),
            "expected ExtensionStackDispersionRange, got {msg}"
        );
    }

    // KHR_materials_diffuse_transmission —
    // docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md.
    fn diffuse_transmission_material(factor: Option<f32>, color: Option<[f32; 3]>) -> Material {
        Material {
            extensions: Some(MaterialExtensions {
                khr_materials_diffuse_transmission: Some(MaterialDiffuseTransmission {
                    diffuse_transmission_factor: factor,
                    diffuse_transmission_color_factor: color,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extension_stack_rejects_diffuse_transmission_missing_used() {
        let mut root = empty_root();
        root.materials
            .push(diffuse_transmission_material(Some(0.25), None));
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackUsedNotDeclared")
                && msg.contains("KHR_materials_diffuse_transmission"),
            "expected ExtensionStackUsedNotDeclared for \
             KHR_materials_diffuse_transmission, got {msg}"
        );
    }

    #[test]
    fn extension_stack_accepts_diffuse_transmission_declared() {
        let mut root = empty_root();
        root.materials.push(diffuse_transmission_material(
            Some(0.25),
            Some([1.0, 0.9, 0.85]),
        ));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_accepts_diffuse_transmission_defaults_only() {
        // Spec defaults: factor = 0.0, color = [1, 1, 1]. Both must be
        // accepted.
        let mut root = empty_root();
        root.materials.push(diffuse_transmission_material(
            Some(0.0),
            Some([1.0, 1.0, 1.0]),
        ));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        validate_extension_stack(&root).unwrap();
    }

    #[test]
    fn extension_stack_rejects_diffuse_transmission_factor_above_one() {
        // Per the spec "A value of 1.0 indicates that 100% of the light
        // that penetrates the surface is transmitted through it." A
        // factor above 1.0 is non-sensical (you cannot transmit more
        // than the available light).
        let mut root = empty_root();
        root.materials
            .push(diffuse_transmission_material(Some(1.5), None));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDiffuseTransmissionFactorRange"),
            "expected ExtensionStackDiffuseTransmissionFactorRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_diffuse_transmission_factor_negative() {
        let mut root = empty_root();
        root.materials
            .push(diffuse_transmission_material(Some(-0.1), None));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDiffuseTransmissionFactorRange"),
            "expected ExtensionStackDiffuseTransmissionFactorRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_diffuse_transmission_factor_not_finite() {
        let mut root = empty_root();
        root.materials
            .push(diffuse_transmission_material(Some(f32::NAN), None));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDiffuseTransmissionFactorRange"),
            "expected ExtensionStackDiffuseTransmissionFactorRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_diffuse_transmission_color_negative() {
        let mut root = empty_root();
        root.materials
            .push(diffuse_transmission_material(None, Some([1.0, -0.1, 1.0])));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDiffuseTransmissionColorRange"),
            "expected ExtensionStackDiffuseTransmissionColorRange, got {msg}"
        );
    }

    #[test]
    fn extension_stack_rejects_diffuse_transmission_color_above_one() {
        let mut root = empty_root();
        root.materials
            .push(diffuse_transmission_material(None, Some([1.0, 1.0, 1.5])));
        root.extensions_used = vec!["KHR_materials_diffuse_transmission".into()];
        let err = validate_extension_stack(&root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("ExtensionStackDiffuseTransmissionColorRange"),
            "expected ExtensionStackDiffuseTransmissionColorRange, got {msg}"
        );
    }

    // --- Animation channel target-path validation ------------------

    fn float_scalar_accessor(count: u32) -> Accessor {
        Accessor {
            buffer_view: Some(0),
            byte_offset: None,
            component_type: COMPONENT_TYPE_FLOAT,
            count,
            kind: "SCALAR".into(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: None,
        }
    }

    fn float_vec3_accessor(count: u32) -> Accessor {
        Accessor {
            kind: "VEC3".into(),
            ..float_scalar_accessor(count)
        }
    }

    fn anim_with_path(
        path: &str,
        target_node: Option<u32>,
        sampler_input: u32,
        sampler_output: u32,
    ) -> Animation {
        Animation {
            channels: vec![AnimationChannel {
                sampler: 0,
                target: AnimationChannelTarget {
                    node: target_node,
                    path: path.into(),
                    extensions: None,
                },
            }],
            samplers: vec![AnimationSampler {
                input: sampler_input,
                output: sampler_output,
                interpolation: None,
            }],
            name: None,
            extras: None,
        }
    }

    #[test]
    fn animation_channels_accepts_translation() {
        let anim = anim_with_path("translation", Some(0), 0, 1);
        let nodes = vec![Node::default()];
        let meshes: Vec<Mesh> = vec![];
        let accessors = vec![float_scalar_accessor(2), float_vec3_accessor(2)];
        validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap();
    }

    #[test]
    fn animation_channels_rejects_unknown_path() {
        let anim = anim_with_path("zoom", Some(0), 0, 1);
        let nodes = vec![Node::default()];
        let meshes: Vec<Mesh> = vec![];
        let accessors = vec![float_scalar_accessor(2), float_vec3_accessor(2)];
        let err = validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap_err();
        assert!(format!("{err}").contains("AnimationChannelPath"));
    }

    #[test]
    fn animation_channels_rejects_out_of_range_sampler() {
        let mut anim = anim_with_path("translation", Some(0), 0, 1);
        anim.channels[0].sampler = 42;
        let nodes = vec![Node::default()];
        let meshes: Vec<Mesh> = vec![];
        let accessors = vec![float_scalar_accessor(2), float_vec3_accessor(2)];
        let err = validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap_err();
        assert!(format!("{err}").contains("AnimationChannelSampler"));
    }

    #[test]
    fn animation_channels_rejects_out_of_range_input_accessor() {
        let anim = anim_with_path("translation", Some(0), 9, 1);
        let nodes = vec![Node::default()];
        let meshes: Vec<Mesh> = vec![];
        let accessors = vec![float_scalar_accessor(2), float_vec3_accessor(2)];
        let err = validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap_err();
        assert!(format!("{err}").contains("AnimationChannelSamplerInput"));
    }

    #[test]
    fn animation_channels_rejects_weights_without_mesh() {
        let anim = anim_with_path("weights", Some(0), 0, 1);
        // Node 0 has no mesh.
        let nodes = vec![Node::default()];
        let meshes: Vec<Mesh> = vec![];
        let accessors = vec![float_scalar_accessor(2), float_scalar_accessor(2)];
        let err = validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap_err();
        assert!(format!("{err}").contains("AnimationChannelWeightsNoMesh"));
    }

    #[test]
    fn animation_channels_rejects_weights_without_targets() {
        let anim = anim_with_path("weights", Some(0), 0, 1);
        let nodes = vec![Node {
            mesh: Some(0),
            ..Default::default()
        }];
        let meshes = vec![Mesh {
            // primitive WITHOUT morph targets
            primitives: vec![Primitive {
                attributes: HashMap::new(),
                indices: None,
                material: None,
                mode: None,
                targets: vec![],
                extensions: None,
                extras: None,
            }],
            name: None,
            weights: None,
            extensions: None,
            extras: None,
        }];
        let accessors = vec![float_scalar_accessor(2), float_scalar_accessor(2)];
        let err = validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap_err();
        assert!(format!("{err}").contains("AnimationChannelWeightsNoTargets"));
    }

    #[test]
    fn animation_channels_accepts_weights_with_targets() {
        let anim = anim_with_path("weights", Some(0), 0, 1);
        let nodes = vec![Node {
            mesh: Some(0),
            ..Default::default()
        }];
        let mut target_map: HashMap<String, u32> = HashMap::new();
        target_map.insert("POSITION".into(), 0);
        let meshes = vec![Mesh {
            primitives: vec![Primitive {
                attributes: HashMap::new(),
                indices: None,
                material: None,
                mode: None,
                targets: vec![target_map],
                extensions: None,
                extras: None,
            }],
            name: None,
            weights: None,
            extensions: None,
            extras: None,
        }];
        let accessors = vec![float_scalar_accessor(2), float_scalar_accessor(2)];
        validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap();
    }

    #[test]
    fn animation_channels_skips_weights_with_no_target_node() {
        // Spec §3.11 — channel with no node is ignored; validator
        // must follow suit even for `path=weights`.
        let anim = anim_with_path("weights", None, 0, 1);
        let nodes = vec![Node::default()];
        let meshes: Vec<Mesh> = vec![];
        let accessors = vec![float_scalar_accessor(2), float_scalar_accessor(2)];
        validate_animation_channels(0, &anim, &nodes, &meshes, &accessors).unwrap();
    }

    // --- Asset version / minVersion ---------------------------------

    fn asset_with(version: &str, min_version: Option<&str>) -> Asset {
        Asset {
            version: version.to_owned(),
            generator: None,
            copyright: None,
            min_version: min_version.map(str::to_owned),
            extensions: None,
            extras: None,
        }
    }

    #[test]
    fn asset_version_accepts_2_0() {
        check_asset_version(&asset_with("2.0", None)).unwrap();
    }

    #[test]
    fn asset_version_accepts_2_1_forward() {
        // Spec §3.2 explicitly allows clients to load a 2.1 asset as
        // long as it doesn't carry a minVersion forcing 2.1 features.
        check_asset_version(&asset_with("2.1", None)).unwrap();
    }

    #[test]
    fn asset_version_rejects_3_0() {
        let err = check_asset_version(&asset_with("3.0", None)).unwrap_err();
        assert!(format!("{err}").contains("AssetVersionUnsupported"));
    }

    #[test]
    fn asset_version_rejects_1_0_major_mismatch() {
        let err = check_asset_version(&asset_with("1.0", None)).unwrap_err();
        assert!(format!("{err}").contains("AssetVersionUnsupported"));
    }

    #[test]
    fn asset_version_rejects_malformed() {
        for bad in ["", "2", "2.", ".0", "2.0.1", "v2.0", "2.0 ", " 2.0", "a.b"] {
            let err = check_asset_version(&asset_with(bad, None)).unwrap_err();
            assert!(
                format!("{err}").contains("AssetVersionFormat"),
                "expected AssetVersionFormat for {bad:?}, got {err}"
            );
        }
    }

    #[test]
    fn asset_min_version_accepts_when_le_version() {
        check_asset_version(&asset_with("2.0", Some("2.0"))).unwrap();
        check_asset_version(&asset_with("2.1", Some("2.0"))).unwrap();
    }

    #[test]
    fn asset_min_version_rejects_when_greater_than_version() {
        // 2.1 > 2.0 — spec §5.9.4 MUST.
        let err = check_asset_version(&asset_with("2.0", Some("2.1"))).unwrap_err();
        assert!(format!("{err}").contains("AssetMinVersionGreaterThanVersion"));
    }

    #[test]
    fn asset_min_version_rejects_beyond_supported() {
        // version is 2.5 (we accept any 2.x for version), but minVersion
        // 2.5 demands 2.5 features we don't implement.
        let err = check_asset_version(&asset_with("2.5", Some("2.5"))).unwrap_err();
        assert!(format!("{err}").contains("AssetMinVersionUnsupported"));
    }

    #[test]
    fn asset_min_version_rejects_malformed() {
        let err = check_asset_version(&asset_with("2.0", Some("2"))).unwrap_err();
        assert!(format!("{err}").contains("AssetMinVersionFormat"));
    }

    #[test]
    fn asset_version_parser_accepts_multi_digit() {
        // No artificial cap on digit count — JSON schema pattern is
        // `^[0-9]+\.[0-9]+$`. Reject only on the version-policy step.
        check_asset_version(&asset_with("2.42", None)).unwrap();
    }

    // Silence unused-import warnings on `AccessorSparse*`.
    #[test]
    fn _unused_imports_silenced() {
        let _ = AccessorSparse {
            count: 0,
            indices: AccessorSparseIndices {
                buffer_view: 0,
                byte_offset: None,
                component_type: COMPONENT_TYPE_UNSIGNED_BYTE,
            },
            values: AccessorSparseValues {
                buffer_view: 0,
                byte_offset: None,
            },
        };
    }

    // --- Accessor fit in bufferView (round 8) -----------------------

    fn bv(byte_length: u32) -> BufferView {
        BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length,
            byte_stride: None,
            target: None,
            name: None,
            extensions: None,
        }
    }

    fn bv_strided(byte_length: u32, stride: u32) -> BufferView {
        BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length,
            byte_stride: Some(stride),
            target: None,
            name: None,
            extensions: None,
        }
    }

    #[test]
    fn accessor_fit_accepts_tight_pack() {
        // 3 VEC3 floats = 3 * 12 = 36 bytes.
        let acc = vec3_float_accessor(0, 3, 0);
        let bvs = vec![bv(36)];
        validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap();
    }

    #[test]
    fn accessor_fit_rejects_overflow_tight_pack() {
        // 3 VEC3 floats = 36 bytes; bufferView has only 35.
        let acc = vec3_float_accessor(0, 3, 0);
        let bvs = vec![bv(35)];
        let err = validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitBufferView"));
    }

    #[test]
    fn accessor_fit_accepts_strided() {
        // stride 16, 3 VEC3 floats: last_element_start = 0 + 16*2 = 32,
        // end = 32 + 12 = 44. Need bufferView >= 44.
        let acc = vec3_float_accessor(0, 3, 0);
        let bvs = vec![bv_strided(44, 16)];
        validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap();
    }

    #[test]
    fn accessor_fit_rejects_strided_short_by_one() {
        let acc = vec3_float_accessor(0, 3, 0);
        let bvs = vec![bv_strided(43, 16)];
        let err = validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitBufferView"));
    }

    #[test]
    fn accessor_fit_rejects_stride_smaller_than_element() {
        // VEC3 float = 12 bytes; stride 8 is smaller than element.
        // (validate_alignment + JSON-schema range catch this too; the
        // fit check independently flags it for callers that don't run
        // the alignment validator first.)
        let acc = vec3_float_accessor(0, 3, 0);
        let bvs = vec![bv_strided(64, 8)];
        let err = validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitStride"));
    }

    #[test]
    fn accessor_fit_skips_when_no_bufferview() {
        // Pure-zero / sparse-only accessor: no bufferView, nothing to
        // fit-check (the spec's MUST is conditional on the reference).
        let mut acc = vec3_float_accessor(0, 3, 0);
        acc.buffer_view = None;
        // Empty bv list: still OK because we skip.
        validate_accessor_fits_bufferview(0, &acc, &[]).unwrap();
    }

    #[test]
    fn accessor_fit_skips_when_count_zero() {
        // Pathological but allowed by §3.6.2; nothing to bound.
        let acc = vec3_float_accessor(0, 0, 0);
        let bvs = vec![bv(0)];
        validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap();
    }

    #[test]
    fn accessor_fit_rejects_with_byte_offset() {
        // 1 VEC3 float at byteOffset 24 in a 32-byte bufferView would
        // need 24 + 12 = 36 bytes; only 32 available.
        let acc = vec3_float_accessor(24, 1, 0);
        let bvs = vec![bv(32)];
        let err = validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitBufferView"));
    }

    #[test]
    fn accessor_fit_rejects_unknown_component_type() {
        let mut acc = vec3_float_accessor(0, 3, 0);
        acc.component_type = 9999;
        let bvs = vec![bv(64)];
        let err = validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitComponentType"));
    }

    #[test]
    fn accessor_fit_rejects_unknown_element_type() {
        let mut acc = vec3_float_accessor(0, 3, 0);
        acc.kind = "VECTOR_OF_QUATERNIONS".into();
        let bvs = vec![bv(64)];
        let err = validate_accessor_fits_bufferview(0, &acc, &bvs).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitElementType"));
    }

    #[test]
    fn accessor_fit_rejects_out_of_range_bufferview() {
        let acc = vec3_float_accessor(0, 3, 99);
        let err = validate_accessor_fits_bufferview(0, &acc, &[]).unwrap_err();
        assert!(format!("{err}").contains("AccessorFitBufferView"));
    }

    // --- BufferView fit in buffer (round 8) -------------------------

    fn buffer_of(byte_length: u32) -> Buffer {
        Buffer {
            byte_length,
            uri: None,
            name: None,
            extensions: None,
        }
    }

    #[test]
    fn bufferview_fit_accepts_exact() {
        let bv = BufferView {
            buffer: 0,
            byte_offset: Some(100),
            byte_length: 200,
            byte_stride: None,
            target: None,
            name: None,
            extensions: None,
        };
        validate_bufferview_fits_buffer(0, &bv, &[buffer_of(300)]).unwrap();
    }

    #[test]
    fn bufferview_fit_rejects_overrun() {
        let bv = BufferView {
            buffer: 0,
            byte_offset: Some(100),
            byte_length: 250,
            byte_stride: None,
            target: None,
            name: None,
            extensions: None,
        };
        let err = validate_bufferview_fits_buffer(0, &bv, &[buffer_of(300)]).unwrap_err();
        assert!(format!("{err}").contains("BufferViewFitBuffer"));
    }

    #[test]
    fn bufferview_fit_rejects_out_of_range_buffer() {
        let bv = BufferView {
            buffer: 7,
            byte_offset: Some(0),
            byte_length: 8,
            byte_stride: None,
            target: None,
            name: None,
            extensions: None,
        };
        let err = validate_bufferview_fits_buffer(0, &bv, &[buffer_of(300)]).unwrap_err();
        assert!(format!("{err}").contains("BufferViewFitBuffer"));
    }

    #[test]
    fn bufferview_stride_rejects_too_small() {
        let bv = BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length: 64,
            byte_stride: Some(2),
            target: None,
            name: None,
            extensions: None,
        };
        let err = validate_bufferview_fits_buffer(0, &bv, &[buffer_of(64)]).unwrap_err();
        assert!(format!("{err}").contains("BufferViewStrideRange"));
    }

    #[test]
    fn bufferview_stride_rejects_too_large() {
        let bv = BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length: 256,
            byte_stride: Some(256),
            target: None,
            name: None,
            extensions: None,
        };
        let err = validate_bufferview_fits_buffer(0, &bv, &[buffer_of(256)]).unwrap_err();
        assert!(format!("{err}").contains("BufferViewStrideRange"));
    }

    #[test]
    fn bufferview_stride_accepts_boundary() {
        // 4 and 252 are the inclusive endpoints from §5.11.4.
        for s in [4u32, 16, 252] {
            let bv = BufferView {
                buffer: 0,
                byte_offset: Some(0),
                byte_length: 1024,
                byte_stride: Some(s),
                target: None,
                name: None,
                extensions: None,
            };
            validate_bufferview_fits_buffer(0, &bv, &[buffer_of(1024)]).unwrap();
        }
    }

    // --- Sparse-indices bufferView restrictions (round 8) -----------

    fn sparse_acc(indices_bv: u32) -> Accessor {
        Accessor {
            buffer_view: None,
            byte_offset: None,
            component_type: COMPONENT_TYPE_FLOAT,
            count: 4,
            kind: "VEC3".into(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: Some(AccessorSparse {
                count: 2,
                indices: AccessorSparseIndices {
                    buffer_view: indices_bv,
                    byte_offset: None,
                    component_type: COMPONENT_TYPE_UNSIGNED_BYTE,
                },
                values: AccessorSparseValues {
                    buffer_view: 0,
                    byte_offset: None,
                },
            }),
        }
    }

    #[test]
    fn sparse_indices_bv_accepts_plain() {
        let accs = vec![sparse_acc(0)];
        let bvs = vec![bv(64)];
        validate_sparse_indices_buffer_views(&accs, &bvs).unwrap();
    }

    #[test]
    fn sparse_indices_bv_rejects_target() {
        let accs = vec![sparse_acc(0)];
        let mut bvs = vec![bv(64)];
        bvs[0].target = Some(34962);
        let err = validate_sparse_indices_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseIndicesBufferViewTarget"));
    }

    #[test]
    fn sparse_indices_bv_rejects_stride() {
        let accs = vec![sparse_acc(0)];
        let mut bvs = vec![bv(64)];
        bvs[0].byte_stride = Some(4);
        let err = validate_sparse_indices_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseIndicesBufferViewStride"));
    }

    #[test]
    fn sparse_indices_bv_rejects_out_of_range() {
        let accs = vec![sparse_acc(7)];
        let bvs = vec![bv(64)];
        let err = validate_sparse_indices_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseIndicesBufferViewIndex"));
    }

    #[test]
    fn sparse_indices_bv_skips_non_sparse_accessors() {
        let acc = vec3_float_accessor(0, 3, 0); // no sparse field
        validate_sparse_indices_buffer_views(&[acc], &[]).unwrap();
    }

    // --- Sparse-values bufferView restrictions (spec §5.4.1, round 256) -----

    fn sparse_acc_values(values_bv: u32) -> Accessor {
        Accessor {
            buffer_view: None,
            byte_offset: None,
            component_type: COMPONENT_TYPE_FLOAT,
            count: 4,
            kind: "VEC3".into(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: Some(AccessorSparse {
                count: 2,
                indices: AccessorSparseIndices {
                    buffer_view: 0,
                    byte_offset: None,
                    component_type: COMPONENT_TYPE_UNSIGNED_BYTE,
                },
                values: AccessorSparseValues {
                    buffer_view: values_bv,
                    byte_offset: None,
                },
            }),
        }
    }

    #[test]
    fn sparse_values_bv_accepts_plain() {
        let accs = vec![sparse_acc_values(0)];
        let bvs = vec![bv(64)];
        validate_sparse_values_buffer_views(&accs, &bvs).unwrap();
    }

    #[test]
    fn sparse_values_bv_rejects_target() {
        let accs = vec![sparse_acc_values(0)];
        let mut bvs = vec![bv(64)];
        // ARRAY_BUFFER target — spec §5.4.1 says the values bufferView
        // MUST NOT define `target`.
        bvs[0].target = Some(34962);
        let err = validate_sparse_values_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseValuesBufferViewTarget"));
    }

    #[test]
    fn sparse_values_bv_rejects_element_array_target() {
        // ELEMENT_ARRAY_BUFFER target — same rule, different value, to
        // lock in both target sentinels.
        let accs = vec![sparse_acc_values(0)];
        let mut bvs = vec![bv(64)];
        bvs[0].target = Some(34963);
        let err = validate_sparse_values_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseValuesBufferViewTarget"));
    }

    #[test]
    fn sparse_values_bv_rejects_stride() {
        let accs = vec![sparse_acc_values(0)];
        let mut bvs = vec![bv(64)];
        bvs[0].byte_stride = Some(4);
        let err = validate_sparse_values_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseValuesBufferViewStride"));
    }

    #[test]
    fn sparse_values_bv_rejects_out_of_range() {
        // values.bufferView = 7 against a buffer-views vec of length 1.
        let accs = vec![sparse_acc_values(7)];
        let bvs = vec![bv(64)];
        let err = validate_sparse_values_buffer_views(&accs, &bvs).unwrap_err();
        assert!(format!("{err}").contains("SparseValuesBufferViewIndex"));
    }

    #[test]
    fn sparse_values_bv_skips_non_sparse_accessors() {
        // Accessor without a sparse block must never be inspected.
        let acc = vec3_float_accessor(0, 3, 0);
        validate_sparse_values_buffer_views(&[acc], &[]).unwrap();
    }

    #[test]
    fn sparse_values_bv_independent_of_indices_bv() {
        // values.bufferView resolves to a clean bufferView while the
        // indices.bufferView (also at slot 0) carries a stride. The
        // values-only validator MUST NOT flag the stride on the indices
        // bufferView — that's the §5.3.1 validator's job. We give the
        // sparse accessor a clean bv at slot 1 for `values` so this
        // validator passes; the indices-side rule is exercised separately.
        let mut accs = vec![sparse_acc_values(1)];
        accs[0].sparse.as_mut().unwrap().indices.buffer_view = 0;
        let mut bvs = vec![bv(64), bv(64)];
        bvs[0].byte_stride = Some(8); // dirties indices bv, not values bv
        validate_sparse_values_buffer_views(&accs, &bvs).unwrap();
    }

    // --- §5.12–§5.14 camera property validation ------------------------

    fn persp(aspect_ratio: Option<f32>, yfov: f32, znear: f32, zfar: Option<f32>) -> Camera {
        Camera {
            kind: "perspective".to_owned(),
            perspective: Some(crate::json_model::CameraPerspective {
                aspect_ratio,
                yfov,
                znear,
                zfar,
            }),
            orthographic: None,
            name: None,
        }
    }

    fn ortho(xmag: f32, ymag: f32, znear: f32, zfar: f32) -> Camera {
        Camera {
            kind: "orthographic".to_owned(),
            perspective: None,
            orthographic: Some(crate::json_model::CameraOrthographic {
                xmag,
                ymag,
                znear,
                zfar,
            }),
            name: None,
        }
    }

    #[test]
    fn cameras_accept_valid_documents() {
        // Perspective with + without optional fields, orthographic with
        // znear exactly 0 (§5.13.4 minimum is >= 0), negative xmag
        // (SHOULD NOT, not MUST NOT), yfov above π (SHOULD, not MUST).
        validate_cameras(&[
            persp(Some(16.0 / 9.0), 1.0, 0.1, Some(100.0)),
            persp(None, 4.0, 0.05, None),
            ortho(5.0, 3.0, 0.0, 50.0),
            ortho(-5.0, -3.0, 0.1, 50.0),
        ])
        .unwrap();
        validate_cameras(&[]).unwrap();
    }

    #[test]
    fn cameras_reject_both_projections() {
        let mut cam = persp(None, 1.0, 0.1, None);
        cam.orthographic = Some(crate::json_model::CameraOrthographic {
            xmag: 1.0,
            ymag: 1.0,
            znear: 0.1,
            zfar: 10.0,
        });
        let err = validate_cameras(&[cam]).unwrap_err();
        assert!(format!("{err}").contains("CameraProjectionExclusive"));
    }

    #[test]
    fn cameras_reject_zero_or_nan_magnification() {
        let err = validate_cameras(&[ortho(0.0, 1.0, 0.1, 10.0)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicXmag"));
        let err = validate_cameras(&[ortho(1.0, 0.0, 0.1, 10.0)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicYmag"));
        let err = validate_cameras(&[ortho(f32::NAN, 1.0, 0.1, 10.0)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicXmag"));
    }

    #[test]
    fn cameras_reject_orthographic_z_violations() {
        // znear < 0 breaks the §5.13.4 schema minimum.
        let err = validate_cameras(&[ortho(1.0, 1.0, -0.5, 10.0)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicZnear"));
        // zfar == 0 is the explicit MUST NOT of §5.13.3.
        let err = validate_cameras(&[ortho(1.0, 1.0, 0.0, 0.0)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicZfar"));
        // zfar <= znear breaks "zfar MUST be greater than znear".
        let err = validate_cameras(&[ortho(1.0, 1.0, 5.0, 5.0)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicZRange"));
        // Non-finite zfar would dodge every comparison via NaN.
        let err = validate_cameras(&[ortho(1.0, 1.0, 0.1, f32::NAN)]).unwrap_err();
        assert!(format!("{err}").contains("CameraOrthographicZfar"));
    }

    #[test]
    fn cameras_reject_perspective_violations() {
        // yfov MUST be > 0 (§5.14.2).
        let err = validate_cameras(&[persp(None, 0.0, 0.1, None)]).unwrap_err();
        assert!(format!("{err}").contains("CameraPerspectiveYfov"));
        // znear MUST be > 0 (§5.14.4) — zero is invalid here, unlike
        // the orthographic camera where the schema minimum is >= 0.
        let err = validate_cameras(&[persp(None, 1.0, 0.0, None)]).unwrap_err();
        assert!(format!("{err}").contains("CameraPerspectiveZnear"));
        // aspectRatio, when defined, MUST be > 0 (§5.14.1).
        let err = validate_cameras(&[persp(Some(0.0), 1.0, 0.1, None)]).unwrap_err();
        assert!(format!("{err}").contains("CameraPerspectiveAspectRatio"));
        // zfar, when defined, MUST be > 0 (§5.14.3) …
        let err = validate_cameras(&[persp(None, 1.0, 0.1, Some(-1.0))]).unwrap_err();
        assert!(format!("{err}").contains("CameraPerspectiveZfar"));
        // … and MUST be greater than znear.
        let err = validate_cameras(&[persp(None, 1.0, 2.0, Some(2.0))]).unwrap_err();
        assert!(format!("{err}").contains("CameraPerspectiveZRange"));
        // NaN yfov must not slip through the comparisons.
        let err = validate_cameras(&[persp(None, f32::NAN, 0.1, None)]).unwrap_err();
        assert!(format!("{err}").contains("CameraPerspectiveYfov"));
    }

    fn node_with_matrix(m: [f32; 16]) -> Node {
        Node {
            matrix: Some(m),
            ..Default::default()
        }
    }

    #[test]
    fn nodes_accept_invertible_shear_matrix() {
        // §3.5.3 — the determinant test rejects only non-invertible
        // matrices. A shear (off-diagonal upper-left term) is an
        // Implementation Note "SHOULD NOT", not a MUST, so an
        // invertible shear matrix is accepted.
        let mut m = [0.0f32; 16];
        // identity …
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        // … plus a shear term in column 1 / row 0 (non-zero det = 1).
        m[4] = 0.5;
        assert!(validate_nodes(&[node_with_matrix(m)], &[]).is_ok());
    }

    #[test]
    fn nodes_reject_long_parent_chain_cycle() {
        // §3.5.2 — a 4-node cycle 0->1->2->3->0. The closing edge gives
        // node 0 a second parent, so the multiple-parents guard fires
        // first; either way it must be rejected.
        let chain = |child: u32| Node {
            children: vec![child],
            ..Default::default()
        };
        let nodes = vec![chain(1), chain(2), chain(3), chain(0)];
        let err = validate_nodes(&nodes, &[]).unwrap_err();
        let m = format!("{err}");
        assert!(
            m.contains("NodeHierarchyCycle") || m.contains("NodeMultipleParents"),
            "{m}"
        );
    }

    #[test]
    fn nodes_reject_non_finite_translation() {
        // §3.5.3 — transform components MUST be finite.
        let node = Node {
            translation: Some([f32::INFINITY, 0.0, 0.0]),
            ..Default::default()
        };
        let err = validate_nodes(&[node], &[]).unwrap_err();
        assert!(format!("{err}").contains("NodeTranslationFinite"));
    }

    #[test]
    fn nodes_accept_deep_strict_tree() {
        // A 4-level linear chain (0->1->2->3) is a valid strict tree.
        let n = vec![
            Node {
                children: vec![1],
                ..Default::default()
            },
            Node {
                children: vec![2],
                ..Default::default()
            },
            Node {
                children: vec![3],
                ..Default::default()
            },
            Node::default(),
        ];
        assert!(validate_nodes(&n, &[]).is_ok());
    }

    // --- §5.26 sampler filter / wrap mode validation ---

    use crate::json_model::{
        Sampler, MAG_FILTER_LINEAR, MAG_FILTER_NEAREST, MIN_FILTER_LINEAR_MIPMAP_LINEAR,
        WRAP_CLAMP_TO_EDGE, WRAP_MIRRORED_REPEAT, WRAP_REPEAT,
    };

    #[test]
    fn samplers_accept_all_legal_enum_values() {
        // Every enumerated combination from §5.26.1–§5.26.4 is valid.
        let s = vec![
            Sampler {
                mag_filter: Some(MAG_FILTER_NEAREST),
                min_filter: Some(MIN_FILTER_LINEAR_MIPMAP_LINEAR),
                wrap_s: Some(WRAP_CLAMP_TO_EDGE),
                wrap_t: Some(WRAP_MIRRORED_REPEAT),
                name: None,
            },
            Sampler {
                mag_filter: Some(MAG_FILTER_LINEAR),
                min_filter: Some(9984), // NEAREST_MIPMAP_NEAREST
                wrap_s: Some(WRAP_REPEAT),
                wrap_t: Some(WRAP_REPEAT),
                name: None,
            },
        ];
        assert!(validate_samplers(&s).is_ok());
    }

    #[test]
    fn samplers_accept_all_absent_properties() {
        // A sampler with no filter/wrap properties is conformant — wrapS/
        // wrapT default to REPEAT, filters are implementation choice.
        let s = vec![Sampler::default()];
        assert!(validate_samplers(&s).is_ok());
    }

    #[test]
    fn samplers_reject_bad_mag_filter() {
        let s = vec![Sampler {
            mag_filter: Some(9987), // a minFilter-only mipmap value
            ..Default::default()
        }];
        let err = validate_samplers(&s).unwrap_err();
        assert!(format!("{err}").contains("SamplerMagFilter"));
    }

    #[test]
    fn samplers_reject_bad_min_filter() {
        let s = vec![Sampler {
            min_filter: Some(9999),
            ..Default::default()
        }];
        let err = validate_samplers(&s).unwrap_err();
        assert!(format!("{err}").contains("SamplerMinFilter"));
    }

    #[test]
    fn samplers_reject_bad_wrap_s() {
        let s = vec![Sampler {
            wrap_s: Some(0),
            ..Default::default()
        }];
        let err = validate_samplers(&s).unwrap_err();
        assert!(format!("{err}").contains("SamplerWrapS"));
    }

    #[test]
    fn samplers_reject_bad_wrap_t() {
        let s = vec![Sampler {
            wrap_t: Some(33072), // off-by-one from CLAMP_TO_EDGE
            ..Default::default()
        }];
        let err = validate_samplers(&s).unwrap_err();
        assert!(format!("{err}").contains("SamplerWrapT"));
    }

    #[test]
    fn samplers_reject_mag_filter_mipmap_value() {
        // §5.26.1: magFilter has only NEAREST / LINEAR — the mipmap
        // combinations are minFilter-only and MUST be rejected here.
        for v in [9984u32, 9985, 9986, 9987] {
            let s = vec![Sampler {
                mag_filter: Some(v),
                ..Default::default()
            }];
            assert!(
                validate_samplers(&s).is_err(),
                "magFilter {v} should be rejected"
            );
        }
    }

    // --- Core accessor property validation (round r311) -------------

    fn bare_accessor(component_type: u32, count: u32, kind: &str) -> Accessor {
        Accessor {
            buffer_view: Some(0),
            byte_offset: Some(0),
            component_type,
            count,
            kind: kind.to_owned(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse: None,
        }
    }

    #[test]
    fn accessors_accept_conformant_entries() {
        let mut a = bare_accessor(COMPONENT_TYPE_FLOAT, 3, "VEC3");
        a.min = Some(vec![0.0, 0.0, 0.0]);
        a.max = Some(vec![1.0, 1.0, 1.0]);
        // Normalized signed byte is allowed (only FLOAT / UINT are barred).
        let mut b = bare_accessor(crate::json_model::COMPONENT_TYPE_BYTE, 4, "VEC4");
        b.normalized = true;
        // UNSIGNED_INT left un-normalized.
        let c = bare_accessor(COMPONENT_TYPE_UNSIGNED_INT, 6, "SCALAR");
        validate_accessors(&[a, b, c]).unwrap();
    }

    #[test]
    fn accessors_reject_zero_count() {
        let a = bare_accessor(COMPONENT_TYPE_FLOAT, 0, "VEC3");
        let err = validate_accessors(&[a]).unwrap_err();
        assert!(format!("{err}").contains("AccessorCount"));
    }

    #[test]
    fn accessors_reject_normalized_float() {
        let mut a = bare_accessor(COMPONENT_TYPE_FLOAT, 3, "VEC3");
        a.normalized = true;
        let err = validate_accessors(&[a]).unwrap_err();
        assert!(format!("{err}").contains("AccessorNormalizedComponentType"));
    }

    #[test]
    fn accessors_reject_normalized_unsigned_int() {
        let mut a = bare_accessor(COMPONENT_TYPE_UNSIGNED_INT, 3, "SCALAR");
        a.normalized = true;
        let err = validate_accessors(&[a]).unwrap_err();
        assert!(format!("{err}").contains("AccessorNormalizedComponentType"));
    }

    #[test]
    fn accessors_reject_short_min_array() {
        let mut a = bare_accessor(COMPONENT_TYPE_FLOAT, 3, "VEC3");
        a.min = Some(vec![0.0, 0.0]); // 2 != 3 components
        let err = validate_accessors(&[a]).unwrap_err();
        assert!(format!("{err}").contains("AccessorMinMaxLength"));
    }

    #[test]
    fn accessors_reject_long_max_array() {
        let mut a = bare_accessor(COMPONENT_TYPE_FLOAT, 3, "VEC3");
        a.max = Some(vec![1.0, 1.0, 1.0, 1.0]); // 4 != 3 components
        let err = validate_accessors(&[a]).unwrap_err();
        assert!(format!("{err}").contains("AccessorMinMaxLength"));
    }

    #[test]
    fn accessors_mat4_bounds_length_is_sixteen() {
        // §3.6.2.5 component count for MAT4 is 16, not 4.
        let mut a = bare_accessor(COMPONENT_TYPE_FLOAT, 1, "MAT4");
        a.min = Some(vec![0.0; 16]);
        a.max = Some(vec![1.0; 16]);
        validate_accessors(std::slice::from_ref(&a)).unwrap();
        // A 4-entry bounds array for MAT4 is wrong.
        a.min = Some(vec![0.0; 4]);
        let err = validate_accessors(&[a]).unwrap_err();
        assert!(format!("{err}").contains("AccessorMinMaxLength"));
    }

    #[test]
    fn accessors_skip_bounds_check_for_unknown_type() {
        // Unknown `type` defers to the bufferView-fit pass; the bounds
        // rule does not fire (no component count to compare against).
        let mut a = bare_accessor(COMPONENT_TYPE_FLOAT, 1, "WEIRD");
        a.min = Some(vec![0.0, 0.0]);
        validate_accessors(&[a]).unwrap();
    }

    // --- §5.28 + §3.7.3 + §5.25.3 skin validation ---

    use crate::json_model::{Scene, Skin};

    /// `n` nodes laid out as a single chain 0 -> 1 -> ... -> n-1 (each
    /// node parents the next), so any subset of them shares root node 0.
    fn chain_nodes(n: usize) -> Vec<Node> {
        let mut nodes = vec![Node::default(); n];
        for (i, node) in nodes.iter_mut().enumerate().take(n.saturating_sub(1)) {
            node.children = vec![(i + 1) as u32];
        }
        nodes
    }

    fn one_scene(roots: &[u32]) -> Vec<Scene> {
        vec![Scene {
            nodes: roots.to_vec(),
            ..Default::default()
        }]
    }

    #[test]
    fn skin_ibm_normalized_rejected() {
        // A MAT4 IBM accessor that is `normalized` — defence-in-depth
        // branch unreachable via the end-to-end path (FLOAT+normalized
        // is caught earlier), exercised directly here.
        let nodes = chain_nodes(2);
        let mut acc = bare_accessor(COMPONENT_TYPE_FLOAT, 2, "MAT4");
        acc.normalized = true;
        let skin = Skin {
            inverse_bind_matrices: Some(0),
            joints: vec![0, 1],
            ..Default::default()
        };
        let err = validate_skins(&[skin], &nodes, &[acc], &one_scene(&[0])).unwrap_err();
        assert!(format!("{err}").contains("SkinIbmAccessorNormalized"));
    }

    #[test]
    fn skin_well_formed_chain_passes() {
        let nodes = chain_nodes(3);
        let acc = bare_accessor(COMPONENT_TYPE_FLOAT, 3, "MAT4");
        let skin = Skin {
            inverse_bind_matrices: Some(0),
            skeleton: Some(0),
            joints: vec![0, 1, 2],
            ..Default::default()
        };
        validate_skins(&[skin], &nodes, &[acc], &one_scene(&[0])).unwrap();
    }

    #[test]
    fn skin_joints_empty_rejected_unit() {
        let skin = Skin {
            joints: vec![],
            ..Default::default()
        };
        let err = validate_skins(&[skin], &chain_nodes(1), &[], &one_scene(&[0])).unwrap_err();
        assert!(format!("{err}").contains("SkinJointsEmpty"));
    }

    #[test]
    fn skin_no_ibm_is_optional() {
        // inverseBindMatrices is OPTIONAL (§5.28.1); a skin without it
        // and without a skeleton is valid.
        let nodes = chain_nodes(2);
        let skin = Skin {
            joints: vec![0, 1],
            ..Default::default()
        };
        validate_skins(&[skin], &nodes, &[], &one_scene(&[0])).unwrap();
    }

    // --- §5.29 + §5.30 texture / material reference validation ---

    use crate::json_model::{Image, PbrMetallicRoughness, Texture as JmTexture};

    #[test]
    fn texture_source_out_of_range_rejected_unit() {
        let tex = JmTexture {
            source: Some(0),
            ..Default::default()
        };
        let err = validate_textures(&[tex], &[], &[], &[]).unwrap_err();
        assert!(format!("{err}").contains("TextureSourceIndex"));
    }

    #[test]
    fn texture_sampler_out_of_range_rejected_unit() {
        let tex = JmTexture {
            source: Some(0),
            sampler: Some(1),
            ..Default::default()
        };
        let images = vec![Image::default()];
        let err = validate_textures(&[tex], &images, &[], &[]).unwrap_err();
        assert!(format!("{err}").contains("TextureSamplerIndex"));
    }

    #[test]
    fn material_texture_index_out_of_range_rejected_unit() {
        let mat = Material {
            pbr_metallic_roughness: Some(PbrMetallicRoughness {
                base_color_texture: Some(crate::json_model::TextureInfo {
                    index: 5,
                    tex_coord: None,
                    extensions: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        // No textures declared → index 5 is out of range.
        let err = validate_textures(&[], &[], &[], &[mat]).unwrap_err();
        assert!(format!("{err}").contains("MaterialTextureIndex"));
    }

    #[test]
    fn well_formed_texture_references_pass_unit() {
        let images = vec![Image::default()];
        let samplers = vec![Sampler::default()];
        let tex = JmTexture {
            source: Some(0),
            sampler: Some(0),
            ..Default::default()
        };
        let mat = Material {
            pbr_metallic_roughness: Some(PbrMetallicRoughness {
                base_color_texture: Some(crate::json_model::TextureInfo {
                    index: 0,
                    tex_coord: None,
                    extensions: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        validate_textures(&[tex], &images, &samplers, &[mat]).unwrap();
    }

    #[test]
    fn texture_transform_non_finite_rotation_rejected() {
        use crate::json_model::TextureTransform;
        let t = TextureTransform {
            offset: None,
            rotation: Some(f32::NAN),
            scale: None,
            tex_coord: None,
        };
        let err = validate_texture_transform(0, "emissiveTexture", &t).unwrap_err();
        assert!(
            format!("{err}").contains("ExtensionStackTextureTransformRotationFinite"),
            "NaN rotation rejected, got {err}"
        );
        let t = TextureTransform {
            rotation: Some(f32::INFINITY),
            ..t
        };
        let err = validate_texture_transform(0, "emissiveTexture", &t).unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackTextureTransformRotationFinite"));
    }

    #[test]
    fn texture_transform_non_finite_offset_scale_rejected() {
        use crate::json_model::TextureTransform;
        let t = TextureTransform {
            offset: Some([f32::INFINITY, 0.0]),
            rotation: None,
            scale: None,
            tex_coord: None,
        };
        let err = validate_texture_transform(2, "KHR_materials_specular.specularTexture", &t)
            .unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackTextureTransformOffsetFinite"));
        let t = TextureTransform {
            offset: None,
            scale: Some([0.0, f32::NAN]),
            ..t
        };
        let err = validate_texture_transform(2, "KHR_materials_specular.specularTexture", &t)
            .unwrap_err();
        assert!(format!("{err}").contains("ExtensionStackTextureTransformScaleFinite"));
    }

    #[test]
    fn texture_transform_finite_values_accepted() {
        use crate::json_model::TextureTransform;
        let t = TextureTransform {
            offset: Some([0.1, 0.2]),
            rotation: Some(1.0e30),
            scale: Some([2.0, -1.0]),
            tex_coord: Some(1),
        };
        validate_texture_transform(0, "emissiveTexture", &t).unwrap();
    }

    #[test]
    fn material_texture_transforms_walks_extension_slots() {
        use crate::json_model::{
            Material, MaterialExtensions, MaterialSpecular, TextureInfo, TextureInfoExtensions,
            TextureTransform,
        };
        let mat = Material {
            extensions: Some(MaterialExtensions {
                khr_materials_specular: Some(MaterialSpecular {
                    specular_factor: None,
                    specular_texture: Some(TextureInfo {
                        index: 0,
                        tex_coord: None,
                        extensions: Some(TextureInfoExtensions {
                            khr_texture_transform: Some(TextureTransform {
                                offset: Some([0.5, 0.5]),
                                rotation: None,
                                scale: None,
                                tex_coord: None,
                            }),
                        }),
                    }),
                    specular_color_factor: None,
                    specular_color_texture: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let found = material_texture_transforms(&mat);
        assert_eq!(found.len(), 1, "specularTexture transform discovered");
        assert_eq!(found[0].0, "KHR_materials_specular.specularTexture");
    }

    // ---- validate_index_references (§3.3 / §5.27.1 / §5.25.5 /
    //      §5.25.1 / §5.24.3) ----

    fn buf(len: u32) -> Buffer {
        Buffer {
            uri: None,
            byte_length: len,
            name: None,
            extensions: None,
        }
    }

    fn bview(len: u32) -> BufferView {
        BufferView {
            buffer: 0,
            byte_offset: Some(0),
            byte_length: len,
            byte_stride: None,
            target: None,
            name: None,
            extensions: None,
        }
    }

    #[test]
    fn index_refs_pass_when_all_resolve() {
        use crate::json_model::Scene;
        let mut root = empty_root();
        root.scenes = vec![Scene {
            nodes: vec![0],
            ..Default::default()
        }];
        root.scene = Some(0);
        root.nodes = vec![Node {
            mesh: Some(0),
            camera: Some(0),
            ..Default::default()
        }];
        root.meshes = vec![Mesh {
            primitives: vec![Primitive {
                material: Some(0),
                ..Default::default()
            }],
            ..Default::default()
        }];
        root.cameras = vec![Camera::default()];
        root.materials = vec![Material::default()];
        validate_index_references(&root).unwrap();
    }

    #[test]
    fn index_refs_reject_default_scene_out_of_range() {
        let mut root = empty_root();
        root.scene = Some(2); // no scenes
        let err = validate_index_references(&root).unwrap_err();
        assert!(format!("{err}").contains("DefaultSceneIndex"));
    }

    #[test]
    fn index_refs_reject_scene_node_out_of_range() {
        use crate::json_model::Scene;
        let mut root = empty_root();
        root.scenes = vec![Scene {
            nodes: vec![5],
            ..Default::default()
        }];
        // one node only -> index 5 invalid
        root.nodes = vec![Node::default()];
        let err = validate_index_references(&root).unwrap_err();
        assert!(format!("{err}").contains("SceneNodeIndex"));
    }

    #[test]
    fn index_refs_reject_node_mesh_out_of_range() {
        let mut root = empty_root();
        root.nodes = vec![Node {
            mesh: Some(0),
            ..Default::default()
        }];
        // no meshes
        let err = validate_index_references(&root).unwrap_err();
        assert!(format!("{err}").contains("NodeMeshIndex"));
    }

    #[test]
    fn index_refs_reject_node_camera_out_of_range() {
        let mut root = empty_root();
        root.nodes = vec![Node {
            camera: Some(3),
            ..Default::default()
        }];
        let err = validate_index_references(&root).unwrap_err();
        assert!(format!("{err}").contains("NodeCameraIndex"));
    }

    #[test]
    fn index_refs_reject_primitive_material_out_of_range() {
        let mut root = empty_root();
        root.meshes = vec![Mesh {
            primitives: vec![Primitive {
                material: Some(1),
                ..Default::default()
            }],
            ..Default::default()
        }];
        // no materials
        let err = validate_index_references(&root).unwrap_err();
        assert!(format!("{err}").contains("PrimitiveMaterialIndex"));
    }

    // ---- validate_structural_minimums (§5.10.2 / §5.11.3 / §5.2.1 /
    //      §3.6.2.3) ----

    fn scalar_accessor(count: u32, sparse: Option<AccessorSparse>) -> Accessor {
        Accessor {
            buffer_view: Some(0),
            byte_offset: Some(0),
            component_type: COMPONENT_TYPE_FLOAT,
            count,
            kind: "SCALAR".to_owned(),
            normalized: false,
            min: None,
            max: None,
            name: None,
            sparse,
        }
    }

    fn sparse_with_count(count: u32) -> AccessorSparse {
        AccessorSparse {
            count,
            indices: AccessorSparseIndices {
                buffer_view: 0,
                byte_offset: None,
                component_type: 5123,
            },
            values: AccessorSparseValues {
                buffer_view: 1,
                byte_offset: None,
            },
        }
    }

    #[test]
    fn structural_min_passes_when_clean() {
        let mut root = empty_root();
        root.buffers = vec![buf(64)];
        root.buffer_views = vec![bview(64)];
        root.accessors = vec![scalar_accessor(8, Some(sparse_with_count(3)))];
        validate_structural_minimums(&root).unwrap();
    }

    #[test]
    fn structural_min_rejects_zero_buffer_byte_length() {
        let mut root = empty_root();
        root.buffers = vec![buf(0)];
        let err = validate_structural_minimums(&root).unwrap_err();
        assert!(format!("{err}").contains("BufferByteLength"));
    }

    #[test]
    fn structural_min_rejects_zero_buffer_view_byte_length() {
        let mut root = empty_root();
        root.buffers = vec![buf(64)];
        root.buffer_views = vec![bview(0)];
        let err = validate_structural_minimums(&root).unwrap_err();
        assert!(format!("{err}").contains("BufferViewByteLength"));
    }

    #[test]
    fn structural_min_rejects_zero_sparse_count() {
        let mut root = empty_root();
        root.accessors = vec![scalar_accessor(8, Some(sparse_with_count(0)))];
        let err = validate_structural_minimums(&root).unwrap_err();
        assert!(format!("{err}").contains("SparseCountMin"));
    }

    #[test]
    fn structural_min_rejects_sparse_count_above_base() {
        let mut root = empty_root();
        // base accessor has 4 elements; sparse claims 5 overrides.
        root.accessors = vec![scalar_accessor(4, Some(sparse_with_count(5)))];
        let err = validate_structural_minimums(&root).unwrap_err();
        assert!(format!("{err}").contains("SparseCountRange"));
    }
}
