//! glTF 2.0 Asset Object Model — pointer-template registry.
//!
//! `KHR_animation_pointer` (see
//! `docs/3d/gltf/extensions/KHR_animation_pointer.md` §Overview) keys
//! its output-accessor conversion on the **Object Model Data Type** of
//! the property a channel's JSON Pointer (RFC 6901) targets. The data
//! types are declared per property as *pointer templates* — pointer
//! strings whose array-index positions are spelled `{}` — in the
//! Object Model tables of the core Object Model specification
//! (`docs/3d/gltf/ObjectModel.md`) and of each extension's Object
//! Model section.
//!
//! This registry transcribes every staged pointer-template table:
//!
//! * the core mutable-pointer table + the core read-only runtime
//!   table (`docs/3d/gltf/ObjectModel.md` §"Core Pointers");
//! * the per-extension mutable tables from the same document
//!   (`KHR_texture_transform`, `KHR_lights_punctual`, the
//!   `KHR_materials_*` family, `EXT_lights_ies`,
//!   `EXT_lights_image_based`, `ADOBE_materials_clearcoat_specular`,
//!   `ADOBE_materials_clearcoat_tint`) plus their read-only rows;
//! * `docs/3d/gltf/extensions/KHR_node_visibility.md` §"Extending
//!   glTF 2.0 Asset Object Model" (`visible` → `bool`);
//! * `docs/3d/gltf/extensions/KHR_audio_emitter.md` §"glTF Object
//!   Model" (emitter/source gains → `float`, `autoplay` / `loop` →
//!   `bool`, `.length` read-only rows).
//!
//! Pointers that match no entry fall back to the `float*` conversion
//! branch of §"Output Accessor Component Types" (FLOAT pass-through /
//! §3.6.2.2 normalized-int dequantisation / non-normalized-int cast) —
//! extensions may publish additional Object Model rows this crate has
//! not staged, so an unmatched pointer is not an error.

/// Object Model Data Type of a registered property, per
/// `docs/3d/gltf/ObjectModel.md` §"Data Types" and the
/// `KHR_animation_pointer` §Operation data-type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectModelDataType {
    /// `bool` — output accessor MUST be SCALAR (data-type table) with
    /// component type *unsigned byte*; `0` converts to `false`, any
    /// other value to `true`; the sampler MUST use `STEP`
    /// interpolation (§"Output Accessor Component Types").
    Bool,
    /// `float` — SCALAR output accessor, `float*` conversion branch.
    Float,
    /// `float[]` — SCALAR output accessor whose per-keyframe element
    /// count equals the runtime array length (for the one staged row,
    /// `/nodes/{}/weights`, that is the instantiated mesh's
    /// morph-target count).
    FloatArray,
    /// `float2` — VEC2 output accessor.
    Float2,
    /// `float3` — VEC3 output accessor.
    Float3,
    /// `float4` — VEC4 output accessor.
    Float4,
    /// `float4x4` — MAT4 output accessor.
    Float4x4,
    /// `int` — SCALAR output accessor; component type MUST be a
    /// non-normalized integer type; STEP interpolation MUST be used.
    /// Every staged `int` row today is read-only, so no animatable
    /// `int` property exists yet.
    Int,
}

impl ObjectModelDataType {
    /// The output accessor `type` the `KHR_animation_pointer`
    /// §Operation data-type table pins for this Object Model Data
    /// Type.
    pub fn expected_accessor_kind(self) -> &'static str {
        match self {
            ObjectModelDataType::Bool
            | ObjectModelDataType::Float
            | ObjectModelDataType::FloatArray
            | ObjectModelDataType::Int => "SCALAR",
            ObjectModelDataType::Float2 => "VEC2",
            ObjectModelDataType::Float3 => "VEC3",
            ObjectModelDataType::Float4 => "VEC4",
            ObjectModelDataType::Float4x4 => "MAT4",
        }
    }
}

/// One resolved registry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEntry {
    pub data_type: ObjectModelDataType,
    /// `true` for rows from a mutable-pointer table; `false` for the
    /// read-only runtime rows (array sizes, current transforms, object
    /// references). `KHR_animation_pointer` §Operation: "The property
    /// being animated MUST be mutable as defined by the glTF 2.0
    /// Asset Object Model."
    pub mutable: bool,
}

use ObjectModelDataType as T;

/// Mutable pointer templates. Sources:
///
/// * `docs/3d/gltf/ObjectModel.md` §"Core Pointers" (first table) and
///   the per-extension "mutable properties" tables of the same
///   document, including each "Interaction with KHR_texture_transform"
///   sub-table;
/// * `docs/3d/gltf/extensions/KHR_node_visibility.md` §"Extending
///   glTF 2.0 Asset Object Model";
/// * `docs/3d/gltf/extensions/KHR_audio_emitter.md` §"glTF Object
///   Model" (mutable table).
const MUTABLE_TEMPLATES: &[(&str, ObjectModelDataType)] = &[
    // --- core: cameras ---
    ("/cameras/{}/orthographic/xmag", T::Float),
    ("/cameras/{}/orthographic/ymag", T::Float),
    ("/cameras/{}/orthographic/zfar", T::Float),
    ("/cameras/{}/orthographic/znear", T::Float),
    ("/cameras/{}/perspective/aspectRatio", T::Float),
    ("/cameras/{}/perspective/yfov", T::Float),
    ("/cameras/{}/perspective/zfar", T::Float),
    ("/cameras/{}/perspective/znear", T::Float),
    // --- core: materials ---
    ("/materials/{}/alphaCutoff", T::Float),
    ("/materials/{}/emissiveFactor", T::Float3),
    ("/materials/{}/normalTexture/scale", T::Float),
    ("/materials/{}/occlusionTexture/strength", T::Float),
    (
        "/materials/{}/pbrMetallicRoughness/baseColorFactor",
        T::Float4,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/metallicFactor",
        T::Float,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/roughnessFactor",
        T::Float,
    ),
    // --- core: nodes ---
    ("/nodes/{}/translation", T::Float3),
    ("/nodes/{}/rotation", T::Float4),
    ("/nodes/{}/scale", T::Float3),
    ("/nodes/{}/weights", T::FloatArray),
    ("/nodes/{}/weights/{}", T::Float),
    // --- KHR_texture_transform (core PBR textureInfo slots) ---
    (
        "/materials/{}/normalTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/normalTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/normalTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/occlusionTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/occlusionTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/occlusionTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/emissiveTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/emissiveTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/emissiveTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/metallicRoughnessTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/metallicRoughnessTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/pbrMetallicRoughness/metallicRoughnessTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_lights_punctual ---
    ("/extensions/KHR_lights_punctual/lights/{}/color", T::Float3),
    (
        "/extensions/KHR_lights_punctual/lights/{}/intensity",
        T::Float,
    ),
    ("/extensions/KHR_lights_punctual/lights/{}/range", T::Float),
    (
        "/extensions/KHR_lights_punctual/lights/{}/spot/innerConeAngle",
        T::Float,
    ),
    (
        "/extensions/KHR_lights_punctual/lights/{}/spot/outerConeAngle",
        T::Float,
    ),
    // --- KHR_materials_anisotropy ---
    (
        "/materials/{}/extensions/KHR_materials_anisotropy/anisotropyStrength",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_anisotropy/anisotropyRotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_anisotropy/anisotropyTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_anisotropy/anisotropyTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_anisotropy/anisotropyTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_materials_clearcoat ---
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatRoughnessFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatNormalTexture/scale",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatRoughnessTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatRoughnessTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatRoughnessTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatNormalTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatNormalTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_clearcoat/clearcoatNormalTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_materials_dispersion ---
    (
        "/materials/{}/extensions/KHR_materials_dispersion/dispersion",
        T::Float,
    ),
    // --- KHR_materials_emissive_strength ---
    (
        "/materials/{}/extensions/KHR_materials_emissive_strength/emissiveStrength",
        T::Float,
    ),
    // --- KHR_materials_ior ---
    ("/materials/{}/extensions/KHR_materials_ior/ior", T::Float),
    // --- KHR_materials_iridescence ---
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceIor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceThicknessMinimum",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceThicknessMaximum",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceThicknessTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceThicknessTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_iridescence/iridescenceThicknessTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_materials_sheen ---
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenColorFactor",
        T::Float3,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenRoughnessFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenColorTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenColorTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenColorTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenRoughnessTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenRoughnessTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_sheen/sheenRoughnessTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_materials_specular ---
    (
        "/materials/{}/extensions/KHR_materials_specular/specularFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularColorFactor",
        T::Float3,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularColorTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularColorTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_specular/specularColorTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_materials_transmission ---
    (
        "/materials/{}/extensions/KHR_materials_transmission/transmissionFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_transmission/transmissionTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_transmission/transmissionTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_transmission/transmissionTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_materials_volume ---
    (
        "/materials/{}/extensions/KHR_materials_volume/thicknessFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_volume/attenuationDistance",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_volume/attenuationColor",
        T::Float3,
    ),
    (
        "/materials/{}/extensions/KHR_materials_volume/thicknessTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/KHR_materials_volume/thicknessTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/KHR_materials_volume/thicknessTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- EXT_lights_ies ---
    ("/nodes/{}/extensions/EXT_lights_ies/multiplier", T::Float),
    ("/nodes/{}/extensions/EXT_lights_ies/color", T::Float3),
    // --- EXT_lights_image_based ---
    (
        "/extensions/EXT_lights_image_based/lights/{}/rotation",
        T::Float4,
    ),
    (
        "/extensions/EXT_lights_image_based/lights/{}/intensity",
        T::Float,
    ),
    // --- ADOBE_materials_clearcoat_specular ---
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_specular/clearcoatIor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_specular/clearcoatSpecularFactor",
        T::Float,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_specular/clearcoatSpecularTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_specular/clearcoatSpecularTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_specular/clearcoatSpecularTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- ADOBE_materials_clearcoat_tint ---
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_tint/clearcoatTintFactor",
        T::Float3,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_tint/clearcoatTintTexture/extensions/KHR_texture_transform/offset",
        T::Float2,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_tint/clearcoatTintTexture/extensions/KHR_texture_transform/rotation",
        T::Float,
    ),
    (
        "/materials/{}/extensions/ADOBE_materials_clearcoat_tint/clearcoatTintTexture/extensions/KHR_texture_transform/scale",
        T::Float2,
    ),
    // --- KHR_node_visibility ---
    (
        "/nodes/{}/extensions/KHR_node_visibility/visible",
        T::Bool,
    ),
    // --- KHR_audio_emitter (mutable table) ---
    ("/extensions/KHR_audio_emitter/emitters/{}/gain", T::Float),
    (
        "/extensions/KHR_audio_emitter/emitters/{}/positional/coneInnerAngle",
        T::Float,
    ),
    (
        "/extensions/KHR_audio_emitter/emitters/{}/positional/coneOuterAngle",
        T::Float,
    ),
    (
        "/extensions/KHR_audio_emitter/emitters/{}/positional/coneOuterGain",
        T::Float,
    ),
    (
        "/extensions/KHR_audio_emitter/emitters/{}/positional/maxDistance",
        T::Float,
    ),
    (
        "/extensions/KHR_audio_emitter/emitters/{}/positional/refDistance",
        T::Float,
    ),
    (
        "/extensions/KHR_audio_emitter/emitters/{}/positional/rolloffFactor",
        T::Float,
    ),
    ("/extensions/KHR_audio_emitter/sources/{}/autoplay", T::Bool),
    ("/extensions/KHR_audio_emitter/sources/{}/gain", T::Float),
    ("/extensions/KHR_audio_emitter/sources/{}/loop", T::Bool),
    (
        "/extensions/KHR_audio_emitter/sources/{}/playbackRate",
        T::Float,
    ),
];

/// Read-only pointer templates — array sizes, current transforms, and
/// object references. Sources: `docs/3d/gltf/ObjectModel.md`
/// §"Core Pointers" (second table) + the per-extension
/// "Additional read-only properties" tables, and
/// `docs/3d/gltf/extensions/KHR_audio_emitter.md` §"glTF Object
/// Model" (read-only table). "Read-only pointers that represent glTF
/// object references can be made mutable by extensions on a
/// case-by-case basis" — no staged extension does so today.
const READONLY_TEMPLATES: &[(&str, ObjectModelDataType)] = &[
    ("/animations.length", T::Int),
    ("/cameras.length", T::Int),
    ("/materials.length", T::Int),
    ("/materials/{}/doubleSided", T::Bool),
    ("/meshes.length", T::Int),
    ("/meshes/{}/primitives.length", T::Int),
    ("/meshes/{}/primitives/{}/material", T::Int),
    ("/nodes.length", T::Int),
    ("/nodes/{}/camera", T::Int),
    ("/nodes/{}/children.length", T::Int),
    ("/nodes/{}/children/{}", T::Int),
    ("/nodes/{}/globalMatrix", T::Float4x4),
    ("/nodes/{}/matrix", T::Float4x4),
    ("/nodes/{}/mesh", T::Int),
    ("/nodes/{}/parent", T::Int),
    ("/nodes/{}/skin", T::Int),
    ("/nodes/{}/weights.length", T::Int),
    ("/scene", T::Int),
    ("/scenes.length", T::Int),
    ("/scenes/{}/nodes.length", T::Int),
    ("/scenes/{}/nodes/{}", T::Int),
    ("/skins.length", T::Int),
    ("/skins/{}/joints.length", T::Int),
    ("/skins/{}/joints/{}", T::Int),
    ("/skins/{}/skeleton", T::Int),
    // --- KHR_lights_punctual ---
    ("/extensions/KHR_lights_punctual/lights.length", T::Int),
    ("/nodes/{}/extensions/KHR_lights_punctual/light", T::Int),
    // --- EXT_lights_ies ---
    ("/extensions/EXT_lights_ies/lights.length", T::Int),
    // --- EXT_lights_image_based ---
    ("/extensions/EXT_lights_image_based/lights.length", T::Int),
    // --- KHR_audio_emitter ---
    ("/extensions/KHR_audio_emitter/emitters.length", T::Int),
    ("/extensions/KHR_audio_emitter/sources.length", T::Int),
    ("/extensions/KHR_audio_emitter/audio.length", T::Int),
];

/// Resolve `pointer` against the full registry (mutable + read-only
/// rows). Returns `None` when no template matches — the caller then
/// treats the pointer as an unstaged extension property and uses the
/// `float*` conversion branch of `KHR_animation_pointer` §"Output
/// Accessor Component Types".
pub fn pointer_entry(pointer: &str) -> Option<PointerEntry> {
    // Mutable rows first: `/nodes/{}/weights/{}` (mutable float) must
    // win over no read-only row, and no template appears in both
    // tables, so order only affects lookup cost.
    if let Some(&(_, ty)) = MUTABLE_TEMPLATES
        .iter()
        .find(|(template, _)| template_matches(template, pointer))
    {
        return Some(PointerEntry {
            data_type: ty,
            mutable: true,
        });
    }
    if let Some(&(_, ty)) = READONLY_TEMPLATES
        .iter()
        .find(|(template, _)| template_matches(template, pointer))
    {
        return Some(PointerEntry {
            data_type: ty,
            mutable: false,
        });
    }
    None
}

/// Resolve `pointer` to the Object Model Data Type of a **mutable**
/// registered property. Returns `None` for unmatched pointers AND for
/// read-only rows — the decode paths use this to pick the output
/// conversion lane, and a read-only property is never legitimately
/// animated (the validator rejects it before decode dispatch
/// matters).
pub fn pointer_data_type(pointer: &str) -> Option<ObjectModelDataType> {
    pointer_entry(pointer).and_then(|e| e.mutable.then_some(e.data_type))
}

/// Match a pointer-template against a concrete RFC 6901 pointer.
/// Both are `/`-separated reference-token sequences; a literal `{}`
/// template token matches exactly one array-index token (RFC 6901 §4:
/// digits without a leading zero, or the single digit `0`), every
/// other template token must match the pointer token verbatim.
fn template_matches(template: &str, pointer: &str) -> bool {
    if !pointer.starts_with('/') || !template.starts_with('/') {
        return false;
    }
    let mut t = template[1..].split('/');
    let mut p = pointer[1..].split('/');
    loop {
        match (t.next(), p.next()) {
            (None, None) => return true,
            (Some("{}"), Some(idx)) => {
                if !is_array_index(idx) {
                    return false;
                }
            }
            (Some(tt), Some(pt)) => {
                if tt != pt {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// RFC 6901 §4 array-index syntax: `0`, or a non-empty digit run
/// without a leading zero.
fn is_array_index(token: &str) -> bool {
    !token.is_empty()
        && token.bytes().all(|b| b.is_ascii_digit())
        && (token == "0" || !token.starts_with('0'))
}

/// Extract the array-index token at `token_position` (0-based,
/// counting reference tokens after the leading `/`) from a concrete
/// pointer. Used by the validators to resolve e.g. the node index of
/// a matched `/nodes/{}/weights` pointer.
pub fn pointer_index_at(pointer: &str, token_position: usize) -> Option<usize> {
    let token = pointer.strip_prefix('/')?.split('/').nth(token_position)?;
    if !is_array_index(token) {
        return None;
    }
    token.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_visibility_visible_resolves_to_bool() {
        // Template row from `docs/3d/gltf/extensions/
        // KHR_node_visibility.md` §"Extending glTF 2.0 Asset Object
        // Model" — `{}` stands for any node array index.
        for idx in ["0", "3", "42", "1007"] {
            let ptr = format!("/nodes/{idx}/extensions/KHR_node_visibility/visible");
            assert_eq!(
                pointer_data_type(&ptr),
                Some(ObjectModelDataType::Bool),
                "index {idx} must match the {{}} template token"
            );
        }
    }

    #[test]
    fn non_index_tokens_do_not_match_the_template() {
        // `{}` matches array indices only (RFC 6901 §4) — a leading
        // zero, a name, or an empty token is not an index.
        for bad in ["01", "x", "-1", ""] {
            let ptr = format!("/nodes/{bad}/extensions/KHR_node_visibility/visible");
            assert_eq!(
                pointer_data_type(&ptr),
                None,
                "token {bad:?} must not match"
            );
        }
    }

    #[test]
    fn core_mutable_rows_resolve() {
        // Spot-check one row per core Object Model type
        // (`docs/3d/gltf/ObjectModel.md` §"Core Pointers").
        for (ptr, ty) in [
            ("/cameras/2/perspective/yfov", ObjectModelDataType::Float),
            ("/nodes/0/translation", ObjectModelDataType::Float3),
            ("/nodes/0/rotation", ObjectModelDataType::Float4),
            ("/nodes/7/weights", ObjectModelDataType::FloatArray),
            ("/nodes/7/weights/2", ObjectModelDataType::Float),
            (
                "/materials/1/pbrMetallicRoughness/baseColorFactor",
                ObjectModelDataType::Float4,
            ),
            (
                "/materials/0/emissiveTexture/extensions/KHR_texture_transform/offset",
                ObjectModelDataType::Float2,
            ),
            (
                "/extensions/KHR_lights_punctual/lights/3/color",
                ObjectModelDataType::Float3,
            ),
            (
                "/extensions/KHR_audio_emitter/sources/0/loop",
                ObjectModelDataType::Bool,
            ),
        ] {
            assert_eq!(pointer_data_type(ptr), Some(ty), "pointer {ptr}");
            assert_eq!(
                pointer_entry(ptr),
                Some(PointerEntry {
                    data_type: ty,
                    mutable: true
                })
            );
        }
    }

    #[test]
    fn read_only_rows_resolve_as_immutable() {
        for (ptr, ty) in [
            ("/nodes.length", ObjectModelDataType::Int),
            ("/nodes/4/matrix", ObjectModelDataType::Float4x4),
            ("/nodes/4/globalMatrix", ObjectModelDataType::Float4x4),
            ("/nodes/4/weights.length", ObjectModelDataType::Int),
            ("/materials/0/doubleSided", ObjectModelDataType::Bool),
            ("/scene", ObjectModelDataType::Int),
            ("/skins/1/joints/0", ObjectModelDataType::Int),
            (
                "/extensions/KHR_lights_punctual/lights.length",
                ObjectModelDataType::Int,
            ),
        ] {
            assert_eq!(
                pointer_entry(ptr),
                Some(PointerEntry {
                    data_type: ty,
                    mutable: false
                }),
                "pointer {ptr}"
            );
            // Read-only rows never dispatch a decode lane.
            assert_eq!(pointer_data_type(ptr), None, "pointer {ptr}");
        }
    }

    #[test]
    fn unrelated_pointers_fall_back_to_float_branch() {
        for ptr in [
            "/materials/0/extensions/VENDOR_custom/factor",
            "/nodes/0/extensions/KHR_node_visibility",
            "/nodes/0/extensions/KHR_node_visibility/visible/0",
            "/nodes/x/translation",
            "",
        ] {
            assert_eq!(pointer_data_type(ptr), None, "pointer {ptr:?} has no row");
            assert_eq!(pointer_entry(ptr), None, "pointer {ptr:?} has no row");
        }
    }

    #[test]
    fn pointer_index_extraction() {
        assert_eq!(pointer_index_at("/nodes/7/weights", 1), Some(7));
        assert_eq!(pointer_index_at("/nodes/7/weights/2", 3), Some(2));
        assert_eq!(pointer_index_at("/nodes/x/weights", 1), None);
        assert_eq!(pointer_index_at("/nodes/07/weights", 1), None);
        assert_eq!(pointer_index_at("/nodes", 1), None);
        assert_eq!(pointer_index_at("nodes/7", 0), None);
    }

    #[test]
    fn expected_accessor_kinds_follow_the_operation_table() {
        use ObjectModelDataType as T;
        assert_eq!(T::Bool.expected_accessor_kind(), "SCALAR");
        assert_eq!(T::Float.expected_accessor_kind(), "SCALAR");
        assert_eq!(T::FloatArray.expected_accessor_kind(), "SCALAR");
        assert_eq!(T::Int.expected_accessor_kind(), "SCALAR");
        assert_eq!(T::Float2.expected_accessor_kind(), "VEC2");
        assert_eq!(T::Float3.expected_accessor_kind(), "VEC3");
        assert_eq!(T::Float4.expected_accessor_kind(), "VEC4");
        assert_eq!(T::Float4x4.expected_accessor_kind(), "MAT4");
    }
}
