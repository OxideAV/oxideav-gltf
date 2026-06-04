# oxideav-gltf

Pure-Rust **glTF 2.0** codec (Khronos KHR-public spec, royalty-free) —
decodes and encodes both the `.gltf` JSON variant and the `.glb` binary
container. Implements the [`oxideav-mesh3d`](https://github.com/OxideAV/oxideav-mesh3d)
`Mesh3DDecoder` + `Mesh3DEncoder` traits.

Part of the [oxideav](https://github.com/OxideAV/oxideav-workspace)
framework but usable standalone.

## What's covered

- `.gltf` JSON document read + write
- `.glb` binary container read + write (header + JSON chunk + BIN chunk)
- glTF 2.0 PBR metallic-roughness materials (base colour / metallic /
  roughness / normal / occlusion / emissive — factors + textures, with
  `alphaMode` and `doubleSided`)
- Multi-primitive meshes, all 7 topologies (POINTS through TRIANGLE_FAN)
- Vertex attributes: POSITION, NORMAL, TANGENT, TEXCOORD_n (all sets),
  COLOR_n (VEC3 promoted to RGBA), JOINTS_0, WEIGHTS_0
- Indices in any of the three spec-allowed widths
  (UNSIGNED_BYTE / UNSIGNED_SHORT / UNSIGNED_INT) — encoder picks the
  narrowest representable
- Cameras: perspective + orthographic
- KHR_lights_punctual extension (directional / point / spot)
- KHR_materials_unlit extension (Khronos ratified) — per-material
  Boolean shading-model flag from
  `docs/3d/gltf/extensions/KHR_materials_unlit.md`. The decoder lifts
  the JSON `materials[i].extensions.KHR_materials_unlit` data block
  into the typed `oxideav_mesh3d::Material::extras["KHR_materials_unlit"]
  = Bool(true)` side-channel; the encoder rebuilds the empty `{}`
  extension object on write and appends `KHR_materials_unlit` to
  `extensionsUsed`. The §3.12 stack validator rejects materials
  carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_emissive_strength extension (Khronos ratified) —
  per-material scalar emissive multiplier from
  `docs/3d/gltf/extensions/KHR_materials_emissive_strength.md`. The
  decoder lifts the JSON `materials[i].extensions.KHR_materials_emissive_strength.emissiveStrength`
  value into the typed
  `oxideav_mesh3d::Material::extras["KHR_materials_emissive_strength"]`
  side-channel as a JSON number (a bare `{}` object resolves to the
  spec default of `1.0`); the encoder rebuilds the extension object on
  write and appends `KHR_materials_emissive_strength` to
  `extensionsUsed`. The §3.12 stack validator rejects materials
  carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_ior extension (Khronos ratified) — per-material scalar
  index of refraction from `docs/3d/gltf/extensions/KHR_materials_ior.md`.
  The decoder lifts the JSON `materials[i].extensions.KHR_materials_ior.ior`
  value into the typed `oxideav_mesh3d::Material::extras["KHR_materials_ior"]`
  side-channel as a JSON number (a bare `{}` object resolves to the spec
  default of `1.5`; the `ior == 0` specular-glossiness
  backwards-compatibility sentinel is carried through verbatim); the
  encoder rebuilds the extension object on write and appends
  `KHR_materials_ior` to `extensionsUsed`. The §3.12 stack validator
  rejects materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_specular extension (Khronos ratified) — per-material
  specular reflection factor + F0 colour + optional textures from
  `docs/3d/gltf/extensions/KHR_materials_specular.md`. The decoder lifts
  the full JSON `materials[i].extensions.KHR_materials_specular` object
  into `oxideav_mesh3d::Material::extras["KHR_materials_specular"]` as a
  JSON `Value::Object` carrying any of the four spec-defined keys
  (`specularFactor`, `specularTexture`, `specularColorFactor`,
  `specularColorTexture`); a bare `{}` resolves to the spec defaults
  `specularFactor = 1.0` and `specularColorFactor = [1, 1, 1]`,
  `specularColorFactor` components above `1.0` pass through unclamped
  per the spec, and `specularTexture` / `specularColorTexture` infos
  round-trip with both `index` and optional `texCoord` preserved. The
  encoder lifts the object back into the typed extensions block and
  appends `KHR_materials_specular` to `extensionsUsed`. The §3.12 stack
  validator rejects materials carrying the data block without the
  declaration (`ExtensionStackUsedNotDeclared`)
- KHR_materials_clearcoat extension (Khronos ratified) — per-material
  clear-coat layer (intensity + roughness scalar factors + optional
  textures) from `docs/3d/gltf/extensions/KHR_materials_clearcoat.md`.
  The decoder lifts the full JSON
  `materials[i].extensions.KHR_materials_clearcoat` object into
  `oxideav_mesh3d::Material::extras["KHR_materials_clearcoat"]` as a JSON
  `Value::Object` carrying any of the five spec-defined keys
  (`clearcoatFactor`, `clearcoatTexture`, `clearcoatRoughnessFactor`,
  `clearcoatRoughnessTexture`, `clearcoatNormalTexture`); a bare `{}`
  resolves to the spec defaults `clearcoatFactor = 0.0` and
  `clearcoatRoughnessFactor = 0.0` (a zero `clearcoatFactor` disables the
  whole layer per the spec). `clearcoatTexture` /
  `clearcoatRoughnessTexture` are `textureInfo` (round-trip `index` +
  optional `texCoord`); `clearcoatNormalTexture` is a `normalTextureInfo`
  so it additionally round-trips an optional `scale`. The encoder lifts
  the object back into the typed extensions block and appends
  `KHR_materials_clearcoat` to `extensionsUsed`. The §3.12 stack
  validator rejects materials carrying the data block without the
  declaration (`ExtensionStackUsedNotDeclared`)
- KHR_materials_sheen extension (Khronos ratified) — per-material sheen
  BRDF (cloth / fabric) from
  `docs/3d/gltf/extensions/KHR_materials_sheen.md`. The decoder lifts the
  full JSON `materials[i].extensions.KHR_materials_sheen` object into
  `oxideav_mesh3d::Material::extras["KHR_materials_sheen"]` as a JSON
  `Value::Object` carrying any of the four spec-defined keys
  (`sheenColorFactor`, `sheenColorTexture`, `sheenRoughnessFactor`,
  `sheenRoughnessTexture`); a bare `{}` resolves to the spec defaults
  `sheenColorFactor = [0, 0, 0]` and `sheenRoughnessFactor = 0.0` (a zero
  `sheenColorFactor` disables the whole layer per the spec), and the
  `sheenColorTexture` / `sheenRoughnessTexture` infos round-trip with both
  `index` and optional `texCoord` preserved. The encoder lifts the object
  back into the typed extensions block and appends `KHR_materials_sheen`
  to `extensionsUsed`. The §3.12 stack validator rejects materials
  carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_transmission extension (Khronos ratified) —
  per-material optical-transparency factor + optional texture from
  `docs/3d/gltf/extensions/KHR_materials_transmission.md`. The decoder
  lifts the full JSON
  `materials[i].extensions.KHR_materials_transmission` object into
  `oxideav_mesh3d::Material::extras["KHR_materials_transmission"]` as a
  JSON `Value::Object` carrying either of the two spec-defined keys
  (`transmissionFactor`, `transmissionTexture`); a bare `{}` resolves
  to the spec default `transmissionFactor = 0.0`, and the
  `transmissionTexture` info round-trips with both `index` and
  optional `texCoord` preserved. The encoder lifts the object back
  into the typed extensions block and appends
  `KHR_materials_transmission` to `extensionsUsed`. The §3.12 stack
  validator rejects materials carrying the data block without the
  declaration (`ExtensionStackUsedNotDeclared`)
- KHR_materials_volume extension (Khronos ratified) — per-material
  homogeneous volumetric medium (thickness + attenuation) from
  `docs/3d/gltf/extensions/KHR_materials_volume.md`. The decoder lifts
  the full JSON `materials[i].extensions.KHR_materials_volume` object
  into `oxideav_mesh3d::Material::extras["KHR_materials_volume"]` as a
  JSON `Value::Object` carrying any of the four spec-defined keys
  (`thicknessFactor`, `thicknessTexture`, `attenuationDistance`,
  `attenuationColor`). A bare `{}` resolves to the spec defaults
  `thicknessFactor = 0.0` (thin-walled) and
  `attenuationColor = [1, 1, 1]`; `attenuationDistance` defaults to
  `+Infinity` per the spec — JSON cannot encode non-finite numbers, so
  the decoder leaves the key absent and consumers interpret missing-key
  as +Infinity. `thicknessTexture` is a `textureInfo` (round-trip
  `index` + optional `texCoord` preserved). The encoder lifts the
  object back into the typed extensions block and appends
  `KHR_materials_volume` to `extensionsUsed`. The §3.12 stack validator
  rejects materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_iridescence extension (Khronos ratified) — per-material
  thin-film interference layer (hue varies with viewing angle + thickness)
  from `docs/3d/gltf/extensions/KHR_materials_iridescence.md`. The decoder
  lifts the full JSON `materials[i].extensions.KHR_materials_iridescence`
  object into `oxideav_mesh3d::Material::extras["KHR_materials_iridescence"]`
  as a JSON `Value::Object` carrying any of the six spec-defined keys
  (`iridescenceFactor`, `iridescenceTexture`, `iridescenceIor`,
  `iridescenceThicknessMinimum`, `iridescenceThicknessMaximum`,
  `iridescenceThicknessTexture`). A bare `{}` resolves to the spec
  defaults `iridescenceFactor = 0.0` (zero disables the layer per
  §Properties), `iridescenceIor = 1.3`, `iridescenceThicknessMinimum =
  100.0`, `iridescenceThicknessMaximum = 400.0` (all in nanometres). The
  spec explicitly allows `iridescenceThicknessMinimum >
  iridescenceThicknessMaximum`, so inverted ranges pass through
  unmodified. `iridescenceTexture` and `iridescenceThicknessTexture` are
  `textureInfo` (round-trip `index` + optional `texCoord` preserved). The
  encoder lifts the object back into the typed extensions block and
  appends `KHR_materials_iridescence` to `extensionsUsed`. The §3.12
  stack validator rejects materials carrying the data block without the
  declaration (`ExtensionStackUsedNotDeclared`)
- KHR_materials_anisotropy extension (Khronos ratified) — per-material
  asymmetric specular lobe (the elongated highlight visible on e.g.
  brushed metal) from
  `docs/3d/gltf/extensions/KHR_materials_anisotropy.md`. The decoder
  lifts the full JSON `materials[i].extensions.KHR_materials_anisotropy`
  object into `oxideav_mesh3d::Material::extras["KHR_materials_anisotropy"]`
  as a JSON `Value::Object` carrying any of the three spec-defined keys
  (`anisotropyStrength`, `anisotropyRotation`, `anisotropyTexture`); a
  bare `{}` resolves to the spec defaults `anisotropyStrength = 0.0`
  (zero disables the effect) and `anisotropyRotation = 0.0` radians.
  `anisotropyTexture` is a `textureInfo` (round-trip `index` + optional
  `texCoord` preserved). The encoder lifts the object back into the
  typed extensions block and appends `KHR_materials_anisotropy` to
  `extensionsUsed`. The §3.12 stack validator additionally enforces the
  spec's `anisotropyStrength ∈ [0, 1]` range
  (`ExtensionStackAnisotropyStrengthRange`) and a finite-value check on
  `anisotropyRotation` (`ExtensionStackAnisotropyRotationFinite`), and
  rejects materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_dispersion extension (Khronos ratified) — per-material
  optical dispersion (chromatic aberration) modulating the volumetric
  transmission model from
  `docs/3d/gltf/extensions/KHR_materials_dispersion.md`. The extension
  carries a single `dispersion` scalar storing `20/Vd` (where `Vd` is
  the Abbe number — the same transform Adobe Standard Material and
  ASWF OpenPBR use). The decoder lifts the full JSON
  `materials[i].extensions.KHR_materials_dispersion` object into
  `oxideav_mesh3d::Material::extras["KHR_materials_dispersion"]` as a
  JSON `Value::Object` carrying the `dispersion` key; a bare `{}`
  resolves to the spec default `dispersion = 0.0` (no dispersion, the
  backwards-compatibility default). Values above `1.0` are explicitly
  allowed for artistic exaggeration (Rutile at `2.04` is the
  spec-listed example). The encoder lifts the object back into the
  typed extensions block and appends `KHR_materials_dispersion` to
  `extensionsUsed`. The §3.12 stack validator additionally enforces
  the spec's "Any value zero or larger" rule — `dispersion` MUST be
  finite and `>= 0` (`ExtensionStackDispersionRange`) — and rejects
  materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_materials_diffuse_transmission extension (Khronos ratified) —
  per-material diffuse-transmission factor + colour + optional textures
  modelling light that diffuses through infinitely-thin surfaces
  (leaves, paper, candle wax …) from
  `docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md`. The
  decoder lifts the full JSON
  `materials[i].extensions.KHR_materials_diffuse_transmission` object
  into
  `oxideav_mesh3d::Material::extras["KHR_materials_diffuse_transmission"]`
  as a JSON `Value::Object` carrying any of the four spec-defined keys
  (`diffuseTransmissionFactor`, `diffuseTransmissionTexture`,
  `diffuseTransmissionColorFactor`, `diffuseTransmissionColorTexture`);
  a bare `{}` resolves to the spec defaults
  `diffuseTransmissionFactor = 0.0` (zero disables the layer) and
  `diffuseTransmissionColorFactor = [1, 1, 1]`. The texture infos
  round-trip with both `index` and optional `texCoord` preserved. The
  encoder lifts the object back into the typed extensions block and
  appends `KHR_materials_diffuse_transmission` to `extensionsUsed`.
  The §3.12 stack validator additionally enforces the spec's implicit
  range constraints — `diffuseTransmissionFactor` MUST be finite and
  within `[0, 1]` (the spec defines `1.0` as 100% of the penetrating
  light being transmitted —
  `ExtensionStackDiffuseTransmissionFactorRange`), each component of
  `diffuseTransmissionColorFactor` MUST be finite and within `[0, 1]`
  (it is a "proportion of light at each color channel" —
  `ExtensionStackDiffuseTransmissionColorRange`) — and rejects
  materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`)
- KHR_texture_transform extension (Khronos ratified) — per-textureInfo
  affine UV transform (offset / rotation / scale / texCoord) from
  `docs/3d/gltf/extensions/KHR_texture_transform.md`. The decoder lifts
  the `materials[i].…Texture.extensions.KHR_texture_transform` object
  from each of the five core PBR texture slots into
  `oxideav_mesh3d::Material::extras["KHR_texture_transform:<slot>"]`
  (slot ∈ `baseColor` / `metallicRoughness` / `normal` / `occlusion` /
  `emissive`) as a JSON `Value::Object` carrying any of the four
  spec-defined keys (`offset`, `rotation`, `scale`, `texCoord`); a bare
  `{}` resolves to the spec defaults `offset = [0, 0]`, `rotation = 0`,
  `scale = [1, 1]` (materialised at use time). The encoder lifts each
  slot's transform back into the typed textureInfo extensions block and
  appends `KHR_texture_transform` to `extensionsUsed`. The §3.12 stack
  validator rejects textureInfos carrying the data block without the
  declaration (`ExtensionStackUsedNotDeclared`)
- KHR_node_visibility extension (Khronos ratified) — per-node Boolean
  `visible` flag from `docs/3d/gltf/extensions/KHR_node_visibility.md`.
  The decoder lifts the JSON `nodes[i].extensions.KHR_node_visibility.visible`
  value into the typed `oxideav_mesh3d::Node::extras["KHR_node_visibility"]`
  side-channel as a JSON boolean; a bare `{}` object resolves to the
  spec default of `true` (the spec defines `visible` as optional with
  default `true` per §Extending Nodes). The encoder lifts the boolean
  back into the typed `KHR_node_visibility` extension object on write
  and appends `KHR_node_visibility` to `extensionsUsed`. The §3.12
  stack validator rejects nodes carrying the data block without the
  declaration (`ExtensionStackUsedNotDeclared`). The two per-node
  extensions (`KHR_lights_punctual` + `KHR_node_visibility`) coexist
  on a single node so a hidden subtree can still own a light
- KHR_xmp_json_ld extension (Khronos ratified) — XMP (ISO 16684-1)
  metadata indirection from `docs/3d/gltf/extensions/KHR_xmp_json_ld.md`.
  Defines a root-level `extensions.KHR_xmp_json_ld.packets[]` array of
  opaque JSON-LD packets (§"Defining XMP Metadata") plus a
  `{ "packet": N }` indirection emitted on the `asset`, `scene`, `node`,
  `mesh`, or `material` object (§"Instantiating XMP metadata"). Decoder
  surfaces the root roster through
  `oxideav_mesh3d::Scene3D::extras["KHR_xmp_json_ld"]` as
  `{ "packets": [...] }` with each packet held verbatim (the spec
  defines a restricted JSON-LD subset in §"Restrictions and
  Recommendations" but does not pin the namespace vocabulary); per-asset
  and per-primary-scene packet refs surface as bare JSON numbers under
  `Scene3D::extras["__asset_xmp_packet"]` /
  `Scene3D::extras["__primary_scene_xmp_packet"]`; per-node and
  per-material refs surface under the matching
  `Node::extras["KHR_xmp_json_ld"]` /
  `Material::extras["KHR_xmp_json_ld"]`; per-mesh refs ride
  `primitive[0].extras["__mesh_xmp_packet"]` because
  `oxideav_mesh3d::Mesh` has no `extras` field. Encoder lifts every side
  channel back into the typed extension block and declares
  `KHR_xmp_json_ld` in `extensionsUsed` whenever any scope surfaces the
  data. The §3.12 stack validator rejects documents carrying the data
  block without the declaration with `ExtensionStackUsedNotDeclared`,
  and additionally enforces the spec's indirection model by rejecting
  every per-object `{ "packet": N }` reference whose index lies outside
  the root `packets[]` array (`ExtensionStackXmpPacketIndex`)
- KHR_materials_variants extension (Khronos ratified) — named
  document-level variants paired with per-primitive material mappings
  from `docs/3d/gltf/extensions/KHR_materials_variants.md`. The decoder
  lifts the root-level `extensions.KHR_materials_variants.variants`
  roster into `oxideav_mesh3d::Scene3D::extras["KHR_materials_variants"]`
  as a JSON `Value::Object` of the form
  `{ "variants": [ { "name": "...", "extras": ... }, ... ] }`, and each
  primitive's `extensions.KHR_materials_variants.mappings` list into
  `oxideav_mesh3d::Primitive::extras["KHR_materials_variants"]` as
  `{ "mappings": [ { "material": idx, "variants": [idx, ...], "name": "...",
  "extras": ... }, ... ] }`. The encoder lifts both back into the typed
  root + primitive extension blocks on write and appends
  `KHR_materials_variants` to `extensionsUsed` whenever a roster or any
  primitive mapping is present. The §3.12 stack validator additionally
  enforces three spec-explicit value-range rules: every mapping
  `material` index MUST resolve into the root `materials[]` array
  (`ExtensionStackVariantsMaterialIndex`), every variant index in a
  mapping MUST resolve into the root variants roster
  (`ExtensionStackVariantsIndex`), and across one primitive's mappings
  each variant index MUST appear at most once
  (`ExtensionStackVariantsDuplicate` — the spec's "each variant index
  must be used no more than one time" rule). Documents carrying either
  data block without the declaration are rejected with
  `ExtensionStackUsedNotDeclared`
- KHR_animation_pointer extension (Khronos ratified) — animation
  channels that drive arbitrary mutable glTF properties via a JSON
  Pointer (RFC 6901) per
  `docs/3d/gltf/extensions/KHR_animation_pointer.md`. Pointer-targeted
  channels carry `target.path = "pointer"` and store the pointer string
  at `target.extensions.KHR_animation_pointer.pointer`; the base spec
  would silently discard them because they don't bind to a node, so the
  decoder siphons them into
  `oxideav_mesh3d::Scene3D::extras["KHR_animation_pointer"]` as
  `{ "animations": [ { "animation": idx, "name": "...", "channels": [
  { "channel": ci, "pointer": "/...", "interpolation": "LINEAR" |
  "STEP" | "CUBICSPLINE", "input": [...f32...], "output_kind":
  "SCALAR" | "VEC2" | "VEC3" | "VEC4" | "MAT2" | "MAT3" | "MAT4",
  "output": [...f32...] } ] } ] }`. The encoder lifts each channel
  back into the typed `target.extensions` block (emitting fresh
  FLOAT-typed input + output accessors and a sampler) and appends
  `KHR_animation_pointer` to `extensionsUsed`. r218 carries the FLOAT
  output lane only — the spec's normalized-int and non-normalized-int
  conversion modes (per §"Output Accessor Component Types") are
  deferred to a follow-up round. The §3.12 stack validator rejects
  documents carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`); rejects pointer channels with
  `target.node` set (`ExtensionStackAnimationPointerNode` — the spec
  forbids combining the two); rejects channels with
  `target.path = "pointer"` but no extension data
  (`ExtensionStackAnimationPointerData`) and the inverse
  (`ExtensionStackAnimationPointerPath`); rejects duplicate pointer
  strings within one animation
  (`ExtensionStackAnimationPointerDuplicate` — spec §Operation:
  "different channels of the same animation MUST NOT have identical
  pointers"); and rejects malformed RFC 6901 prefixes
  (`ExtensionStackAnimationPointerSyntax`)
- KHR_mesh_quantization (Khronos ratified) decode + encode — quantized
  vertex attributes AND quantized morph-target deltas from
  `docs/3d/gltf/extensions/KHR_mesh_quantization.md`. Base mesh
  attributes (`POSITION` / `NORMAL` / `TANGENT` / `TEXCOORD_n`) may
  store 8-/16-bit signed/unsigned integers (normalized or
  unnormalized) in place of `FLOAT`; morph-target deltas
  (`POSITION` / `NORMAL` / `TANGENT` VEC3 and `TEXCOORD_n` VEC2, with
  TANGENT.w dropped on morph deltas per §3.7.2.2) may store BYTE /
  SHORT signed integers (normalized or, for POSITION / TEXCOORD,
  unnormalized) per §Extending Morph Target Attributes. The decoder
  dequantizes per the spec int→float table — BYTE
  `f = max(c/127, -1)`, UNSIGNED_BYTE `f = c/255`, SHORT
  `f = max(c/32767, -1)`, UNSIGNED_SHORT `f = c/65535`; unnormalized
  integers cast directly to `f32`. A quantized base or morph attribute
  is gated on `KHR_mesh_quantization` being declared in
  `extensionsUsed` and the (componentType, normalized) pair being in
  the extension's allowed set for that attribute; the base-spec
  UNSIGNED_BYTE / UNSIGNED_SHORT *normalized* `TEXCOORD` types stay
  accepted without the extension. Each quantized base attribute's
  storage form (componentType + normalized) is recorded under the
  primitive `extras["__attr_quant"]` sentinel; each quantized morph
  delta's storage form is recorded under
  `extras["__morph_attr_quant"]` keyed by
  `<target-index>.<attribute>`. The encoder picks both sentinels back
  up on write and re-quantises each attribute via the spec float→int
  table — BYTE `c = round(f * 127)`, UBYTE `c = round(f * 255)`, SHORT
  `c = round(f * 32767)`, USHORT `c = round(f * 65535)` — padding to
  the spec-mandated 4-byte element stride (covers morph VEC3 BYTE /
  VEC2 BYTE / VEC3 SHORT as well as the base attribute strides).
  POSITION `accessor.min` / `accessor.max` carry the quantised integer
  values per the Implementation Note in §Extending Mesh Attributes.
  The encoder declares `KHR_mesh_quantization` in BOTH
  `extensionsUsed` AND `extensionsRequired` per §Overview ("files that
  use the extension must specify it in extensionsRequired array - the
  extension is not optional"). (componentType, normalized) tuples that
  fall outside the spec's allowed combo tables revert to the
  FLOAT-emit path so the encoder never produces a non-spec form
- KHR_texture_basisu extension (Khronos ratified) — per-texture
  alternative `source` indirection to a KTX v2 image with Basis
  Universal supercompression from
  `docs/3d/gltf/extensions/KHR_texture_basisu.md`. The crate is a
  pass-through engine (no KTX2 transcoding), so the decoder routes
  the texture's image load through either the spec's "with
  fallback" path (base `texture.source` PNG/JPEG present →
  pick the fallback, extension's KTX2 source is acknowledged but
  the live image is the PNG/JPEG) or the "without fallback" path
  (base `source` omitted → load the KTX2 image as opaque
  `BufferViewAsset` / `InMemoryAsset` bytes carrying the spec's
  `image/ktx2` MIME). Scene-texture indices loaded via the
  "without fallback" path are recorded under
  `Scene3D::extras["KHR_texture_basisu"].textures` so the encoder
  re-emits the same shape: `texture.source` omitted,
  `extensions.KHR_texture_basisu.source` pointing at the
  re-emitted image, and the extension declared in BOTH
  `extensionsUsed` AND `extensionsRequired` per the spec
  §"Using Without a Fallback". The §3.12 stack validator rejects
  textures carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`), out-of-range
  `KHR_texture_basisu.source` image indices
  (`ExtensionStackTextureBasisuSource`), and the "without
  fallback" shape without `KHR_texture_basisu` in
  `extensionsRequired` (`ExtensionStackTextureBasisuRequired` —
  the spec §"Using Without a Fallback" example places the
  extension name in both arrays). The §3.6.2.4-style "texture
  must have a source" rule expands to cover the new spec-allowed
  shape: a texture is invalid only when it carries neither base
  `source` nor the extension indirection
- Skins + skeletons (joint roster, inverseBindMatrices accessor,
  optional skeleton root) per spec §3.7.3 — `node.skin` round-trips
- Animations (channels + samplers) per spec §3.11 — translation /
  rotation / scale / weights paths, LINEAR + STEP + CUBICSPLINE
  interpolation
- Sparse accessors per spec §3.6.2.3 — decode + opt-in encode (the
  `GltfEncoder::with_sparse_threshold(f32)` heuristic re-emits FLOAT
  animation outputs, `skin.inverseBindMatrices`, and mesh vertex
  attributes (POSITION / NORMAL / TANGENT / COLOR_n / WEIGHTS_0) as
  `accessor.sparse` storage when their all-components-zero element
  fraction meets the threshold; POSITION keeps its spec-mandated
  min/max bounds; identity-quaternion rotation and identity-`[1,1,1]`
  scale outputs stay dense to avoid mis-representing the implicit
  values)
- Normalised-integer animation output accessors per spec §3.11 +
  §3.6.2.2 — ROTATION (VEC4) and MORPH_WEIGHTS (SCALAR) sampler
  outputs decode from `BYTE / UBYTE / SHORT / USHORT` with
  `normalized: true`, dequantising via the spec equations; and encode
  via `GltfEncoder::with_quantize_animation(QuantizeMode::UByte | UShort | IByte | IShort)`
  (round-trips within `1/255` / `1/65535` / `1/127` / `1/32767` of the
  source f32s; signed modes reserve the `-128` / `-32768` slots)
- Multi-scene documents — secondary `scenes[]` are preserved through
  round-trip via `Scene3D::extras["__additional_scenes"]`; the active
  scene index is honoured on both decode and encode
- Textures with samplers + images (buffer-view-backed images via
  `BufferViewAsset` for zero-copy slicing into the `.glb` BIN chunk;
  `data:` URI base64 inlining; external URI passthrough)
- Morph targets per spec §3.7.2.2 — POSITION / NORMAL / TANGENT
  vertex-delta accessors round-trip through
  `primitive.extras["__morph_targets"]` (mesh.weights via
  `primitive[0].extras["__mesh_weights"]`); the typed
  `oxideav_mesh3d::Primitive` model lacks a dedicated `targets` field
  pending a cross-crate followup
- Accessor `min` / `max` bounds per spec §3.6.2.1.5 — encoder fills
  missing POSITION min/max from the data; decoder validates declared
  bounds on VEC3 attribute accessors and rejects mismatches with an
  `AccessorBoundsMismatch`-prefixed error message
- Vertex-attribute compression validation per spec §3.6.2.4 (data
  alignment) + §3.7.2.1 (semantic constraints) — the decoder rejects
  six classes of spec-non-conformant attribute layouts with stable
  `VertexAttribute…`-prefixed errors: misaligned `accessor.byteOffset`
  / `bufferView.byteStride` (`VertexAttributeAlignment`), attribute
  count mismatch across a primitive (`VertexAttributeCount`), the
  primitive-restart sentinel showing up in an indices accessor
  (`VertexAttributeIndexRestart`), TANGENT.w not exactly ±1.0
  (`VertexAttributeTangentW`), and COLOR_0 components outside `[0, 1]`
  (`VertexAttributeColor0Range`). The encoder also keeps TANGENT
  dense regardless of sparse threshold to honour the same TANGENT.w
  constraint
- Extension-stack consistency validation per spec §3.12 — the decoder
  rejects documents whose `extensionsRequired` set is not a subset of
  `extensionsUsed` (`ExtensionStackRequiredNotListed`), and documents
  that materialise a `KHR_lights_punctual` data block (root or
  per-node) or a `KHR_materials_unlit` data block (per material)
  without declaring the extension in `extensionsUsed`
  (`ExtensionStackUsedNotDeclared`)
- Animation channel target-path validation per spec §3.11 — every
  channel `target.path` must be one of `translation` / `rotation` /
  `scale` / `weights` (`AnimationChannelPath`); sampler index +
  `sampler.input` / `sampler.output` accessor indices must resolve
  (`AnimationChannelSampler` / `AnimationChannelSamplerInput` /
  `AnimationChannelSamplerOutput`); `weights` channels must target a
  node bound to a mesh whose primitives declare morph targets
  (`AnimationChannelWeightsNoMesh` / `AnimationChannelWeightsNoTargets`)
- Decoder fuzz hardening — two pre-serde caps bound the JSON payload
  before it reaches the recursive parser. `check_json_byte_length`
  refuses documents larger than `MAX_JSON_BYTES` (128 MiB) with a
  `JsonTooLarge` prefix; `check_json_depth` refuses documents nesting
  deeper than `MAX_JSON_DEPTH` (256 levels) with a `JsonDepthExceeded`
  prefix. Linear-time scan that respects JSON string + escape syntax
  so a `[` inside `"..."` doesn't count. Defends against 1000-deep
  nested-array bombs that crash recursive descent on stack overflow
- Accessor-fit-in-bufferView per spec §3.6.2.4 line 3104 — the
  decoder applies the bound `accessor.byteOffset +
  EFFECTIVE_BYTE_STRIDE * (count - 1) + SIZE_OF_COMPONENT *
  NUMBER_OF_COMPONENTS <= bufferView.byteLength` to every accessor
  with a bufferView and rejects overruns with
  `AccessorFitBufferView` (also covers stride < element size,
  unknown componentType / type, and u64 overflow in the offset
  arithmetic)
- BufferView-fit-in-buffer per spec §5.11 — `bufferView.byteOffset
  + byteLength > buffer.byteLength` is rejected with
  `BufferViewFitBuffer`; `bufferView.byteStride` outside the
  JSON-schema range `[4, 252]` (§5.11.4) is rejected with
  `BufferViewStrideRange`
- Sparse-indices bufferView restrictions per spec §5.3.1 — an
  `accessor.sparse.indices.bufferView` that carries `target` or
  `byteStride` is rejected with `SparseIndicesBufferViewTarget` /
  `SparseIndicesBufferViewStride`; out-of-range indices surface as
  `SparseIndicesBufferViewIndex`
- `extras` round-trip on root, scenes, nodes, materials, primitives

## Extension roadmap (next-round work)

The KHR extension registry is now staged under
`docs/3d/gltf/extensions/` (25 specs + index), so the remaining work
is implementation, not docs:

- KHR_animation_pointer non-FLOAT output paths — the spec table in
  §"Output Accessor Component Types" allows normalized-int (BYTE /
  UBYTE / SHORT / USHORT) outputs for `float*` data types and
  non-normalized-int outputs for `int` data types plus UBYTE for
  `bool`; r218 only carries the FLOAT lane, so the other modes are
  the next iteration. Also pending: spec-aware Object-Model
  validation of the pointer string (resolving it to a mutable
  property and checking accessor-type vs data-type compatibility per
  the spec table)
- KHR_texture_basisu transcode lane — round 233 lands the per-
  texture indirection round-trip (sidecar + §3.12 validation) as
  a pass-through; an actual KTX2 / Basis Universal transcode lane
  that turns the opaque bytes into a sampled `ImageData::Embedded`
  is the next iteration, conditional on a `docs/image/ktx2/`
  spec landing
- KHR_audio_emitter wiring against `oxideav_mesh3d::AudioSource` /
  `AudioEmitter` — **blocked on DOCS-GAP**: the
  `docs/3d/gltf/extensions/` mirror is missing
  `KHR_audio_emitter.md`; the per-crate README's "lacks" tail is
  reserved for it, but implementation can't proceed without the
  spec landing under `docs/3d/gltf/extensions/`

## Installation

```toml
[dependencies]
oxideav-mesh3d = "0.0"
oxideav-gltf   = "0.0"
```

For a free-standing build that drops the `oxideav-core` dep tree:

```toml
oxideav-gltf = { version = "0.0", default-features = false }
```

## Reading a glTF file

```rust,no_run
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

let bytes = std::fs::read("scene.glb")?;
let mut decoder = GltfDecoder::new();
let scene = decoder.decode(&bytes)?;

println!("{} meshes, {} primitives, {} vertices",
    scene.meshes.len(),
    scene.meshes.iter().map(|m| m.primitives.len()).sum::<usize>(),
    scene.vertex_count(),
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The first four bytes (`b"glTF"`) trigger the binary container parse;
anything else is treated as JSON.

## Writing a `.glb`

```rust,no_run
use oxideav_gltf::GltfEncoder;
use oxideav_mesh3d::{Mesh3DEncoder, Scene3D};

let scene = Scene3D::new();
let mut enc = GltfEncoder::new(); // .glb by default
let bytes = enc.encode(&scene)?;
std::fs::write("out.glb", bytes)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

For a `.gltf` JSON file with the binary buffer inlined as a base64
`data:` URI:

```rust,no_run
use oxideav_gltf::{GltfEncoder, OutputFlavour};
use oxideav_mesh3d::{Mesh3DEncoder, Scene3D};

let scene = Scene3D::new();
let mut enc = GltfEncoder::with_output(OutputFlavour::JsonEmbedded);
let bytes = enc.encode(&scene)?;
std::fs::write("out.gltf", bytes)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Framework registration

```rust,no_run
use oxideav_mesh3d::Mesh3DRegistry;

let mut registry = Mesh3DRegistry::new();
oxideav_gltf::register(&mut registry);

assert!(registry.decoder_for_extension("gltf").is_some());
assert!(registry.decoder_for_extension("glb").is_some());
```

## Fuzz testing

`fuzz/fuzz_targets/parse.rs` is a libfuzzer harness that drives
arbitrary attacker bytes through `GltfDecoder::decode` (the magic-sniff
+ JSON-or-GLB dispatcher) and `glb::parse` (the chunk walker).
The contract under test is panic-freedom: every reachable parse path
must return a `Result` on any input — chunk-length overflow, mismatched
accessor count / componentType, buffer-view stride arithmetic,
extension dispatch on unknown names, and GLB header / chunk-alignment
violations are all expected `Err`, never aborts. Run with

```bash
cargo +nightly fuzz run parse
```

(no externally-staged corpus; the JSON depth + byte-length caps from
`validation` keep iterations bounded).

## License

[MIT](LICENSE)
