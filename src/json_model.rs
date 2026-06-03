//! Serde-derived structs that mirror the Khronos glTF 2.0 JSON
//! schema 1:1.
//!
//! Every field that the spec marks as optional is `Option<...>` plus
//! `#[serde(skip_serializing_if = "Option::is_none")]` so the encoder
//! emits exactly the keys the input had — no spurious `null` /
//! defaulted output. Ditto for collections (`Vec`s default to empty +
//! `skip_serializing_if = "Vec::is_empty"`).
//!
//! Reference: glTF 2.0 spec §3 (`docs/3d/gltf/gltf-2.0-spec.html`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Top-level glTF 2.0 document.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GltfRoot {
    pub asset: Asset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<Scene>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<Mesh>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accessors: Vec<Accessor>,
    #[serde(rename = "bufferViews", default, skip_serializing_if = "Vec::is_empty")]
    pub buffer_views: Vec<BufferView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buffers: Vec<Buffer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<Material>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<Texture>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samplers: Vec<Sampler>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cameras: Vec<Camera>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<Animation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skins: Vec<Skin>,
    #[serde(
        rename = "extensionsUsed",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub extensions_used: Vec<String>,
    #[serde(
        rename = "extensionsRequired",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub extensions_required: Vec<String>,
    /// Top-level `extensions` carries object-level extension data —
    /// notably `KHR_lights_punctual` lives here at root scope (not
    /// per-node) per the extension spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<RootExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

/// `asset` block — the only required top-level object per spec §3.2.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(
        rename = "minVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_version: Option<String>,
    /// Per-object `extensions` block. Today this carries the
    /// `KHR_xmp_json_ld` packet-reference indirection per
    /// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating XMP
    /// metadata" — the asset object MAY point at one of the root-level
    /// `packets[]` entries to declare document-wide metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<AssetExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

/// Per-asset `extensions` block. Today only carries
/// `KHR_xmp_json_ld` per
/// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating XMP
/// metadata"; the `{ "packet": N }` indirection points at a root-level
/// `KHR_xmp_json_ld.packets[N]`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AssetExtensions {
    #[serde(
        rename = "KHR_xmp_json_ld",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_xmp_json_ld: Option<XmpPacketRef>,
}

/// Per-object indirection into the root `KHR_xmp_json_ld.packets[]`
/// array. The spec defines a single property:
///
/// * `packet` — index into the root-level packets array.
///
/// Per `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating
/// XMP metadata" the same shape is used on `asset`, `scene`, `node`,
/// `mesh`, `material`, `image`, and `animation`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct XmpPacketRef {
    pub packet: u32,
}

impl Default for Asset {
    fn default() -> Self {
        Self {
            version: "2.0".to_owned(),
            generator: Some(format!("oxideav-gltf {}", env!("CARGO_PKG_VERSION"))),
            copyright: None,
            min_version: None,
            extensions: None,
            extras: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Scene {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Per-scene `extensions` block. Today carries the
    /// `KHR_xmp_json_ld` packet-reference indirection per
    /// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating XMP
    /// metadata" — when set, the per-scene packet ref takes precedence
    /// over the asset-level packet ref per the spec's "Precedence"
    /// note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<SceneExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

/// Per-scene `extensions` block — today only carries
/// `KHR_xmp_json_ld` (a `{ "packet": N }` indirection into the
/// root-level packets array).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SceneExtensions {
    #[serde(
        rename = "KHR_xmp_json_ld",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_xmp_json_ld: Option<XmpPacketRef>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Node {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
    /// `matrix`, when present, is column-major per spec §3.5.2.1 and
    /// must NOT be combined with TRS. We surface it as-is so the
    /// scene translator can dispatch on which form was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<[f32; 16]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<[f32; 3]>,
    /// xyzw quaternion per glTF (Three.js / Unity convention).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<NodeExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Mesh {
    pub primitives: Vec<Primitive>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Per-spec §3.7.2.2 morph weights — default per-target weight
    /// vector used when `node.weights` is undefined. Length must
    /// match the number of `primitive.targets`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f32>>,
    /// Per-mesh `extensions` block. Today this carries the
    /// `KHR_xmp_json_ld` packet-reference indirection per
    /// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating XMP
    /// metadata" — the spec's primary illustration of the indirection
    /// uses a Mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<MeshExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

/// Per-mesh `extensions` block — today only carries
/// `KHR_xmp_json_ld` (a `{ "packet": N }` indirection into the
/// root-level packets array).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MeshExtensions {
    #[serde(
        rename = "KHR_xmp_json_ld",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_xmp_json_ld: Option<XmpPacketRef>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Primitive {
    /// Attribute name → accessor index. Standard names per spec
    /// §3.7.2.1: POSITION, NORMAL, TANGENT, TEXCOORD_n, COLOR_n,
    /// JOINTS_n, WEIGHTS_n.
    pub attributes: HashMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indices: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<u32>,
    /// Topology (4 = TRIANGLES default per spec §3.7.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    /// Per-spec §3.7.2.2 morph targets — each entry is
    /// `attribute name → accessor index` (POSITION_0, NORMAL_0,
    /// TANGENT_0 are the standard names). All primitives in a mesh
    /// MUST have the same number of targets in the same order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<HashMap<String, u32>>,
    /// Per-spec primitive-level `extensions` block. Currently surfaces
    /// `KHR_materials_variants` mappings per
    /// `docs/3d/gltf/extensions/KHR_materials_variants.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<PrimitiveExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

/// Per-primitive `extensions` block. Today carries the
/// `KHR_materials_variants` mapping table that pairs material indices
/// with the root-level variant indices they apply to.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PrimitiveExtensions {
    #[serde(
        rename = "KHR_materials_variants",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_variants: Option<PrimitiveVariantMappings>,
}

/// `KHR_materials_variants` per-primitive extension object — a
/// `mappings` list pairing material indices with variant indices.
/// Per `docs/3d/gltf/extensions/KHR_materials_variants.md` each
/// `variant` index across one primitive's mappings list MUST appear at
/// most once.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PrimitiveVariantMappings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<VariantMapping>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct VariantMapping {
    /// Index into the root-level `materials[]` array.
    pub material: u32,
    /// Indices into the root-level
    /// `extensions.KHR_materials_variants.variants[]` array.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Accessor {
    #[serde(
        rename = "bufferView",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub buffer_view: Option<u32>,
    #[serde(
        rename = "byteOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byte_offset: Option<u32>,
    #[serde(rename = "componentType")]
    pub component_type: u32,
    pub count: u32,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub normalized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Sparse-storage block per spec §3.6.2.3. When present alongside
    /// `bufferView`, the sparse entries override `count` elements at
    /// `indices` with the matching `values` slot. When `bufferView` is
    /// `None`, the base array is initialised to zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sparse: Option<AccessorSparse>,
}

/// `accessor.sparse` block — describes element-level overrides on top
/// of the (optional) base buffer-view content.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccessorSparse {
    pub count: u32,
    pub indices: AccessorSparseIndices,
    pub values: AccessorSparseValues,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccessorSparseIndices {
    #[serde(rename = "bufferView")]
    pub buffer_view: u32,
    #[serde(
        rename = "byteOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byte_offset: Option<u32>,
    /// 5121 / 5123 / 5125 (UNSIGNED_BYTE / SHORT / INT).
    #[serde(rename = "componentType")]
    pub component_type: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccessorSparseValues {
    #[serde(rename = "bufferView")]
    pub buffer_view: u32,
    #[serde(
        rename = "byteOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byte_offset: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BufferView {
    pub buffer: u32,
    #[serde(
        rename = "byteOffset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byte_offset: Option<u32>,
    #[serde(rename = "byteLength")]
    pub byte_length: u32,
    #[serde(
        rename = "byteStride",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub byte_stride: Option<u32>,
    /// 34962 = ARRAY_BUFFER, 34963 = ELEMENT_ARRAY_BUFFER (optional
    /// hint per spec §3.6.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Buffer {
    #[serde(rename = "byteLength")]
    pub byte_length: u32,
    /// `None` here on buffer 0 of a `.glb` means "use the BIN chunk"
    /// per spec §4.4.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Material {
    #[serde(
        rename = "pbrMetallicRoughness",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pbr_metallic_roughness: Option<PbrMetallicRoughness>,
    #[serde(
        rename = "normalTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub normal_texture: Option<NormalTextureInfo>,
    #[serde(
        rename = "occlusionTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub occlusion_texture: Option<OcclusionTextureInfo>,
    #[serde(
        rename = "emissiveFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub emissive_factor: Option<[f32; 3]>,
    #[serde(
        rename = "emissiveTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub emissive_texture: Option<TextureInfo>,
    #[serde(rename = "alphaMode", default, skip_serializing_if = "Option::is_none")]
    pub alpha_mode: Option<String>,
    #[serde(
        rename = "alphaCutoff",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub alpha_cutoff: Option<f32>,
    #[serde(
        rename = "doubleSided",
        default,
        skip_serializing_if = "is_default_false"
    )]
    pub double_sided: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Per-material `extensions` block. Today this carries
    /// `KHR_materials_unlit` (a boolean-flag shading-model selector
    /// per the KHR_materials_unlit spec — `docs/3d/gltf/extensions/
    /// KHR_materials_unlit.md`), `KHR_materials_emissive_strength`
    /// (a scalar emissive multiplier — `docs/3d/gltf/extensions/
    /// KHR_materials_emissive_strength.md`), `KHR_materials_ior`
    /// (a scalar index of refraction — `docs/3d/gltf/extensions/
    /// KHR_materials_ior.md`), and `KHR_materials_specular`
    /// (a specular reflection factor + F0 colour + optional textures
    /// — `docs/3d/gltf/extensions/KHR_materials_specular.md`),
    /// `KHR_materials_anisotropy` (an anisotropic specular lobe
    /// strength + rotation + optional direction/strength texture —
    /// `docs/3d/gltf/extensions/KHR_materials_anisotropy.md`), and
    /// `KHR_materials_dispersion` (a single `dispersion` scalar = 20/Vd
    /// driving the chromatic-aberration spread for the volumetric
    /// transmission model per `docs/3d/gltf/extensions/
    /// KHR_materials_dispersion.md`), and
    /// `KHR_materials_diffuse_transmission` (a diffuse-transmission
    /// factor, colour, and optional textures modelling light that
    /// diffuses through infinitely-thin surfaces like leaves or paper
    /// per `docs/3d/gltf/extensions/
    /// KHR_materials_diffuse_transmission.md`); future per-material KHR
    /// extensions land here too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<MaterialExtensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

fn is_default_false(b: &bool) -> bool {
    !*b
}

/// Per-material `extensions` block. Models the per-material KHR
/// extensions the crate understands: `KHR_materials_unlit` (an
/// empty-object shading-model flag), `KHR_materials_emissive_strength`
/// (a scalar emissive multiplier per
/// `docs/3d/gltf/extensions/KHR_materials_emissive_strength.md`),
/// `KHR_materials_ior` (a scalar index of refraction per
/// `docs/3d/gltf/extensions/KHR_materials_ior.md`),
/// `KHR_materials_specular` (a specular factor + F0 colour + optional
/// textures per `docs/3d/gltf/extensions/KHR_materials_specular.md`),
/// `KHR_materials_clearcoat` (a clear-coat layer's intensity +
/// roughness factors + optional textures per
/// `docs/3d/gltf/extensions/KHR_materials_clearcoat.md`), and
/// `KHR_materials_sheen` (a sheen colour + roughness factors + optional
/// textures per `docs/3d/gltf/extensions/KHR_materials_sheen.md`), and
/// `KHR_materials_transmission` (a transmission factor + optional
/// texture per
/// `docs/3d/gltf/extensions/KHR_materials_transmission.md`), and
/// `KHR_materials_volume` (a thickness + attenuation distance + colour
/// describing a homogeneous volumetric medium enclosed by the mesh per
/// `docs/3d/gltf/extensions/KHR_materials_volume.md`), and
/// `KHR_materials_iridescence` (a thin-film intensity + IOR + thickness
/// range modelling the iridescence effect per
/// `docs/3d/gltf/extensions/KHR_materials_iridescence.md`), and
/// `KHR_materials_anisotropy` (an anisotropic specular lobe with a
/// strength scalar + rotation angle + optional direction/strength
/// texture per `docs/3d/gltf/extensions/KHR_materials_anisotropy.md`),
/// and `KHR_materials_dispersion` (a single `dispersion` scalar set to
/// `20/Vd` driving the wavelength-dependent index-of-refraction
/// spread used by the volumetric transmission model per
/// `docs/3d/gltf/extensions/KHR_materials_dispersion.md`), and
/// `KHR_materials_diffuse_transmission` (a diffuse-transmission factor,
/// colour, and optional textures modelling light that diffuses through
/// infinitely-thin surfaces like leaves or paper per
/// `docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialExtensions {
    #[serde(
        rename = "KHR_materials_unlit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_unlit: Option<MaterialUnlit>,
    #[serde(
        rename = "KHR_materials_emissive_strength",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_emissive_strength: Option<MaterialEmissiveStrength>,
    #[serde(
        rename = "KHR_materials_ior",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_ior: Option<MaterialIor>,
    #[serde(
        rename = "KHR_materials_specular",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_specular: Option<MaterialSpecular>,
    #[serde(
        rename = "KHR_materials_clearcoat",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_clearcoat: Option<MaterialClearcoat>,
    #[serde(
        rename = "KHR_materials_sheen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_sheen: Option<MaterialSheen>,
    #[serde(
        rename = "KHR_materials_transmission",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_transmission: Option<MaterialTransmission>,
    #[serde(
        rename = "KHR_materials_volume",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_volume: Option<MaterialVolume>,
    #[serde(
        rename = "KHR_materials_iridescence",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_iridescence: Option<MaterialIridescence>,
    #[serde(
        rename = "KHR_materials_anisotropy",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_anisotropy: Option<MaterialAnisotropy>,
    #[serde(
        rename = "KHR_materials_dispersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_dispersion: Option<MaterialDispersion>,
    #[serde(
        rename = "KHR_materials_diffuse_transmission",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_diffuse_transmission: Option<MaterialDiffuseTransmission>,
    /// `KHR_xmp_json_ld` — `{ "packet": N }` indirection pointing the
    /// material at one of the root-level XMP packets per
    /// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating
    /// XMP metadata". Per the spec's "Precedence" note this overrides
    /// the asset-level packet ref for the material's scope.
    #[serde(
        rename = "KHR_xmp_json_ld",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_xmp_json_ld: Option<XmpPacketRef>,
}

/// `KHR_materials_unlit` extension object. Per the spec the schema
/// allows additional properties but no field is defined, so the
/// presence of the object itself is the signal. We keep the struct
/// empty for the encoder to emit a literal `{}`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialUnlit {}

/// `KHR_materials_emissive_strength` extension object — a single
/// `emissiveStrength` scalar that multiplies the core material's
/// emissive value, allowing emission above the [0,1] clamp for HDR
/// rendering. Per the spec §Parameters the field is optional with a
/// default of `1.0`. See
/// `docs/3d/gltf/extensions/KHR_materials_emissive_strength.md`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialEmissiveStrength {
    #[serde(
        rename = "emissiveStrength",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub emissive_strength: Option<f32>,
}

/// `KHR_materials_ior` extension object — a single `ior` scalar that
/// overrides the metallic-roughness dielectric BRDF's fixed index of
/// refraction (the core spec hard-codes 1.5). Per the spec the field is
/// optional with a default of `1.5`; valid values are `>= 1`, with `0`
/// reserved as the special specular-glossiness backwards-compatibility
/// sentinel. See `docs/3d/gltf/extensions/KHR_materials_ior.md`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialIor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ior: Option<f32>,
}

/// `KHR_materials_specular` extension object — adds two parameters to
/// the metallic-roughness material: a scalar `specularFactor` (default
/// `1.0`) that scales the dielectric BRDF's specular reflection, an
/// optional `specularTexture` whose alpha channel multiplies the
/// factor; an RGB `specularColorFactor` (default `[1.0, 1.0, 1.0]`)
/// that tints the F0 colour of the dielectric BRDF, and an optional
/// sRGB `specularColorTexture` whose RGB channels multiply the colour
/// factor. All four fields are optional per the spec. See
/// `docs/3d/gltf/extensions/KHR_materials_specular.md` §Extending
/// Materials.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialSpecular {
    #[serde(
        rename = "specularFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub specular_factor: Option<f32>,
    #[serde(
        rename = "specularTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub specular_texture: Option<TextureInfo>,
    #[serde(
        rename = "specularColorFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub specular_color_factor: Option<[f32; 3]>,
    #[serde(
        rename = "specularColorTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub specular_color_texture: Option<TextureInfo>,
}

/// `KHR_materials_clearcoat` extension object — layers a protective
/// clear coating on top of the metallic-roughness material. Adds two
/// scalar factors and three optional texture references per
/// `docs/3d/gltf/extensions/KHR_materials_clearcoat.md` §Extending
/// Materials §Clearcoat:
///
/// * `clearcoatFactor` (default `0.0`) — clearcoat layer intensity;
///   when zero the whole clearcoat layer is disabled.
/// * `clearcoatTexture` (a `textureInfo`) — the intensity texture; its
///   `.r` channel multiplies `clearcoatFactor`.
/// * `clearcoatRoughnessFactor` (default `0.0`) — clearcoat layer
///   roughness.
/// * `clearcoatRoughnessTexture` (a `textureInfo`) — the roughness
///   texture; its `.g` channel multiplies `clearcoatRoughnessFactor`.
/// * `clearcoatNormalTexture` (a `normalTextureInfo`, so it carries an
///   optional `scale`) — the clearcoat layer's normal map.
///
/// All five fields are optional per the spec.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialClearcoat {
    #[serde(
        rename = "clearcoatFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub clearcoat_factor: Option<f32>,
    #[serde(
        rename = "clearcoatTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub clearcoat_texture: Option<TextureInfo>,
    #[serde(
        rename = "clearcoatRoughnessFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub clearcoat_roughness_factor: Option<f32>,
    #[serde(
        rename = "clearcoatRoughnessTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub clearcoat_roughness_texture: Option<TextureInfo>,
    #[serde(
        rename = "clearcoatNormalTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub clearcoat_normal_texture: Option<NormalTextureInfo>,
}

/// `KHR_materials_sheen` extension object — layers a sheen BRDF (used
/// to model cloth / fabric) on top of the metallic-roughness material.
/// Adds an RGB colour factor, a scalar roughness factor, and two
/// optional texture references per
/// `docs/3d/gltf/extensions/KHR_materials_sheen.md` §Extending Materials
/// §Sheen:
///
/// * `sheenColorFactor` (default `[0.0, 0.0, 0.0]`) — the sheen colour
///   in linear space; when zero the whole sheen layer is disabled.
/// * `sheenColorTexture` (a `textureInfo`) — the sheen colour (RGB) in
///   the sRGB transfer function; its RGB channels multiply the factor.
/// * `sheenRoughnessFactor` (default `0.0`) — the sheen roughness.
/// * `sheenRoughnessTexture` (a `textureInfo`) — the sheen roughness
///   (Alpha) texture; its `.a` channel multiplies the factor.
///
/// All four fields are optional per the spec.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialSheen {
    #[serde(
        rename = "sheenColorFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sheen_color_factor: Option<[f32; 3]>,
    #[serde(
        rename = "sheenColorTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sheen_color_texture: Option<TextureInfo>,
    #[serde(
        rename = "sheenRoughnessFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sheen_roughness_factor: Option<f32>,
    #[serde(
        rename = "sheenRoughnessTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sheen_roughness_texture: Option<TextureInfo>,
}

/// `KHR_materials_transmission` extension object — makes the
/// metallic-roughness material optically transparent (light passes
/// through the surface rather than being diffusely re-emitted), enabling
/// physically-plausible glass / plastic. Adds a single scalar factor and
/// one optional texture reference per
/// `docs/3d/gltf/extensions/KHR_materials_transmission.md` §Properties:
///
/// * `transmissionFactor` (default `0.0`) — the base percentage of light
///   that is transmitted through the surface (`1.0` = 100% of the light
///   that penetrates the surface is transmitted); when zero the material
///   is fully opaque to transmission.
/// * `transmissionTexture` (a `textureInfo`) — its `.r` channel defines
///   the transmission percentage and is multiplied by
///   `transmissionFactor`.
///
/// Both fields are optional per the spec.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialTransmission {
    #[serde(
        rename = "transmissionFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transmission_factor: Option<f32>,
    #[serde(
        rename = "transmissionTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transmission_texture: Option<TextureInfo>,
}

/// `KHR_materials_volume` extension object — turns the surface into an
/// interface between volumes (the mesh defines the boundaries of a
/// homogeneous medium), enabling effects like refraction and absorption.
/// Adds two scalar factors, one optional texture reference, and an RGB
/// attenuation colour per
/// `docs/3d/gltf/extensions/KHR_materials_volume.md` §Properties:
///
/// * `thicknessFactor` (default `0.0`) — thickness of the volume beneath
///   the surface, in mesh-coordinate space; a value of `0` means the
///   material is thin-walled, anything `> 0` makes it a volume boundary
///   and requires a manifold/closed mesh. Range `[0, +inf)`.
/// * `thicknessTexture` (a `textureInfo`) — the thickness texture; its
///   `.g` channel multiplies `thicknessFactor`. Texture-sampled value
///   range is `[0, 1]`.
/// * `attenuationDistance` (default `+Infinity`) — average distance light
///   travels in the medium before interacting with a particle, in world
///   space. Range `(0, +inf)`. We treat `None` as "not specified" so the
///   spec-mandated `+Infinity` default is implicit (a finite default
///   would round-trip incorrectly through JSON, which cannot encode
///   non-finite numbers).
/// * `attenuationColor` (default `[1, 1, 1]`) — the colour that white
///   light turns into due to absorption when reaching the attenuation
///   distance.
///
/// All four fields are optional per the spec.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialVolume {
    #[serde(
        rename = "thicknessFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub thickness_factor: Option<f32>,
    #[serde(
        rename = "thicknessTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub thickness_texture: Option<TextureInfo>,
    #[serde(
        rename = "attenuationDistance",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub attenuation_distance: Option<f32>,
    #[serde(
        rename = "attenuationColor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub attenuation_color: Option<[f32; 3]>,
}

/// `KHR_materials_iridescence` extension object — adds a thin-film
/// interference layer on top of the metallic-roughness material so that
/// the hue depends on the viewing angle and the thin-film thickness, per
/// `docs/3d/gltf/extensions/KHR_materials_iridescence.md` §Properties:
///
/// * `iridescenceFactor` (default `0.0`) — iridescence intensity; when
///   zero the whole iridescence effect is disabled per §Properties
///   ("If `iridescenceFactor` is zero (default), the iridescence
///   extension has no effect on the material").
/// * `iridescenceTexture` (a `textureInfo`) — the iridescence intensity
///   texture; its `.r` channel multiplies `iridescenceFactor`.
/// * `iridescenceIor` (default `1.3`) — the index of refraction of the
///   thin-film layer; valid values are `>= 1.0`.
/// * `iridescenceThicknessMinimum` (default `100.0`) — minimum thickness
///   of the thin-film layer in nanometres; corresponds to a sampled
///   thickness texture value of `0.0`.
/// * `iridescenceThicknessMaximum` (default `400.0`) — maximum thickness
///   of the thin-film layer in nanometres; corresponds to a sampled
///   thickness texture value of `1.0`. The spec explicitly allows
///   `iridescenceThicknessMinimum > iridescenceThicknessMaximum`. When
///   no `iridescenceThicknessTexture` is present, the spec says the
///   thickness is uniformly set to `iridescenceThicknessMaximum`.
/// * `iridescenceThicknessTexture` (a `textureInfo`) — the thickness
///   texture; its `.g` channel selects between the two thickness bounds.
///
/// All six fields are optional per the spec.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialIridescence {
    #[serde(
        rename = "iridescenceFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iridescence_factor: Option<f32>,
    #[serde(
        rename = "iridescenceTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iridescence_texture: Option<TextureInfo>,
    #[serde(
        rename = "iridescenceIor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iridescence_ior: Option<f32>,
    #[serde(
        rename = "iridescenceThicknessMinimum",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iridescence_thickness_minimum: Option<f32>,
    #[serde(
        rename = "iridescenceThicknessMaximum",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iridescence_thickness_maximum: Option<f32>,
    #[serde(
        rename = "iridescenceThicknessTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub iridescence_thickness_texture: Option<TextureInfo>,
}

/// `KHR_materials_anisotropy` extension object — adds an asymmetric
/// specular lobe (the elongated highlight visible on e.g. brushed
/// metal) on top of the metallic-roughness material per
/// `docs/3d/gltf/extensions/KHR_materials_anisotropy.md` §Extending
/// Materials:
///
/// * `anisotropyStrength` (default `0.0`) — dimensionless strength in
///   the `[0, 1]` range; when zero the whole anisotropy effect is
///   disabled. When `anisotropyTexture` is present its blue channel
///   multiplies this value.
/// * `anisotropyRotation` (default `0.0`) — rotation of the anisotropy
///   in tangent / bitangent space, in radians, counter-clockwise from
///   the tangent. When `anisotropyTexture` is present this value
///   provides additional rotation to the texture vectors. The spec
///   does not bound the value (it is interpreted modulo 2π).
/// * `anisotropyTexture` (a `textureInfo`) — red and green channels
///   carry the XY components of the per-texel direction vector in
///   `[-1, 1]` tangent / bitangent space (encoded as `[0, 1]` and
///   remapped on dequantisation); blue carries strength in `[0, 1]`.
///
/// All three fields are optional per the spec.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialAnisotropy {
    #[serde(
        rename = "anisotropyStrength",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub anisotropy_strength: Option<f32>,
    #[serde(
        rename = "anisotropyRotation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub anisotropy_rotation: Option<f32>,
    #[serde(
        rename = "anisotropyTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub anisotropy_texture: Option<TextureInfo>,
}

/// `KHR_materials_dispersion` extension object — adds an optical
/// dispersion (chromatic aberration) to the volumetric transmission
/// model on top of the metallic-roughness material per
/// `docs/3d/gltf/extensions/KHR_materials_dispersion.md` §Extending
/// Materials.
///
/// * `dispersion` (default `0.0`) — strength of the dispersion effect,
///   stored as `20/Vd` where `Vd` is the Abbe number. A value of `0`
///   means no dispersion (the default for backwards compatibility);
///   any non-negative value is valid, with `[0, 1]` covering the
///   realistic range. Values above `1.0` exaggerate the effect for
///   artistic use. Backed by the same `20/Vd` transform that Adobe
///   Standard Material and ASWF OpenPBR use.
///
/// The extension only meaningfully applies when the material also
/// carries `KHR_materials_volume`; the spec ratification is independent
/// of that pairing, so we don't enforce it here.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialDispersion {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispersion: Option<f32>,
}

/// `KHR_materials_diffuse_transmission` extension object — models
/// diffusely-transmitted light through an infinitely-thin surface
/// (leaves, paper, candle wax …) on top of the metallic-roughness
/// material per `docs/3d/gltf/extensions/
/// KHR_materials_diffuse_transmission.md` §Extending Materials.
///
/// * `diffuseTransmissionFactor` (default `0.0`) — percentage of
///   non-specularly-reflected light that is diffusely transmitted
///   through the surface. A value of `1.0` indicates 100% of the
///   penetrating light is transmitted; `0.0` (the default) disables
///   the layer.
/// * `diffuseTransmissionTexture` (a `textureInfo`) — the factor is
///   sampled from the texture's alpha channel and multiplied by
///   `diffuseTransmissionFactor`.
/// * `diffuseTransmissionColorFactor` (default `[1, 1, 1]`) — the
///   colour that modulates the transmitted light (attenuation is the
///   complement: `1.0 - diffuseTransmissionColor`).
/// * `diffuseTransmissionColorTexture` (a `textureInfo`) — the colour
///   is sampled from the texture's RGB channels (sRGB-encoded) and
///   multiplied by `diffuseTransmissionColorFactor`.
///
/// All four fields are optional per the spec. The extension is
/// mutually exclusive with `KHR_materials_pbrSpecularGlossiness` and
/// `KHR_materials_unlit` per the spec §Exclusions; we don't enforce
/// those pairings here (neither sibling is in the registry's
/// per-material set).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialDiffuseTransmission {
    #[serde(
        rename = "diffuseTransmissionFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub diffuse_transmission_factor: Option<f32>,
    #[serde(
        rename = "diffuseTransmissionTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub diffuse_transmission_texture: Option<TextureInfo>,
    #[serde(
        rename = "diffuseTransmissionColorFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub diffuse_transmission_color_factor: Option<[f32; 3]>,
    #[serde(
        rename = "diffuseTransmissionColorTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub diffuse_transmission_color_texture: Option<TextureInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PbrMetallicRoughness {
    #[serde(
        rename = "baseColorFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub base_color_factor: Option<[f32; 4]>,
    #[serde(
        rename = "baseColorTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub base_color_texture: Option<TextureInfo>,
    #[serde(
        rename = "metallicFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub metallic_factor: Option<f32>,
    #[serde(
        rename = "roughnessFactor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub roughness_factor: Option<f32>,
    #[serde(
        rename = "metallicRoughnessTexture",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub metallic_roughness_texture: Option<TextureInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TextureInfo {
    pub index: u32,
    #[serde(rename = "texCoord", default, skip_serializing_if = "Option::is_none")]
    pub tex_coord: Option<u32>,
    /// Per-textureInfo `extensions` block. Today this carries
    /// `KHR_texture_transform` — an affine offset/rotation/scale on
    /// the UV coordinates per
    /// `docs/3d/gltf/extensions/KHR_texture_transform.md`; future
    /// per-textureInfo KHR extensions land here too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<TextureInfoExtensions>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NormalTextureInfo {
    pub index: u32,
    #[serde(rename = "texCoord", default, skip_serializing_if = "Option::is_none")]
    pub tex_coord: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
    /// Per-textureInfo `extensions` block — same shape as the one on
    /// [`TextureInfo`], surfacing `KHR_texture_transform` per
    /// `docs/3d/gltf/extensions/KHR_texture_transform.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<TextureInfoExtensions>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct OcclusionTextureInfo {
    pub index: u32,
    #[serde(rename = "texCoord", default, skip_serializing_if = "Option::is_none")]
    pub tex_coord: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f32>,
    /// Per-textureInfo `extensions` block — same shape as the one on
    /// [`TextureInfo`], surfacing `KHR_texture_transform` per
    /// `docs/3d/gltf/extensions/KHR_texture_transform.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<TextureInfoExtensions>,
}

/// Per-textureInfo `extensions` block. Models the per-textureInfo KHR
/// extensions the crate understands: today just `KHR_texture_transform`
/// (offset / rotation / scale applied to the texture's UV coordinates
/// per `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
/// Updates). Future per-textureInfo KHR extensions land here too.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TextureInfoExtensions {
    #[serde(
        rename = "KHR_texture_transform",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_texture_transform: Option<TextureTransform>,
}

/// `KHR_texture_transform` extension object — an affine 2D transform
/// applied to the UV coordinates of any `textureInfo`. The transform is
/// `translation × rotation × scale` applied as a `mat3` to the
/// homogeneous UV vector `(u, v, 1)`. Per the spec §glTF Schema
/// Updates all four fields are optional:
///
/// * `offset` (default `[0.0, 0.0]`) — UV-space translation, in
///   texture-dimension factors.
/// * `rotation` (default `0.0`) — counter-clockwise rotation in
///   radians around the UV origin (equivalent to a clockwise rotation
///   of the image).
/// * `scale` (default `[1.0, 1.0]`) — multiplicative scale applied to
///   the UV components.
/// * `texCoord` — overrides the parent `textureInfo.texCoord` value
///   only if the consumer supports this extension; the underlying
///   texCoord remains the fallback for engines that don't.
///
/// See `docs/3d/gltf/extensions/KHR_texture_transform.md`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TextureTransform {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 2]>,
    #[serde(rename = "texCoord", default, skip_serializing_if = "Option::is_none")]
    pub tex_coord: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Texture {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Image {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(
        rename = "bufferView",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub buffer_view: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Sampler {
    #[serde(rename = "magFilter", default, skip_serializing_if = "Option::is_none")]
    pub mag_filter: Option<u32>,
    #[serde(rename = "minFilter", default, skip_serializing_if = "Option::is_none")]
    pub min_filter: Option<u32>,
    #[serde(rename = "wrapS", default, skip_serializing_if = "Option::is_none")]
    pub wrap_s: Option<u32>,
    #[serde(rename = "wrapT", default, skip_serializing_if = "Option::is_none")]
    pub wrap_t: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Camera {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perspective: Option<CameraPerspective>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orthographic: Option<CameraOrthographic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CameraPerspective {
    #[serde(
        rename = "aspectRatio",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub aspect_ratio: Option<f32>,
    pub yfov: f32,
    pub znear: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zfar: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CameraOrthographic {
    pub xmag: f32,
    pub ymag: f32,
    pub znear: f32,
    pub zfar: f32,
}

/// `extensions` block at root scope. Currently we surface
/// `KHR_lights_punctual` (the punctual-lights light table lives there
/// per the extension spec) and the root-level variant roster for
/// `KHR_materials_variants`; other extensions pass through as `extras`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RootExtensions {
    #[serde(
        rename = "KHR_lights_punctual",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_lights_punctual: Option<KhrLightsPunctualRoot>,
    /// `KHR_materials_variants` root-level variant roster per
    /// `docs/3d/gltf/extensions/KHR_materials_variants.md`.
    #[serde(
        rename = "KHR_materials_variants",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_materials_variants: Option<KhrMaterialsVariantsRoot>,
    /// `KHR_xmp_json_ld` root-level XMP metadata packet roster per
    /// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Defining XMP
    /// Metadata". Each packet is an opaque JSON-LD object; consumers
    /// of the typed model treat the contents as round-trip JSON.
    #[serde(
        rename = "KHR_xmp_json_ld",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_xmp_json_ld: Option<KhrXmpJsonLdRoot>,
}

/// `extensions.KHR_xmp_json_ld` root-level object per
/// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Defining XMP
/// Metadata" — the array of metadata packets the document declares.
/// Each packet is a JSON-LD object held verbatim because the spec
/// describes a restricted JSON-LD subset (§"JSON-LD Restrictions and
/// Recommendations") but does not constrain the namespace vocabulary.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KhrXmpJsonLdRoot {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packets: Vec<Value>,
}

/// `extensions.KHR_materials_variants` root-level object — the array
/// of named variants the document can switch between.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KhrMaterialsVariantsRoot {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<MaterialVariant>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialVariant {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KhrLightsPunctualRoot {
    pub lights: Vec<KhrLight>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KhrLight {
    /// `"directional"`, `"point"`, or `"spot"` per the extension spec.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot: Option<KhrLightSpot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KhrLightSpot {
    #[serde(
        rename = "innerConeAngle",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub inner_cone_angle: Option<f32>,
    #[serde(
        rename = "outerConeAngle",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub outer_cone_angle: Option<f32>,
}

/// Per-node `extensions` block. Used by `KHR_lights_punctual` to point
/// a node at one of the root `lights[]`, and by `KHR_node_visibility`
/// to toggle a node subtree's visibility per
/// `docs/3d/gltf/extensions/KHR_node_visibility.md`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NodeExtensions {
    #[serde(
        rename = "KHR_lights_punctual",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_lights_punctual: Option<NodeLightRef>,
    /// `KHR_node_visibility` — boolean `visible` flag that hides the
    /// node and its descendant subtree per
    /// `docs/3d/gltf/extensions/KHR_node_visibility.md`.
    #[serde(
        rename = "KHR_node_visibility",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_node_visibility: Option<NodeVisibility>,
    /// `KHR_xmp_json_ld` — `{ "packet": N }` indirection that points
    /// the node at one of the root-level XMP packets per
    /// `docs/3d/gltf/extensions/KHR_xmp_json_ld.md` §"Instantiating
    /// XMP metadata".
    #[serde(
        rename = "KHR_xmp_json_ld",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_xmp_json_ld: Option<XmpPacketRef>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NodeLightRef {
    pub light: u32,
}

/// `KHR_node_visibility` extension object — a single optional boolean
/// `visible` property per
/// `docs/3d/gltf/extensions/KHR_node_visibility.md` §Extending Nodes.
/// The spec defines `visible` as optional with a default of `true`;
/// a value of `false` hides the node and all its descendants
/// (visibility is the logical AND of every ancestor's value).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NodeVisibility {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

/// `animations[i]` — a bag of channels played as one timeline.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Animation {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<AnimationChannel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samplers: Vec<AnimationSampler>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AnimationChannel {
    pub sampler: u32,
    pub target: AnimationChannelTarget,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AnimationChannelTarget {
    /// `None` → channel SHOULD be ignored per spec §3.11.
    ///
    /// Also `None` for channels carrying the `KHR_animation_pointer`
    /// extension — the spec explicitly forbids setting both `node` and
    /// the pointer extension on the same target (see
    /// `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Extension
    /// Usage").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<u32>,
    /// `"translation"`, `"rotation"`, `"scale"`, or `"weights"` for
    /// base-spec channels. `"pointer"` flags a channel that targets an
    /// arbitrary mutable property via `KHR_animation_pointer` —
    /// see `docs/3d/gltf/extensions/KHR_animation_pointer.md`.
    pub path: String,
    /// Per-channel-target extensions block. Currently
    /// `KHR_animation_pointer` is the only extension that lives here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<AnimationChannelTargetExtensions>,
}

/// `animations[i].channels[j].target.extensions` — the per-channel-target
/// extensions block. `KHR_animation_pointer` lives here per
/// `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Extension Usage".
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AnimationChannelTargetExtensions {
    /// `KHR_animation_pointer` — JSON Pointer (RFC 6901) string
    /// indirection into any mutable glTF property per the spec's
    /// §Operation. The channel `target.path` MUST be `"pointer"` when
    /// this is present, and `target.node` MUST NOT be set.
    #[serde(
        rename = "KHR_animation_pointer",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub khr_animation_pointer: Option<AnimationPointer>,
}

/// `KHR_animation_pointer` extension object — a single required `pointer`
/// string per `docs/3d/gltf/extensions/KHR_animation_pointer.md`
/// §"Extension Usage". The pointer is a JSON Pointer (RFC 6901) into
/// any mutable glTF asset property as defined by the glTF 2.0 Asset
/// Object Model.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AnimationPointer {
    pub pointer: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AnimationSampler {
    /// Accessor index for the keyframe times (SCALAR float, monotonic).
    pub input: u32,
    /// Accessor index for the per-keyframe values.
    pub output: u32,
    /// `"LINEAR"` (default), `"STEP"`, or `"CUBICSPLINE"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpolation: Option<String>,
}

/// `skins[i]` — joint roster + (optional) inverse-bind-matrix accessor.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Skin {
    #[serde(
        rename = "inverseBindMatrices",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub inverse_bind_matrices: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<u32>,
    pub joints: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

// ----- componentType + topology constants per spec §3.6.2.2 / §3.7.2.1 -----

pub const COMPONENT_TYPE_BYTE: u32 = 5120;
pub const COMPONENT_TYPE_UNSIGNED_BYTE: u32 = 5121;
pub const COMPONENT_TYPE_SHORT: u32 = 5122;
pub const COMPONENT_TYPE_UNSIGNED_SHORT: u32 = 5123;
pub const COMPONENT_TYPE_UNSIGNED_INT: u32 = 5125;
pub const COMPONENT_TYPE_FLOAT: u32 = 5126;

pub const MODE_POINTS: u32 = 0;
pub const MODE_LINES: u32 = 1;
pub const MODE_LINE_LOOP: u32 = 2;
pub const MODE_LINE_STRIP: u32 = 3;
pub const MODE_TRIANGLES: u32 = 4;
pub const MODE_TRIANGLE_STRIP: u32 = 5;
pub const MODE_TRIANGLE_FAN: u32 = 6;

pub const TARGET_ARRAY_BUFFER: u32 = 34962;
pub const TARGET_ELEMENT_ARRAY_BUFFER: u32 = 34963;

pub const MAG_FILTER_NEAREST: u32 = 9728;
pub const MAG_FILTER_LINEAR: u32 = 9729;
pub const MIN_FILTER_NEAREST: u32 = 9728;
pub const MIN_FILTER_LINEAR: u32 = 9729;
pub const MIN_FILTER_NEAREST_MIPMAP_NEAREST: u32 = 9984;
pub const MIN_FILTER_LINEAR_MIPMAP_NEAREST: u32 = 9985;
pub const MIN_FILTER_NEAREST_MIPMAP_LINEAR: u32 = 9986;
pub const MIN_FILTER_LINEAR_MIPMAP_LINEAR: u32 = 9987;

pub const WRAP_CLAMP_TO_EDGE: u32 = 33071;
pub const WRAP_MIRRORED_REPEAT: u32 = 33648;
pub const WRAP_REPEAT: u32 = 10497;

/// glTF `type` field component-count lookup.
pub fn type_components(kind: &str) -> Option<u32> {
    match kind {
        "SCALAR" => Some(1),
        "VEC2" => Some(2),
        "VEC3" => Some(3),
        "VEC4" => Some(4),
        "MAT2" => Some(4),
        "MAT3" => Some(9),
        "MAT4" => Some(16),
        _ => None,
    }
}

/// Size of one component in bytes.
pub fn component_size(component_type: u32) -> Option<u32> {
    match component_type {
        COMPONENT_TYPE_BYTE | COMPONENT_TYPE_UNSIGNED_BYTE => Some(1),
        COMPONENT_TYPE_SHORT | COMPONENT_TYPE_UNSIGNED_SHORT => Some(2),
        COMPONENT_TYPE_UNSIGNED_INT | COMPONENT_TYPE_FLOAT => Some(4),
        _ => None,
    }
}
