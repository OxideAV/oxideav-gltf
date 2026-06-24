# oxideav-gltf

Pure-Rust **glTF 2.0** codec (Khronos KHR-public spec, royalty-free) —
decodes and encodes both the `.gltf` JSON variant and the `.glb` binary
container. Implements the [`oxideav-mesh3d`](https://github.com/OxideAV/oxideav-mesh3d)
`Mesh3DDecoder` + `Mesh3DEncoder` traits.

Part of the [oxideav](https://github.com/OxideAV/oxideav-workspace)
framework but usable standalone.

## What's covered

- `.gltf` JSON document read + write
- `.glb` binary container read + write (header + JSON chunk + BIN chunk).
  GLB robustness per spec §4.4.2 + §4.4.3: chunk 4-byte alignment
  (`GlbChunkAlignment`), JSON-first / BIN-second ordering
  (`GlbJsonChunkOrder` / `GlbBinChunkOrder`), and exact-length policing
  — the header `length` MUST equal the file size, so trailing bytes
  appended past the declared Binary glTF length (`GlbHeaderLength`) or a
  `length` below the 12-byte header are rejected. The GLB-stored
  `buffer[0]` (uri-less, references the BIN chunk) honours the §3.6.1.2
  padding allowance: the BIN chunk MAY be up to 3 bytes larger than the
  JSON-declared `buffer.byteLength` (so a writer need not re-update it
  after 4-byte chunk padding), but a surplus of 4+ bytes is a genuine
  length mismatch and is rejected (`GlbBufferLength`); a BIN chunk
  shorter than the declared `byteLength` is likewise rejected
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
- KHR_lights_punctual extension (directional / point / spot) with
  per-light spec validation (type enum, cone-angle ordering, range
  rules, light-index bounds)
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
  appends `KHR_texture_transform` to `extensionsUsed`. Per the spec the
  transform "may be defined on `textureInfo` structures" — **any**
  textureInfo, not just the five core PBR slots — so the transform also
  rides every textureInfo nested inside a material extension
  (`KHR_materials_specular.specularTexture` /
  `specularColorTexture`, `KHR_materials_clearcoat.clearcoatTexture` /
  `clearcoatRoughnessTexture` / `clearcoatNormalTexture`,
  `KHR_materials_sheen.sheenColorTexture` / `sheenRoughnessTexture`,
  `KHR_materials_transmission.transmissionTexture`,
  `KHR_materials_volume.thicknessTexture`,
  `KHR_materials_iridescence.iridescenceTexture` /
  `iridescenceThicknessTexture`,
  `KHR_materials_anisotropy.anisotropyTexture`,
  `KHR_materials_diffuse_transmission.diffuseTransmissionTexture` /
  `diffuseTransmissionColorTexture`) — these ride through verbatim
  inside the material-extension `extras` object, and the encoder now
  declares `KHR_texture_transform` in `extensionsUsed` when a nested
  transform is present (a single exhaustive walk,
  `material_texture_transforms`, is shared between the decode-side
  §3.12 validator and the encoder's declaration scan). The §3.12 stack
  validator rejects any textureInfo — core PBR slot OR material-extension
  slot — carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`), and enforces the §Overview affine
  finiteness MUSTs on every transform: a non-finite `rotation`
  (`ExtensionStackTextureTransformRotationFinite`), `offset` component
  (`ExtensionStackTextureTransformOffsetFinite`), or `scale` component
  (`ExtensionStackTextureTransformScaleFinite`) is rejected because it
  would make the UV `mat3` non-finite
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
  "output_component_type": 5120 | 5121 | 5122 | 5123 | 5125 | 5126,
  "output_normalized": bool, "output": [...f32...] } ] } ] }`. The
  encoder lifts each channel back into the typed `target.extensions`
  block (emitting an input accessor + a sampler + an output accessor
  re-quantised to the recorded `output_component_type` +
  `output_normalized` lane) and appends `KHR_animation_pointer` to
  `extensionsUsed`. All eight `float*` Object Model Data Type
  conversion modes from §"Output Accessor Component Types" are
  covered: FLOAT pass-through, normalised BYTE / UBYTE / SHORT /
  USHORT via the §3.6.2.2 dequantisation table (`f = max(c/127, -1)`
  / `f = c/255` / `f = max(c/32767, -1)` / `f = c/65535`), and
  non-normalised BYTE / UBYTE / SHORT / USHORT / UINT cast directly
  to f32 (`1` → `1.0` per spec). Normalised UINT is rejected because
  §3.6.2.2 has no dequantisation row for it. The `bool` Object Model
  Data Type branch dispatches through the pointer-template registry in
  `object_model.rs` (seeded from the staged extension specs' §"Extending
  glTF 2.0 Asset Object Model" tables — today the single row
  `/nodes/{}/extensions/KHR_node_visibility/visible` → `bool`): a
  registry-matched channel surfaces JSON booleans in the sidecar
  (`output_data_type: "bool"`, `0` → `false`, any other value →
  `true` per §"Output Accessor Component Types") and re-encodes as a
  SCALAR UNSIGNED_BYTE 0/1 accessor with a STEP sampler. The `int`
  branch stays deferred — no staged Object Model table declares an
  `int`-typed property (core `ObjectModel.adoc` is not staged). The §3.12 stack validator rejects
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
  pointers"); rejects malformed RFC 6901 prefixes
  (`ExtensionStackAnimationPointerSyntax`); and enforces the three
  bool-lane MUSTs on registry-matched pointers — non-SCALAR output
  accessor type (`ExtensionStackAnimationPointerBoolType`), output
  componentType other than UNSIGNED_BYTE
  (`ExtensionStackAnimationPointerBoolComponentType`), and sampler
  interpolation other than STEP
  (`ExtensionStackAnimationPointerBoolInterpolation`)
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
  `source` nor the extension indirection. The
  target-image mimeType rule (§Overview + §"glTF Schema
  Updates") is also enforced — when the image referenced by
  `KHR_texture_basisu.source` declares a `mimeType` it MUST be
  `image/ktx2`, else `ExtensionStackTextureBasisuMimeType`; a
  uri-only target image with no `mimeType` stays accepted
- KHR_meshopt_compression extension (Khronos Release Candidate,
  ratified registry entry) — per-bufferView compression
  descriptors + per-buffer `{ "fallback": true }` placeholder
  markers from
  `docs/3d/gltf/extensions/KHR_meshopt_compression.md`, with a
  full Appendix A (Bitstream) + Appendix B (Filters) decoder. On
  decode every compressed bufferView is **inflated** through
  `meshopt.rs`: the descriptor's `buffer` / `byteOffset` /
  `byteLength` source range is decompressed into the parent
  bufferView's region (per §"Fallback buffers" "encoders should
  use the decompressed data to populate the fallback buffer
  view"), after which the standard accessor pipeline reads the
  real attribute / index data unchanged. All three Appendix A
  modes are covered: **ATTRIBUTES** mode 0 (byte-deinterleaved
  per-channel delta coding) for both the v0 stream (`0xa0`,
  identical to `EXT_meshopt_compression`) and the v1 stream
  (`0xa1`, with the four control modes + the three channel modes —
  byte / 2-byte zigzag deltas and 4-byte rotated XOR deltas);
  **TRIANGLES** mode 1 (edge/vertex-FIFO + `codeaux` triangle-list
  index compression with varint-7 / zigzag index deltas); and
  **INDICES** mode 2 (two-baseline generic index delta coding).
  All four Appendix B filters are applied post-decompression:
  OCTAHEDRAL (byteStride 4/8 octahedral unit-vector decode),
  QUATERNION (byteStride 8 largest-omitted quaternion decode),
  EXPONENTIAL (`2^e * m` per-lane float decode), and COLOR
  (YCoCg → RGBA). The decoder is panic-free on malformed input —
  bad header bytes, truncated streams, out-of-range FIFO reads,
  and leftover-before-tail bytes all surface as `Err`. The full
  JSON descriptor (`buffer` / `byteOffset` / `byteLength` /
  `byteStride` / `count` / `mode` / optional `filter`) is still
  captured into
  `Scene3D::extras["KHR_meshopt_compression"].bufferViews["<bvi>"]`
  so the sidecar round-trips, and every buffer marked
  `extensions.KHR_meshopt_compression.fallback = true` is recorded
  under `…fallbackBuffers` as an array of buffer indices. A
  uri-less fallback buffer (the spec's "Fallback buffers" shape)
  is materialised as a zero-filled byte vector of the declared
  `byteLength` and then overwritten by the inflated bytes. On
  encode the sidecar is stripped from
  `scene.extras` and the descriptors are NOT re-emitted onto the
  freshly-built uncompressed bufferViews — documents written by
  this crate are always uncompressed (the compression is a
  load-time concern only). The §3.12 stack validator rejects
  documents with the data block on any bufferView/buffer without
  the declaration (`ExtensionStackUsedNotDeclared`); uri-less
  fallback buffers without `KHR_meshopt_compression` in
  `extensionsRequired` (`ExtensionStackMeshoptRequired` per spec
  §"Fallback buffers"); descriptors with `mode` outside
  `{ATTRIBUTES, TRIANGLES, INDICES}`
  (`ExtensionStackMeshoptMode`) or `filter` outside
  `{NONE, OCTAHEDRAL, QUATERNION, EXPONENTIAL, COLOR}`
  (`ExtensionStackMeshoptFilter`); parent-layout mismatches where
  `byteStride * count != parent.byteLength`
  (`ExtensionStackMeshoptLayout`); per-mode byteStride
  invariants (ATTRIBUTES requires byteStride divisible by 4 in
  `[4, 256]`; TRIANGLES / INDICES require byteStride ∈ `{2, 4}` —
  `ExtensionStackMeshoptStride`); TRIANGLES count not divisible
  by 3 (`ExtensionStackMeshoptCount`); TRIANGLES / INDICES with
  any filter other than `"NONE"` (`ExtensionStackMeshoptFilter`);
  per-filter byteStride invariants (OCTAHEDRAL ∈ `{4, 8}`,
  QUATERNION == 8, EXPONENTIAL divisible by 4, COLOR ∈ `{4, 8}`);
  descriptor `buffer` out of range
  (`ExtensionStackMeshoptBuffer`); compressed range overrunning
  the source buffer (`ExtensionStackMeshoptRange`); a bufferView
  pointing at a fallback buffer WITHOUT carrying the extension
  (`ExtensionStackMeshoptFallbackRef`); and a descriptor's own
  `buffer` index pointing at a fallback buffer
  (`ExtensionStackMeshoptFallbackSource`)
- KHR_gaussian_splatting extension (Khronos Release Candidate) —
  the per-primitive descriptor block that flags a `POINTS` mesh
  primitive as a 3D Gaussian splat field per
  `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Extending
  Mesh Primitives". The descriptor carries `kernel` (required —
  the spec defines `"ellipse"`), `colorSpace` (required —
  `"srgb_rec709_display"` or `"lin_rec709_display"`), and the two
  optional fields `projection` (default `"perspective"`) and
  `sortingMethod` (default `"cameraDistance"`). The decoder surfaces
  the descriptor through `Primitive::extras["KHR_gaussian_splatting"]`
  as a JSON object so the typed `oxideav_mesh3d::Primitive` round-trips
  unchanged; the encoder lifts it back into the typed
  `PrimitiveExtensions` block on write and appends
  `KHR_gaussian_splatting` to `extensionsUsed` exactly once. The
  §3.12 stack validator rejects descriptor blocks without the
  `extensionsUsed` entry (`ExtensionStackUsedNotDeclared`),
  unknown `kernel` / `colorSpace` / `projection` / `sortingMethod`
  strings outside the spec-defined sets
  (`ExtensionStackGaussianSplattingKernel` /
  `…ColorSpace` / `…Projection` / `…SortingMethod`), and rejects
  a base-`"ellipse"` kernel on a non-POINTS primitive
  (`ExtensionStackGaussianSplattingMode`, per §"Ellipse Kernel"
  §"Dependencies on glTF"). The forward-compat carve-out lets a
  vendor-extension-prefixed identifier (`KHR_…`, `EXT_…`, vendor
  prefixes) layer additional kernels / color spaces / projections /
  sorting methods on top without re-touching this crate. The custom
  attribute semantics (`KHR_gaussian_splatting:ROTATION` / `:SCALE`
  / `:OPACITY` / `:SH_DEGREE_l_COEF_n` per §"Ellipse Kernel"
  §"Attributes") flow through the standard accessor pipeline as raw
  attributes. For the base `"ellipse"` kernel the validator now also
  enforces the full §"Ellipse Kernel" §"Attributes" storage contract:
  the five required semantics MUST all be present (`POSITION` +
  `:ROTATION` + `:SCALE` + `:OPACITY` + `:SH_DEGREE_0_COEF_0`,
  `ExtensionStackGaussianSplattingMissingAttribute`); each present
  splat attribute's accessor MUST carry the spec-mandated `type`
  (`ExtensionStackGaussianSplattingAttributeType` — ROTATION = VEC4,
  SCALE = VEC3, OPACITY = SCALAR, every SH coefficient = VEC3) and a
  spec-allowed component-type + normalized form
  (`ExtensionStackGaussianSplattingAttributeComponent` — ROTATION is
  float / signed-byte-normalized / signed-short-normalized; SCALE is
  float / unsigned-byte(-normalized) / unsigned-short(-normalized);
  OPACITY is float / unsigned-byte-normalized /
  unsigned-short-normalized; SH coefficients are float only); and the
  spherical-harmonics degrees MUST be complete per §"Spherical
  Harmonics Attributes" — for any used degree `l` in 1..=3 every
  `COEF_0..2l` of that degree AND all lower degrees MUST be defined
  (`ExtensionStackGaussianSplattingSHIncomplete`). A vendor-prefixed
  kernel defers this entire attribute contract to the kernel-defining
  extension and skips the checks. The spherical-harmonics colour
  evaluator from §"Lighting" / §"Fallback Behavior" ships in
  `splatting.rs`: `diffuse_color` applies the spec's
  `Color_diffuse = SH_{0,0} · 0.2820947917738781 + 0.5` degree-0
  reconstruction; `evaluate` computes the full view-dependent colour
  from up to 45 coefficients (degrees 0..=3 packed lowest-order to
  highest within each degree) and a normalised view direction via the
  exact §"Appendix A: Table of Constants" basis constants (the `0.5`
  bias applied once to the final sum, the Condon–Shortley `(-1)^m`
  sign folded into the per-lane multipliers, `degree` auto-capped to
  the coefficients actually supplied so a short slice can't index out
  of bounds); `color_0_fallback` derives the §"Fallback Behavior"
  `COLOR_0` RGBA a non-splat renderer paints onto the sparse point
  cloud (degree-0 diffuse clamped to `[0, 1]`, sRGB-decoded to linear
  when `colorSpace == "srgb_rec709_display"` because `COLOR_0` carries
  linear values per the glTF spec, splat opacity in alpha). The typed
  splat-field decode now lands: for an `"ellipse"`-kernel primitive the
  decoder reads the per-vertex `KHR_gaussian_splatting:ROTATION` (VEC4),
  `:SCALE` (VEC3), `:OPACITY` (SCALAR), and `:SH_DEGREE_l_COEF_n` (VEC3)
  accessors — applying the spec int→float dequantisation for the
  allowed normalized-integer storage forms (ROTATION signed
  byte/short-normalized; SCALE / OPACITY unsigned byte/short, raw or
  normalized; SH floats only) — and parks them as parallel typed arrays
  under `Primitive::extras["__gaussian_splats"]`
  (`{ count, rotation, scale, opacity, sh }`, the SH coefficients
  gathered in canonical `evaluate` order). `splatting::SplatField` is
  the typed view: `SplatField::from_extras(&prim.positions, sidecar)`
  reconstructs `Vec<Splat>`, each `Splat` exposing `position` /
  `rotation` / `scale` / `opacity` / `sh` plus `sh_degree()`,
  `diffuse()`, `color(dir)`, and `color_0_fallback(color_space)`
  delegating to the SH evaluator. A vendor-prefixed kernel defers the
  attribute contract to the kernel-defining extension and produces no
  `__gaussian_splats` sidecar
- KHR_draco_mesh_compression extension (Khronos ratified) — the
  per-primitive descriptor that redirects a mesh primitive's
  `attributes` + `indices` to a Draco-compressed `bufferView`
  payload, per
  `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md` §"glTF
  Schema Updates". The descriptor carries a `bufferView` indirection
  plus an `attributes` map pairing the parent primitive's attribute
  names (POSITION, NORMAL, …) with the Draco-side unique attribute
  IDs. This crate is a pass-through engine — the Draco bitstream
  inflate path is out of scope for this round — so the decoder
  surfaces the descriptor through
  `Primitive::extras["KHR_draco_mesh_compression"]` as a JSON object
  while reading the parent primitive's uncompressed-fallback
  accessors through the usual accessor pipeline (per §"accessors"
  the parent accessors describe the decompressed data). The encoder
  lifts the sidecar back into the typed `PrimitiveExtensions` block
  and appends `KHR_draco_mesh_compression` to `extensionsUsed` once
  per document. The §3.12 + §Conformance validators cover seven
  failure modes with stable `ExtensionStackDraco…` prefixes:
  descriptor present without the `extensionsUsed` entry
  (`ExtensionStackUsedNotDeclared`); descriptor `bufferView` out of
  range (`ExtensionStackDracoBufferView`); descriptor `bufferView`
  refers to a bufferView that defines `byteStride` — forbidden per
  glTF 2.0 §5.11.4 because the Draco payload is opaque compressed
  bytes, not vertex attribute data, and the extension does not
  enable a strided payload layout (`ExtensionStackDracoByteStride`,
  the same shape as the §5.3.1 sparse-indices `MUST NOT` rule);
  descriptor `attributes` key that is not present in the parent
  primitive's own attributes map (`ExtensionStackDracoAttributes`,
  per §"attributes" subset rule); duplicate Draco-side attribute IDs
  within one descriptor (`ExtensionStackDracoAttributeId`); primitive
  `mode` outside `{TRIANGLES (4), TRIANGLE_STRIP (5)}` per
  §"Restrictions on geometry type" (`ExtensionStackDracoMode`); and
  a compressed-only shape (parent primitive carries no uncompressed
  attributes alongside the descriptor) without
  `KHR_draco_mesh_compression` listed in `extensionsRequired` per
  §Conformance (`ExtensionStackDracoRequired`). The compressed-
  payload inflation remains a follow-up; the descriptor handshake is
  in place for any Draco-aware consumer layered above this crate
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
- Primitive topology vertex-count validation per spec §3.7.2.1 — the
  decoder rejects primitives whose number of vertex indices is invalid
  for the topology `mode`: POINTS MUST be non-zero, LINE_LOOP /
  LINE_STRIP MUST be ≥ 2, TRIANGLE_STRIP / TRIANGLE_FAN MUST be ≥ 3,
  LINES MUST be divisible by 2 and non-zero, TRIANGLES MUST be divisible
  by 3 and non-zero (`PrimitiveIndexCount`). The count is the `indices`
  accessor's `count` when `indices` is defined, otherwise the shared
  attribute accessors' `count`. The companion §3.7.2.1 rule is also
  enforced: when `indices` is defined every index value MUST be strictly
  less than the attribute accessors' `count` (`PrimitiveIndexBound`).
  Both checks are skipped for primitives carrying
  `KHR_draco_mesh_compression` (the rendered index stream lives inside
  the opaque compressed payload this pass-through engine does not
  inflate) or `KHR_gaussian_splatting` (a splat field, not a
  triangle/line/point list — the base ellipse kernel pins `mode` to
  POINTS through its own validator and a vendor kernel defers geometry
  semantics to the kernel-defining extension)
- Extension-stack consistency validation per spec §3.12 — the decoder
  rejects documents whose `extensionsRequired` set is not a subset of
  `extensionsUsed` (`ExtensionStackRequiredNotListed`), and documents
  that materialise a `KHR_lights_punctual` data block (root or
  per-node) or a `KHR_materials_unlit` data block (per material)
  without declaring the extension in `extensionsUsed`
  (`ExtensionStackUsedNotDeclared`)
- KHR_lights_punctual per-light property validation per
  `docs/3d/gltf/extensions/KHR_lights_punctual.md` §"Light Types" /
  §"Range Property" / §"Spot": each light's `type` must be one of
  `directional` / `point` / `spot` (`ExtensionStackLightType`); `color`
  components and `intensity` (≥ 0) must be finite
  (`ExtensionStackLightColorFinite` / `ExtensionStackLightIntensity`);
  `range` is point/spot-only and must be `> 0`
  (`ExtensionStackLightRange`); the `spot` property is required on spot
  lights and forbidden elsewhere (`ExtensionStackLightSpotRequired` /
  `ExtensionStackLightSpotMisplaced`); `innerConeAngle` ≥ 0,
  `outerConeAngle` ≤ `PI / 2`, and inner < outer
  (`ExtensionStackLightInnerCone` / `ExtensionStackLightOuterCone` /
  `ExtensionStackLightConeOrder`); and every node's
  `KHR_lights_punctual.light` index must reference a declared light
  (`ExtensionStackLightRef`)
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
- Sparse-values bufferView restrictions per spec §5.4.1 — an
  `accessor.sparse.values.bufferView` that carries `target` or
  `byteStride` is rejected with `SparseValuesBufferViewTarget` /
  `SparseValuesBufferViewStride`; out-of-range indices surface as
  `SparseValuesBufferViewIndex`. The §5.4 paragraph says the override
  elements are "tightly packed", so a strided layout would be a spec
  violation, and the bufferView is not a vertex-attribute /
  element-array buffer in the GPU-pipeline sense so a `target` hint
  is equally nonsensical — the symmetric companion to the §5.3.1
  rule above
- Camera property validation per spec §5.12 + §5.13 + §5.14 — every
  `cameras[i]` entry (referenced or not) is checked before
  conversion: the two projection blocks are mutually exclusive
  (`CameraProjectionExclusive`); orthographic `xmag` / `ymag` MUST
  NOT be zero (`CameraOrthographicXmag` / `…Ymag`), `zfar` MUST be
  `> 0` (`CameraOrthographicZfar`) and `> znear`
  (`CameraOrthographicZRange`), `znear` MUST be `>= 0`
  (`CameraOrthographicZnear`); perspective `yfov` / `znear` MUST be
  `> 0` (`CameraPerspectiveYfov` / `…Znear`), `aspectRatio` (when
  defined) MUST be `> 0` (`CameraPerspectiveAspectRatio`), `zfar`
  (when defined) MUST be `> 0` (`CameraPerspectiveZfar`) and
  `> znear` (`CameraPerspectiveZRange`). Non-finite values (NaN /
  ±∞) are rejected by the same rules; the spec's SHOULD-level advice
  (non-negative magnification, `yfov < π`) is deliberately NOT
  enforced, and an undefined perspective `zfar` (infinite
  projection) stays valid
- Node-hierarchy + node-transform validation per spec §3.5.2 + §3.5.3
  — the decoder rejects malformed node graphs and transforms with
  stable `Node…`-prefixed errors. §3.5.2 "the node hierarchy MUST be a
  set of disjoint strict trees": every `children[]` index MUST resolve
  into `nodes[]` (`NodeChildIndex`), a node MUST have zero or one parent
  (`NodeMultipleParents`), and the hierarchy MUST NOT contain cycles —
  including a node listing itself as a child (`NodeHierarchyCycle`).
  §3.5.3 transforms: `matrix` is mutually exclusive with the TRS
  properties (`NodeMatrixTRSExclusive`); a node targeted by an
  animation channel MUST use TRS only and MUST NOT carry `matrix`
  (`NodeAnimatedMatrix`); `rotation` MUST be a finite unit quaternion
  (`NodeRotationUnitQuaternion`, with a tolerance that absorbs
  normalized-integer round-trip error); `translation` / `scale` /
  `matrix` components MUST be finite (`NodeTranslationFinite` /
  `NodeScaleFinite` / `NodeMatrixFinite`); and a `matrix` MUST be
  decomposable to TRS, so a zero / non-finite upper-left 3×3
  determinant is rejected (`NodeMatrixDecompose`). The conservative
  determinant test leaves the shear/skew sub-case (an Implementation
  Note, not a MUST) accepted when the matrix is still invertible
- Skin-roster validation per spec §5.28 + §3.7.3 + §5.25.3 — the
  `validate_skins` pass (run after `validate_nodes`) policies the skin
  MUSTs the decoder previously parsed but never enforced. §5.28.3:
  `skin.joints` MUST be non-empty, every joint MUST be a valid node
  index, and each joint MUST be unique (`SkinJointsEmpty` /
  `SkinJointIndex` / `SkinJointDuplicate`). §5.28.2: `skin.skeleton`,
  when present, MUST be a valid node index (`SkinSkeletonIndex`).
  §5.28.1 / §3.7.3.1: the `inverseBindMatrices` accessor, when present,
  MUST be a valid accessor of `"MAT4"` type with `FLOAT` components, MUST
  NOT be `normalized`, and its `count` MUST be ≥ the joint count
  (`SkinIbmIndex` / `SkinIbmAccessorType` / `SkinIbmAccessorComponentType`
  / `SkinIbmAccessorNormalized` / `SkinIbmCount`). §5.25.3: a node with
  `skin` MUST reference a valid skin AND MUST also define `mesh`
  (`NodeSkinIndex` / `NodeSkinWithoutMesh`). §3.7.3.2: a skin referenced
  by a node within a scene MUST have all of its joints in that same scene
  (`SkinJointWrongScene`). The §3.7.3.2 *common-root* SHOULD is not
  enforced as a document-node-ancestry MUST — joints that are distinct
  scene roots are accepted (the scene is their implicit common root,
  which the spec explicitly permits to be a node that "may or may not be
  a joint node itself", and which this crate's encoder emits)
- Texture / material reference validation per spec §5.29 + §5.30 + §5.22
  — the `validate_textures` pass (run after `validate_skins`) policies
  the index-resolution MUSTs the decoder parsed but never enforced. The
  field types already pin the `>= 0` minimum; the missing rule is the
  upper bound: `texture.source` MUST resolve into `images[]`
  (`TextureSourceIndex`, §5.29.1); `texture.sampler` MUST resolve into
  `samplers[]` (`TextureSamplerIndex`, §5.29.2); and every core material
  `textureInfo.index` — across `pbrMetallicRoughness.baseColorTexture` /
  `metallicRoughnessTexture`, `normalTexture`, `occlusionTexture`,
  `emissiveTexture` — MUST resolve into `textures[]`
  (`MaterialTextureIndex`, §5.30.1, naming the offending slot). The
  `KHR_texture_basisu` per-texture `source` indirection keeps its own
  in-range check in `validate_extension_stack`
- Texture-sampler filter / wrap validation per spec §5.26 — every
  `samplers[i]` entry is checked before conversion against the closed
  enum sets in §5.26.1–§5.26.4: `magFilter`, when present, MUST be
  `9728` NEAREST or `9729` LINEAR (`SamplerMagFilter`); `minFilter`
  MUST be one of the six filter/mipmap combinations `9728` / `9729` /
  `9984` / `9985` / `9986` / `9987` (`SamplerMinFilter`); `wrapS`
  (`SamplerWrapS`) and `wrapT` (`SamplerWrapT`) MUST be `33071`
  CLAMP_TO_EDGE, `33648` MIRRORED_REPEAT, or `10497` REPEAT. Absent
  properties stay valid (wrapS/wrapT default to REPEAT; filters are
  implementation choice) — only an out-of-set integer is rejected
- Core accessor property validation per spec §3.6.2 + §5.1 — a
  `validate_accessors` pass checks every `accessors[i]` entry (referenced
  or not): `count` MUST be `>= 1` (`AccessorCount`, §5.1 schema minimum);
  `normalized` MUST NOT be `true` for FLOAT (5126) or UNSIGNED_INT (5125)
  componentType (`AccessorNormalizedComponentType`, §5.1.6 / §3.6.2.1 —
  no integer→[0,1]/[-1,1] decode is defined for those); and `min` / `max`
  array length MUST equal the accessor's component count (1/2/3/4/9/16 per
  `type`) when present (`AccessorMinMaxLength`, §3.6.2.5). An unknown
  `type` string defers to the bufferView-fit pass's element-type rejection
- Top-level index-reference resolution per spec §3.3 + §5.27.1 + §5.25.5
  + §5.25.1 + §5.24.3 — `validate_index_references` rejects every dangling
  top-level index edge the field types only bounded from below: the
  default `scene` index (`DefaultSceneIndex`), a `scene.nodes[]` entry
  (`SceneNodeIndex`), `node.mesh` (`NodeMeshIndex`), `node.camera`
  (`NodeCameraIndex`), and `primitive.material` (`PrimitiveMaterialIndex`)
  MUST each resolve into their referenced root array (node.skin /
  node.children / textureInfo / animation-target references keep their own
  dedicated passes)
- Schema structural minimums per spec §5.10.2 + §5.11.3 + §5.2.1 +
  §3.6.2.3 — `validate_structural_minimums` enforces the bounds that hold
  on the declared integers alone: `buffer.byteLength` ≥ 1
  (`BufferByteLength`), `bufferView.byteLength` ≥ 1
  (`BufferViewByteLength`), `accessor.sparse.count` ≥ 1 (`SparseCountMin`)
  and ≤ the base accessor element count (`SparseCountRange`)
- Animation-sampler structural validation per spec §3.11 + Appendix C —
  `validate_animation_channels` now also enforces the sampler-accessor
  MUSTs: the `input` accessor MUST define both `min` and `max`
  (`AnimationSamplerInputBounds`); `interpolation` MUST be `LINEAR` /
  `STEP` / `CUBICSPLINE` (`AnimationSamplerInterpolation`); the `output`
  element count MUST equal `keyframes × per-keyframe` for LINEAR / STEP
  and `3 × keyframes × per-keyframe` for CUBICSPLINE
  (`AnimationSamplerOutputCount`), where `per-keyframe` is 1 for TRS /
  pointer channels and the morph-target count for `weights` channels
  (§3.11: the morph-weight output accessor's "final size is equal to the
  number of morph targets times the number of animation frames"); and a
  CUBICSPLINE sampler MUST have ≥ 2 keyframes
  (`AnimationSamplerCubicKeyframes`, §C.5)
- Image-source validation per spec §5.18 — `validate_images` policies
  every `images[i]` (referenced or not): exactly one source MUST be
  defined, `uri` XOR `bufferView` (`ImageNoSource` /
  `ImageSourceExclusive`); a `bufferView`-backed image MUST carry a
  `mimeType` (`ImageMimeTypeRequired`) and its `bufferView` index MUST
  resolve (`ImageBufferViewIndex`)
- Mesh morph-weights length validation per spec §5.23.2 —
  `validate_morph_weights` enforces that a `mesh.weights` array's length
  matches the mesh's morph-target count (`MeshWeightsLength`)
- Morph-target structural validation per spec §3.7.2.2 —
  `validate_morph_targets` enforces the morph MUSTs that hold on the
  declared accessors alone: all primitives in a mesh MUST declare the
  same number of targets (`MorphTargetPrimitiveCount`); every morphed
  attribute MUST have a base attribute of the same name in the primitive
  (`MorphTargetMissingBase`) and a morph accessor whose `count` equals
  the base attribute accessor's `count` (`MorphTargetCount`); each
  morphed semantic MUST follow the §3.7.2.2 type/componentType table —
  POSITION / NORMAL / TANGENT are VEC3 float (the TANGENT handedness W
  is omitted on displacements), TEXCOORD_n is VEC2, COLOR_n is VEC3 or
  VEC4, with the float and four normalized-integer storage forms allowed
  for TEXCOORD / COLOR (`MorphTargetAttributeType` /
  `MorphTargetAttributeComponent`). When `KHR_mesh_quantization` is
  declared the §"Extending Morph Target Attributes" extra forms are also
  accepted (POSITION VEC3 byte/short raw-or-normalized; NORMAL / TANGENT
  VEC3 byte/short normalized; TEXCOORD_n VEC2 byte/short raw). A morphed
  POSITION accessor MUST define `min` / `max`
  (`MorphTargetPositionBounds`). Out-of-range morph
  accessor indices surface as `MorphTargetAccessorIndex`;
  application-specific semantics (names prefixed with `_`) defer their
  type contract to the application and are checked only for
  base-attribute presence + count
- `extras` round-trip on root, scenes, nodes, materials, primitives

## Extension roadmap (next-round work)

The KHR extension registry is now staged under
`docs/3d/gltf/extensions/` (25 specs + index), so the remaining work
is implementation, not docs:

- KHR_animation_pointer `int` Object-Model branch + core property
  table — the pointer-template registry
  (`object_model.rs`) and the `bool` branch (componentType MUST be
  UNSIGNED_BYTE, `0` → false, else true, STEP-only samplers) are in
  place, seeded
  from the staged extension specs' §"Extending glTF 2.0 Asset Object
  Model" tables. The `int` branch (componentType MUST be a
  non-normalised integer, values used as-is, STEP-only) is wired the
  same way but has zero registry rows to dispatch on — **blocked on
  DOCS-GAP**: the core spec's Object Model table (`ObjectModel.adoc`)
  is not staged under `docs/3d/gltf/`, and no staged extension
  declares an `int`-typed mutable property. Staging `ObjectModel.adoc`
  would also unlock spec-aware pointer-resolution validation
  (accessor-type vs data-type compatibility per the §Operation table
  for core properties)
- KHR_texture_basisu transcode lane — the per-texture
  indirection round-trip (sidecar + §3.12 validation) is in place as
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
