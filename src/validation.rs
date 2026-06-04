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
//!   `mappings`), and `KHR_xmp_json_ld` (root-level `packets[]` roster +
//!   per-asset / per-scene / per-node / per-mesh / per-material
//!   `{ packet: N }` indirection).
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
//!
//! All failures surface as `Error::InvalidData` with a stable
//! `VertexAttribute…` / `ExtensionStack…` / `AnimationChannel…` /
//! `JsonDepthExceeded` / `JsonTooLarge` / `AssetVersion…` /
//! `AccessorFit…` / `BufferViewFit…` / `BufferViewStride…` /
//! `SparseIndicesBufferView…` prefix so callers can grep for the
//! specific sub-rule without reaching for a typed enum (the shared
//! `oxideav_core::Error` enum can't gain a new variant from a sibling
//! crate).

use crate::error::{invalid, Result};
use crate::json_model::{
    component_size, type_components, Accessor, Animation, Buffer, BufferView, GltfRoot, Mesh,
    COMPONENT_TYPE_UNSIGNED_BYTE, COMPONENT_TYPE_UNSIGNED_INT, COMPONENT_TYPE_UNSIGNED_SHORT,
};
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

    // KHR_texture_transform — per-textureInfo extension. Same §3.12 rule:
    // the extension MUST be declared in `extensionsUsed` if any
    // textureInfo carries the data block. The block may appear on any of
    // the five core PBR textureInfo slots (`baseColorTexture`,
    // `metallicRoughnessTexture`, `normalTexture`, `occlusionTexture`,
    // `emissiveTexture`) per `docs/3d/gltf/extensions/
    // KHR_texture_transform.md` §glTF Schema Updates.
    let has_texture_transform = root.materials.iter().any(|m| {
        let pbr_hit = m
            .pbr_metallic_roughness
            .as_ref()
            .map(|p| {
                texture_info_has_transform(p.base_color_texture.as_ref())
                    || texture_info_has_transform(p.metallic_roughness_texture.as_ref())
            })
            .unwrap_or(false);
        let normal_hit = m
            .normal_texture
            .as_ref()
            .and_then(|t| t.extensions.as_ref())
            .and_then(|e| e.khr_texture_transform.as_ref())
            .is_some();
        let occlusion_hit = m
            .occlusion_texture
            .as_ref()
            .and_then(|t| t.extensions.as_ref())
            .and_then(|e| e.khr_texture_transform.as_ref())
            .is_some();
        let emissive_hit = texture_info_has_transform(m.emissive_texture.as_ref());
        pbr_hit || normal_hit || occlusion_hit || emissive_hit
    });
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

fn texture_info_has_transform(t: Option<&crate::json_model::TextureInfo>) -> bool {
    t.and_then(|t| t.extensions.as_ref())
        .and_then(|e| e.khr_texture_transform.as_ref())
        .is_some()
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
}
