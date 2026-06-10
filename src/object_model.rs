//! glTF 2.0 Asset Object Model — pointer-template registry.
//!
//! `KHR_animation_pointer` (see
//! `docs/3d/gltf/extensions/KHR_animation_pointer.md` §Overview) keys
//! its output-accessor conversion on the **Object Model Data Type** of
//! the property a channel's JSON Pointer (RFC 6901) targets. The data
//! types are declared per property as *pointer templates* — pointer
//! strings whose array-index positions are spelled `{}` — in the
//! Object Model tables of the core spec and of each extension's
//! §"Extending glTF 2.0 Asset Object Model" section.
//!
//! This registry holds every pointer template staged under
//! `docs/3d/gltf/extensions/` whose data type is NOT one of the
//! `float*` family. Pointers that match no entry fall back to the
//! `float*` conversion branch of §"Output Accessor Component Types"
//! (FLOAT pass-through / §3.6.2.2 normalized-int dequantisation /
//! non-normalized-int cast) — the only branch the spec defines without
//! consulting the registry. The core spec's own Object Model table
//! (`ObjectModel.adoc`) is not staged in `docs/3d/gltf/`, so core
//! properties are not represented here; every ratified core mutable
//! property an animation can plausibly target is `float*`-typed per
//! the staged extension specs' usage, and the registry can grow rows
//! as further Object Model tables land in `docs/`.

/// Object Model Data Type of a mutable property, per the
/// `KHR_animation_pointer` §Operation data-type table. Only the
/// non-`float*` types need registry rows — `float*` is the fallback
/// branch — and the staged extension specs declare exactly one such
/// property today (`bool`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectModelDataType {
    /// `bool` — output accessor MUST be SCALAR (data-type table) with
    /// component type *unsigned byte*; `0` converts to `false`, any
    /// other value to `true`; the sampler MUST use `STEP`
    /// interpolation (§"Output Accessor Component Types").
    Bool,
}

/// Pointer templates with a non-`float*` Object Model Data Type,
/// transcribed from the staged extension specs:
///
/// * `/nodes/{}/extensions/KHR_node_visibility/visible` → `bool` —
///   `docs/3d/gltf/extensions/KHR_node_visibility.md` §"Extending
///   glTF 2.0 Asset Object Model" pointer-template table.
const POINTER_TEMPLATES: &[(&str, ObjectModelDataType)] = &[(
    "/nodes/{}/extensions/KHR_node_visibility/visible",
    ObjectModelDataType::Bool,
)];

/// Resolve `pointer` against the registry. Returns `None` when no
/// template matches — the caller then uses the `float*` conversion
/// branch of `KHR_animation_pointer` §"Output Accessor Component
/// Types".
pub fn pointer_data_type(pointer: &str) -> Option<ObjectModelDataType> {
    POINTER_TEMPLATES
        .iter()
        .find(|(template, _)| template_matches(template, pointer))
        .map(|&(_, ty)| ty)
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
    fn unrelated_pointers_fall_back_to_float_branch() {
        for ptr in [
            "/materials/0/pbrMetallicRoughness/baseColorFactor",
            "/nodes/0/rotation",
            "/nodes/0/extensions/KHR_node_visibility",
            "/nodes/0/extensions/KHR_node_visibility/visible/0",
            "",
        ] {
            assert_eq!(pointer_data_type(ptr), None, "pointer {ptr:?} has no row");
        }
    }
}
