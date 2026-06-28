# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Skinned-mesh JOINTS_n / WEIGHTS_n attribute validation (spec
  §3.7.3.3)** — a new `validate_skinning_attributes` pass enforces both
  attributes are VEC4, JOINTS_n componentType is unsigned byte / short,
  and WEIGHTS_n componentType is float or normalized unsigned byte / short
  (`SkinningAttributeType` / `SkinningJointsComponentType` /
  `SkinningWeightsComponentType`); the materialised weights are then
  checked non-negative (`SkinningWeightsNegative`). The WEIGHTS_0 decode
  path additionally dequantises the normalized-integer storage forms
  (previously FLOAT-only), so spec-valid normalized weights round-trip.
- **Indexed attribute set-index validation (spec §3.7.2.1)** — a new
  `validate_attribute_set_indices` pass enforces that the four indexed
  vertex semantics (`TEXCOORD_n` / `COLOR_n` / `JOINTS_n` / `WEIGHTS_n`)
  use set indices that start at 0, are consecutive, and carry no leading
  zeroes — on both the primitive attributes and each morph target.
  A gap, a non-zero start, or a malformed suffix is rejected with
  `AttributeSetIndex`, closing the silent-drop hole in the decoder's
  count-up TEXCOORD_n loop.
- **Core material factor / scalar range validation (spec §5.19–§5.22)** —
  a new `validate_materials` pass enforces the JSON-schema closed-range
  MUSTs the typed `f32` model does not: `baseColorFactor` /
  `metallicFactor` / `roughnessFactor` / `emissiveFactor` each in `[0, 1]`,
  `alphaCutoff` ≥ 0, `occlusionTexture.strength` in `[0, 1]`, and
  `normalTexture.scale` finite (`Material…Range` / `MaterialNormalScaleFinite`).
  Non-finite values are rejected by the same windows.
- **Inverse-bind-matrix fourth-row validation (spec §3.7.3.1)** — the
  decoder now enforces that the fourth row of every materialised
  inverse-bind matrix is `[0, 0, 0, 1]` (an IBM is an affine joint
  transform with no projective component). A deviating bottom row is
  rejected with `SkinIbmBottomRow`, decided on the decoded
  `[[f32; 4]; 4]` matrices with a small f32-round-trip tolerance.
- **Material-extension textureInfo `index` resolution (spec §5.30.1)** —
  the `validate_textures` pass now policies every textureInfo nested
  inside a KHR material extension (specular / clearcoat / sheen /
  transmission / volume / iridescence / anisotropy / diffuse-transmission),
  rejecting a `.index` that points past `textures[]` with
  `MaterialExtensionTextureIndex`. Previously only the five core PBR
  texture slots were range-checked. The roster is collected by a new
  `material_extension_texture_indices` walk paralleling the existing
  `material_texture_transforms` enumeration.
- **Per-accessor component-size alignment validation (spec §3.6.2.4 line
  3091)** — the accessor-fit pass now enforces, on every accessor with a
  bufferView, that `accessor.byteOffset` and
  `accessor.byteOffset + bufferView.byteOffset` are multiples of the
  component size (`AccessorByteOffsetAlignment`), and that a defined
  `bufferView.byteStride` is a multiple of the component size
  (`AccessorStrideAlignment`). Previously only the per-primitive
  vertex-attribute pass checked alignment (with the stricter 4-byte rule),
  leaving non-vertex accessors — animation sampler input/output, indices,
  inverseBindMatrices, sparse — unpoliced.
- **Accessor `min`/`max` consistency generalised to all arities (spec
  §3.6.2.1.5)** — the component-wise extrema check that previously fired
  only on VEC3 attributes is now arity-generic (`validate_vecn_bounds`):
  the SCALAR animation-input accessor (keyframe times), VEC2 (TEXCOORD_n),
  and VEC4 (TANGENT / COLOR_n) attribute accessors that declare `min`/`max`
  are now policed too, rejecting a declared/actual mismatch with
  `AccessorBoundsMismatch`.
- **Animation-sampler keyframe-time ordering validation per spec §3.11** —
  the decoder now policies the input-accessor timestamp MUSTs that hold on
  the *materialised* `&[f32]` values (not derivable from JSON metadata):
  the first keyframe time MUST be `>= 0.0`
  (`AnimationSamplerInputTimeStart`, spec "time[0] >= 0.0") and the
  sequence MUST be strictly increasing — `time[n + 1] > time[n]`
  (`AnimationSamplerInputTimeOrder`). Equal consecutive times, decreasing
  times, and non-finite (Infinity/NaN) keyframe times are all rejected
  (`partial_cmp` returns the unordered case for NaN). Both the base
  sampler decode path and the `KHR_animation_pointer` sampler decode path
  run the check.

- `KHR_meshopt_compression` **ATTRIBUTES v1 (`0xa1`) encoder** — the
  production ATTRIBUTES write path now emits the v1 format Appendix C
  recommends ("v1 format should be preferred since it provides better
  compression ratio at no additional runtime cost"). Per block, each byte
  position gets the cheapest 2-bit control mode by encoded size:
  **mode 2** (all deltas zero) stores **no data** — the dominant win for
  quantised attributes whose high bytes never change; **mode 0**
  (`{0, 1, 2, 4}` ladder) for small deltas; **mode 1** (`{1, 2, 4, 8}`
  ladder) for larger deltas; and **mode 3** (literal, no group headers)
  for high-entropy byte positions where header overhead would lose. Each
  16-element group independently picks the narrowest width on its mode's
  ladder. Channel modes stay byte-delta (0). The decode reconstructs the
  input exactly. The prior v0 (`0xa0`) encoder is retained for
  compatibility coverage. Covered by a control-mode-2 effectiveness test
  (v1 beats v0 on constant byte positions), a high-entropy round-trip
  that exercises modes 1/3, and the existing round-trip / fuzz suites
  driven through the new path.

- `KHR_meshopt_compression` **INDICES two-baseline encoder** — the Mode 2
  (INDICES) write path now uses the decoder's dual-baseline scheme,
  greedily delta-coding each index against whichever of the two baselines
  yields the smaller zigzag magnitude (hence the shorter varint). Index
  data drawn from two interleaved runs — e.g. dual-stream geometry —
  compresses to roughly one byte per index instead of paying a multi-byte
  varint for every cross-run jump, while staying exactly invertible. The
  prior encoder delta-coded against baseline 0 only. Covered by a
  two-interleaved-run round-trip + compactness assertion.

- `KHR_meshopt_compression` **TRIANGLES edge-reuse encoder** — the Mode 1
  (TRIANGLES) write path now mirrors the decoder's full state machine
  (edge FIFO, vertex FIFO, `next` new-vertex counter, `last` explicit
  baseline) and emits the compact `0xXY` edge-reuse codes whenever a
  triangle's leading edge is already in the edge FIFO, encoding the third
  corner as the running `next` counter, a vertex-FIFO reference, a
  `last ± 1` step, or an explicit delta — falling back to the
  order-preserving `0xff` explicit code only when no edge can be reused.
  This replaces the previous all-explicit encoding, so locality-optimised
  triangle lists compress substantially (an 8×8-quad grid encodes well
  below its raw index size with edge reuse on ≥ half the triangles).
  Index order is preserved byte-for-byte (only the leading edge is
  reused, never a cyclic rotation). Covered by grid / fan / disjoint
  round-trips plus a fixed-seed fuzz test over 200 random triangle lists.

- `KHR_meshopt_compression` **forward (encode-side) Appendix B filters** —
  `meshopt::encode` now accepts the four post-decompression filters
  (OCTAHEDRAL / QUATERNION / EXPONENTIAL / COLOR) in addition to NONE,
  with ATTRIBUTES mode. Each filter's forward transform produces the
  filtered integer representation that the inverse filter decodes back,
  so a full `encode(.., filter, ..)` → `decode(.., filter, ..)`
  reconstructs the caller's high-level data within the tolerance the
  spec states (`docs/3d/gltf/extensions/KHR_meshopt_compression.md`,
  "Appendix B"): **exact** for EXPONENTIAL (an f32 is decomposed into the
  signed 8-bit exponent + signed 24-bit mantissa so `2^e * m` reproduces
  the value bit-for-bit; values needing a full 24-bit *odd* mantissa, or
  an exponent outside the spec's `[-100, 100]` window, are the only
  finite-f32 carve-out and are reported rather than truncated), and
  **within one unit in last place** for OCTAHEDRAL (octahedral projection
  of unit normals/tangents with the pass-through 4th component preserved),
  QUATERNION (largest-component max encoding with double-cover sign
  alignment), and COLOR (YCoCg with a ±1 neighbourhood search that keeps
  every reconstructed component in range under the unclamped decode).
  Filters remain rejected for TRIANGLES / INDICES per §"Specifying
  compressed views". Covered by `tests/meshopt_encode_filters.rs`.

- Compression-effectiveness unit tests for `meshopt::encode` — a smooth
  256-element VEC3 f32 attribute ramp and a 300-element sequential index
  list both compress below their raw size (and still round-trip), so the
  encoder is verified to actually shrink data, not merely invert.

- Deterministic property/fuzz coverage for the meshopt write path
  (`tests/meshopt_encode_fuzz.rs`) — a fixed-seed LCG generates ~96
  random indexed triangle meshes (varied vertex/triangle counts,
  optional normals, large multi-block primitives) and asserts each
  reproduces the plain-encode round-trip's positions / normals /
  indices after meshopt compression, across both GLB and JSON flavours.

- `KHR_meshopt_compression` **write path** —
  `GltfEncoder::with_meshopt_compression(true)` post-compresses eligible
  bufferViews: index views (`ELEMENT_ARRAY_BUFFER`, SCALAR `u16`/`u32`)
  with the INDICES codec, and dense vertex-attribute views
  (`ARRAY_BUFFER`, single accessor, element stride a positive multiple
  of 4 in `[4, 256]`) with the ATTRIBUTES v0 codec. The packed BIN keeps
  the uncompressed bytes (a plain real-data buffer, NOT a fallback
  marker — it also backs the vertex views, and §"Fallback buffers"
  reserves the marker for no-data placeholders whose every reference
  carries a descriptor), a second `data:`-URI buffer carries the
  compressed payloads, each compressed bufferView gains its mode
  descriptor, and `KHR_meshopt_compression` is declared in
  `extensionsUsed` only (the document stays readable without the
  extension, so it is not `extensionsRequired`). `u8` index views and
  sparse / interleaved views are left uncompressed. Opt-in, off by
  default; works with both GLB and JSON-embedded flavours. Documents
  round-trip back through this crate's decoder to the original
  attribute + index data. Six new integration tests
  (`tests/meshopt_encode_compression.rs`), including a multi-attribute
  (POSITION + NORMAL + TEXCOORD + indices) round-trip.

- `KHR_meshopt_compression` **encoder** (`meshopt::encode`) — the
  inverse of the Appendix A bitstream decoder, producing payloads that
  round-trip byte-for-byte through `meshopt::decode`. All three modes:
  ATTRIBUTES (mode 0) emits the v0 stream (`0xa0`, the wire shape
  `EXT_meshopt_compression` shares) with per-byte-position group
  bit-width selection over the v0 widths `{0, 2, 4, 8}` and sentinel
  escapes; INDICES (mode 2) emits the two-baseline varint zigzag delta
  stream (baseline 0 only — a valid general encoding); TRIANGLES
  (mode 1) emits an all-explicit `0xff`/`zw=0xff` per-triangle stream
  the FIFO decoder accepts. Restricted to the `NONE` filter (the four
  Appendix B filters are author-side quantising transforms, not a
  lossless raw-byte path). Fourteen new round-trip / inverse unit tests
  (zigzag, varint, per-mode round-trips across u16/u32 strides,
  multi-block multi-group attributes, all-zero + zero-count edges).

- Morph-target structural validation (`validate_morph_targets`) per
  glTF 2.0 §3.7.2.2 — the morph MUSTs that hold on declared accessors
  alone: all primitives in a mesh declare the same number of targets
  (`MorphTargetPrimitiveCount`); every morphed attribute has a base
  attribute of the same name (`MorphTargetMissingBase`) and a morph
  accessor count matching the base accessor count (`MorphTargetCount`);
  morphed semantics follow the §3.7.2.2 accessor type/componentType
  table — POSITION/NORMAL/TANGENT VEC3 float, TEXCOORD_n VEC2, COLOR_n
  VEC3/VEC4, float or the four normalized-integer forms for
  TEXCOORD/COLOR (`MorphTargetAttributeType` /
  `MorphTargetAttributeComponent`); a morphed POSITION accessor defines
  `min`/`max` (`MorphTargetPositionBounds`); out-of-range morph accessor
  indices (`MorphTargetAccessorIndex`). Application-specific semantics
  (`_`-prefixed) defer their type contract. Ten unit tests; the
  `morph_targets.rs` integration fixtures gained the spec-required
  morphed-POSITION `min`/`max` and a base NORMAL attribute.

- Animation-channel target-uniqueness validation per glTF 2.0 §3.11 —
  "within one animation, each target (a combination of a node and a
  path) MUST NOT be used more than once." `validate_animation_channels`
  now rejects two channels driving the same `(node, path)` pair with
  `AnimationChannelTargetDuplicate`. The `"pointer"` path
  (KHR_animation_pointer) is excluded — it carries no node and its
  uniqueness is keyed on the pointer string, already policed by
  `ExtensionStackAnimationPointerDuplicate`. Two tests.

- bufferView.target enum validation per glTF 2.0 §5.11.5 — a
  `bufferViews[i].target`, when present, MUST be one of the two closed
  GPU-binding-hint enums `34962` ARRAY_BUFFER or `34963`
  ELEMENT_ARRAY_BUFFER; an out-of-set integer is rejected with
  `BufferViewTargetEnum` inside `validate_structural_minimums`. An absent
  `target` stays valid (it is a SHOULD-level hint per §5.11). Two tests.

- GLB container robustness per glTF 2.0 §4.4.2 + §3.6.1.2 — the binary
  reader now enforces two length MUSTs it previously accepted silently.
  §4.4.2: the header `length` field is "the total length of the Binary
  glTF, including header and all chunks"; a file whose byte count exceeds
  the declared `length` carries trailing data the header does not account
  for and is rejected (`GlbHeaderLength`), as is a `length` below the
  12-byte header. §3.6.1.2: the GLB-stored `buffer[0]` (uri-less,
  referencing the BIN chunk) may have a BIN chunk up to 3 bytes larger
  than its JSON-declared `byteLength` (the spec's padding allowance so a
  writer need not re-update the length after 4-byte chunk padding); a
  surplus of 4 or more bytes is a real length mismatch, not padding, and
  is rejected (`GlbBufferLength`). Seven new tests (four `.glb` integration
  cases in `glb_bin_length.rs`, three unit tests in `glb.rs`).

- Image-source validation (`validate_images`) per glTF 2.0 §5.18 — every
  `images[i]` (referenced or not) MUST define exactly one source:
  `uri` XOR `bufferView` (`ImageNoSource` / `ImageSourceExclusive`,
  §5.18.1); a `bufferView`-backed image MUST carry a `mimeType`
  (`ImageMimeTypeRequired`, §5.18.2) and its `bufferView` index MUST
  resolve (`ImageBufferViewIndex`, §5.18.3). Previously only images
  reached by a texture were checked, and only for the "neither source"
  case.
- Mesh-morph-weights length validation (`validate_morph_weights`) per
  glTF 2.0 §5.23.2 — `mesh.weights` array length MUST match the mesh's
  morph-target count (`MeshWeightsLength`). (`node.weights` carries the
  same §5.25.9 rule but is not retained in the parsed model, so only the
  modelled `mesh.weights` is policed.)

- Animation-sampler structural validation per glTF 2.0 §3.11 + Appendix C
  — `validate_animation_channels` now enforces the sampler MUSTs the
  decoder previously parsed but never policed: the `input` accessor MUST
  define both `min` and `max` (`AnimationSamplerInputBounds`); the
  `interpolation` value MUST be one of `LINEAR` / `STEP` / `CUBICSPLINE`
  (`AnimationSamplerInterpolation`); the `output` element count MUST equal
  `keyframes * per-keyframe-elements` for LINEAR / STEP and
  `3 * keyframes * per-keyframe-elements` for CUBICSPLINE
  (`AnimationSamplerOutputCount`), where the per-keyframe element count is
  1 for translation / rotation / scale / pointer channels and the
  morph-target count for `weights` channels (§3.11 "the output accessor …
  final size is equal to the number of morph targets times the number of
  animation frames"); and a CUBICSPLINE sampler MUST have at least 2
  keyframes (`AnimationSamplerCubicKeyframes`, §C.5). Six new unit tests
  plus fixture corrections in `khr_animation_pointer`, `normalized_animation`,
  and `node_hierarchy_validation` to keep their hand-crafted samplers
  spec-valid.
- Top-level index-reference resolution validation (`validate_index_references`)
  per glTF 2.0 §3.3 + §5.27.1 + §5.25.5 + §5.25.1 + §5.24.3 — the decoder
  now rejects documents whose top-level index edges dangle: the default
  `scene` index out of range (`DefaultSceneIndex`), a `scene.nodes[]`
  entry out of range (`SceneNodeIndex`), `node.mesh` out of range
  (`NodeMeshIndex`), `node.camera` out of range (`NodeCameraIndex`), and
  `primitive.material` out of range (`PrimitiveMaterialIndex`). The field
  types already pinned the non-negative minimum; this pass adds the
  upper-bound MUST. (`node.skin` / `node.children` / textureInfo /
  animation-target references continue to be policed by their dedicated
  passes.)
- Structural-minimum validation (`validate_structural_minimums`) per
  glTF 2.0 §5.10.2 + §5.11.3 + §5.2.1 + §3.6.2.3 — `buffer.byteLength`
  and `bufferView.byteLength` MUST be `>= 1` (`BufferByteLength` /
  `BufferViewByteLength`, schema "Minimum: >= 1"); `accessor.sparse.count`
  MUST be `>= 1` (`SparseCountMin`) and MUST NOT exceed the base accessor
  element `count` (`SparseCountRange` — "This number MUST NOT be greater
  than the number of the base accessor elements"). These hold on the
  declared integers alone, so a never-materialised accessor / buffer now
  fails fast rather than slipping through.
- KHR_texture_transform on material-extension textureInfos — per
  `docs/3d/gltf/extensions/KHR_texture_transform.md` §glTF Schema
  Updates the transform "may be defined on `textureInfo` structures"
  (any textureInfo). The crate previously handled only the five core
  PBR slots; the transform now rides every textureInfo nested inside a
  material extension (`KHR_materials_specular` / `_clearcoat` / `_sheen`
  / `_transmission` / `_volume` / `_iridescence` / `_anisotropy` /
  `_diffuse_transmission`). A single exhaustive walk
  (`validation::material_texture_transforms`) is shared between the
  decode-side §3.12 validator and the encoder's `extensionsUsed`
  declaration scan, so (a) a document carrying a nested transform now
  declares `KHR_texture_transform` on encode (previously it emitted the
  nested block but left the declaration off, producing a §3.12-invalid
  document), and (b) the decoder now rejects a nested transform whose
  extension is undeclared (`ExtensionStackUsedNotDeclared` — previously
  a false negative). The validator additionally enforces the §Overview
  affine-transform finiteness MUSTs on every transform (core or
  extension slot): a non-finite `rotation`
  (`ExtensionStackTextureTransformRotationFinite`), `offset` component
  (`ExtensionStackTextureTransformOffsetFinite`), or `scale` component
  (`ExtensionStackTextureTransformScaleFinite`) is rejected.

- Texture / material reference validation per spec §5.29 (Texture) +
  §5.30 (Texture Info) + §5.22 (Material PBR Metallic Roughness) — the
  new `validate_textures` pass (run after `validate_skins`) policies the
  index-resolution MUSTs the decoder parsed but never enforced. Field
  types already pin the `>= 0` minimum; the missing rule is the upper
  bound: `texture.source` MUST resolve into `images[]`
  (`TextureSourceIndex`, §5.29.1); `texture.sampler` MUST resolve into
  `samplers[]` (`TextureSamplerIndex`, §5.29.2); and every core material
  `textureInfo.index` — across `pbrMetallicRoughness.baseColorTexture` /
  `metallicRoughnessTexture`, `normalTexture`, `occlusionTexture`,
  `emissiveTexture` — MUST resolve into `textures[]`
  (`MaterialTextureIndex`, §5.30.1, with the offending slot named in the
  diagnostic). The `KHR_texture_basisu` per-texture `source` indirection
  keeps its own in-range check in `validate_extension_stack`.
- Skin-roster validation per spec §5.28 (Skin) + §3.7.3 (Skins) +
  §5.25.3 (node.skin) — the new `validate_skins` pass, wired into
  `convert()` after `validate_nodes`, enforces the document-level MUSTs
  the decoder previously parsed but never policed:
  - `skin.joints` MUST be non-empty (`integer [1-*]`), every joint index
    MUST be a valid node index, and each joint index MUST be unique
    (`SkinJointsEmpty` / `SkinJointIndex` / `SkinJointDuplicate`,
    §5.28.3).
  - `skin.skeleton`, when present, MUST be a valid node index
    (`SkinSkeletonIndex`, §5.28.2).
  - the `skin.inverseBindMatrices` accessor, when present, MUST be a
    valid accessor index of `"MAT4"` type with floating-point (`FLOAT`)
    components, MUST NOT be `normalized`, and its `count` MUST be ≥ the
    number of joints (`SkinIbmIndex` / `SkinIbmAccessorType` /
    `SkinIbmAccessorComponentType` / `SkinIbmAccessorNormalized` /
    `SkinIbmCount`, §5.28.1 / §3.7.3.1).
  - a node defining `skin` MUST reference a valid skin index AND MUST
    also define `mesh` (`NodeSkinIndex` / `NodeSkinWithoutMesh`,
    §5.25.3).
  - when a skin is referenced by a node within a scene, all of the
    skin's joints MUST belong to that same scene (`SkinJointWrongScene`,
    §3.7.3.2). Joints that are distinct root nodes of one scene are
    accepted — the scene is their implicit common root, which the spec
    explicitly allows to be a node that "may or may not be a joint node
    itself", and which this crate's own encoder emits; no document-node
    common ancestor is required.
- Primitive topology vertex-count validation per spec §3.7.2.1 — the
  decoder now rejects primitives whose number of vertex indices is
  invalid for the topology `mode`: POINTS MUST be non-zero, LINE_LOOP /
  LINE_STRIP MUST be ≥ 2, TRIANGLE_STRIP / TRIANGLE_FAN MUST be ≥ 3,
  LINES MUST be divisible by 2 and non-zero, TRIANGLES MUST be divisible
  by 3 and non-zero (`PrimitiveIndexCount`). The count is the `indices`
  accessor's `count` when `indices` is defined, otherwise the shared
  attribute accessors' `count`. The decoder also enforces the companion
  §3.7.2.1 rule that, when `indices` is defined, every index value MUST
  be strictly less than the attribute accessors' `count`
  (`PrimitiveIndexBound`). Both checks are skipped for primitives
  carrying `KHR_draco_mesh_compression` (the rendered index stream lives
  inside the opaque compressed payload) or `KHR_gaussian_splatting` (the
  primitive is a splat field, not a triangle/line/point list — the base
  ellipse kernel pins `mode` to POINTS via its own validator and a
  vendor kernel defers geometry semantics to the kernel-defining
  extension)
- KHR_gaussian_splatting typed splat-field decode — for an
  `"ellipse"`-kernel `POINTS` primitive the decoder now reads the
  per-vertex `KHR_gaussian_splatting:ROTATION` (VEC4), `:SCALE` (VEC3),
  `:OPACITY` (SCALAR), and `:SH_DEGREE_l_COEF_n` (VEC3) accessors,
  applying the spec int→float dequantisation for the allowed
  normalized-integer storage forms (§"Ellipse Kernel" §"Attributes"),
  and parks them as parallel typed arrays under
  `Primitive::extras["__gaussian_splats"]`
  (`{ count, rotation, scale, opacity, sh }`, SH coefficients in
  canonical `evaluate` order). New `splatting::{Splat, SplatField}`
  typed view: `SplatField::from_extras(&positions, sidecar)`
  reconstructs `Vec<Splat>` with `position` / `rotation` / `scale` /
  `opacity` / `sh` fields plus `sh_degree()`, `diffuse()`,
  `color(dir)`, and `color_0_fallback(color_space)` delegating to the
  SH evaluator. New `quantization::dequantize_scalar` helper handles
  the normalized-integer OPACITY path. A vendor-prefixed kernel defers
  the attribute contract and produces no `__gaussian_splats` sidecar
  (round 329)
- KHR_gaussian_splatting spherical-harmonics colour evaluator
  (`splatting.rs`) — `diffuse_color` (degree-0 reconstruction
  `SH_{0,0} · 0.2820947917738781 + 0.5`), `evaluate` (full
  view-dependent colour from up to 45 coefficients, degrees 0..=3,
  using the exact §"Appendix A: Table of Constants" basis constants
  with the Condon–Shortley `(-1)^m` phase and the `0.5` bias), and
  `color_0_fallback` (the §"Fallback Behavior" `COLOR_0` RGBA derived
  from the degree-0 diffuse colour, clamped to `[0, 1]`, sRGB-decoded
  to linear for `srgb_rec709_display`, opacity in alpha) (round 324)
- KHR_meshopt_compression bitstream decoder (Appendix A + B) — full
  inflate of compressed bufferViews (ATTRIBUTES v0/v1, TRIANGLES,
  INDICES + OCTAHEDRAL/QUATERNION/EXPONENTIAL/COLOR filters), wired
  into the buffer-materialisation path so meshopt documents decode end
  to end (round 316)

### Changed

- The inverse-bind-matrix fourth-row check (`SkinIbmBottomRow`, §3.7.3.1)
  exempts an all-zero matrix — the sparse zero-base sentinel the
  sparse-IBM encoder emits for non-overridden joint slots — so a
  sparse-encoded IBM accessor still round-trips.

## [0.0.3](https://github.com/OxideAV/oxideav-gltf/compare/v0.0.2...v0.0.3) - 2026-06-15

### Added

- KHR_node_visibility extension (round 199)

### Other

- relocate validate_accessors below validate_cameras so doc comments attribute correctly
- core accessor property validation (§3.6.2 + §5.1) — count >= 1, normalized componentType, min/max length
- texture-sampler filter/wrap validation per spec §5.26
- node hierarchy + transform rules per spec §3.5.2 / §3.5.3
- KHR_texture_basisu target-image mimeType conformance validator
- KHR_gaussian_splatting ellipse-kernel attribute conformance validation
- camera property validation per core spec §5.12–§5.14
- round 269: KHR_animation_pointer Object-Model pointer-template registry + bool output lane
- KHR_animation_pointer non-FLOAT output accessor lanes
- accessor.sparse.values.bufferView §5.4.1 validator
- KHR_draco_mesh_compression byteStride MUST-NOT validator
- drop release-plz.toml — use release-plz defaults across the workspace
- KHR_draco_mesh_compression per-primitive descriptor parser + validators
- KHR_gaussian_splatting per-primitive descriptor parser
- KHR_meshopt_compression descriptor parser + validators
- KHR_texture_basisu extension (per-texture KTX2 indirection)
- KHR_mesh_quantization morph-target decode + encode
- KHR_mesh_quantization encoder (re-quantise base attrs + declare required)
- KHR_animation_pointer (decode + encode + §3.12 + 10 tests)
- KHR_xmp_json_ld extension (decode + encode + §3.12 validation)
- KHR_materials_variants extension (decode + encode + §3.12 validation)

### Added (round 311)

- Core accessor property validation per glTF 2.0 spec §3.6.2 (Accessor
  Data) + §5.1 (Accessor). A new `validate_accessors` pass in
  `src/validation.rs`, wired into `convert()` after the bufferView-fit /
  sparse-bufferView checks and before camera validation, enforces three
  document-level MUSTs on every `accessors[i]` entry (referenced or not):
  - §5.1 `accessor.count` "Minimum: >= 1" — a zero-element accessor is
    rejected with `AccessorCount`.
  - §5.1.6 / §3.6.2.1 `accessor.normalized` "MUST NOT be set to `true`
    for accessors with `FLOAT` or `UNSIGNED_INT` component type" —
    rejected with `AccessorNormalizedComponentType` (normalization is the
    integer→[0,1]/[-1,1] decode, undefined for a float and lacking a
    §3.6.2.2 dequantisation row for 32-bit unsigned int).
  - §3.6.2.5 (Accessor Bounds) "The length of these arrays MUST be equal
    to the number of accessor's components" — `min` / `max`, when present,
    MUST carry exactly `type_components(type)` entries (one of
    1/2/3/4/9/16); a mismatch is rejected with `AccessorMinMaxLength`.
    The check defers to the bufferView-fit pass for an unknown `type`
    string (no component count to compare against).
  - 7 end-to-end tests in `tests/accessor_property_validation.rs` (driven
    through the public `GltfDecoder`) plus 8 unit tests in
    `src/validation.rs` (conformant spread accept, zero-count,
    normalized-FLOAT, normalized-UNSIGNED_INT, short-min, long-max,
    MAT4 16-component bounds, unknown-type skip).

### Added (round 306)

- Texture-sampler filter / wrap validation per glTF 2.0 spec §5.26
  (Sampler). A new `validate_samplers` pass in `src/validation.rs`,
  wired into `convert()` before buffer materialisation, enforces the
  closed enum sets from §5.26.1–§5.26.4 on every `samplers[i]` entry:
  `magFilter` ∈ { 9728 NEAREST, 9729 LINEAR } (`SamplerMagFilter`);
  `minFilter` ∈ { 9728, 9729, 9984, 9985, 9986, 9987 }
  (`SamplerMinFilter`); `wrapS` / `wrapT` ∈ { 33071 CLAMP_TO_EDGE,
  33648 MIRRORED_REPEAT, 10497 REPEAT } (`SamplerWrapS` /
  `SamplerWrapT`). Absent properties remain valid (wrapS/wrapT carry a
  spec default of REPEAT; the filters have no default) — only an
  out-of-set integer is rejected. Covered by eight unit tests in
  `src/validation.rs` plus an end-to-end `tests/sampler_validation.rs`
  that pins the `convert()` wiring through the public `GltfDecoder`.

### Added (round 300)

- Node-hierarchy + node-transform validation per glTF 2.0 spec §3.5.2
  (node hierarchy) and §3.5.3 (transformations). A new `validate_nodes`
  pass in `src/validation.rs`, wired into `convert()` before buffer
  materialisation, enforces every hard MUST in those two sections:
  - §3.5.2 "The node hierarchy MUST be a set of disjoint strict trees …
    MUST NOT contain cycles and each node MUST have zero or one parent
    node" — child indices MUST resolve into `nodes[]`
    (`NodeChildIndex`); a node MUST NOT appear in two parents'
    `children` (`NodeMultipleParents`); the parent-link walk MUST NOT
    close a cycle, which also catches a node listing itself as a child
    (`NodeHierarchyCycle`).
  - §3.5.3 `matrix` ⊥ TRS (`NodeMatrixTRSExclusive`); an
    animation-targeted node MUST use TRS only, never `matrix`
    (`NodeAnimatedMatrix`); `rotation` MUST be a finite unit quaternion
    (`NodeRotationUnitQuaternion`, ~2e-3 length tolerance absorbing
    normalized-integer round-trip); `translation` / `scale` / `matrix`
    components MUST be finite (`NodeTranslationFinite` /
    `NodeScaleFinite` / `NodeMatrixFinite`); and a `matrix` MUST be
    decomposable to TRS — a zero/non-finite upper-left-3×3 determinant
    is rejected (`NodeMatrixDecompose`), conservatively leaving the
    shear/skew SHOULD-NOT sub-case (an Implementation Note) accepted.
  - 15 end-to-end tests in `tests/node_hierarchy_validation.rs` plus 4
    unit tests in `src/validation.rs` (invertible-shear accept,
    long-chain cycle, non-finite translation, deep strict tree).

### Added (round 294)

- `KHR_texture_basisu` target-image mimeType conformance per
  `docs/3d/gltf/extensions/KHR_texture_basisu.md` §Overview + §"glTF
  Schema Updates" ("the image that points to the KTX v2 resource uses
  the mimeType value of image/ktx2"). The per-texture basisu validator
  in `src/validation.rs` now resolves `KHR_texture_basisu.source` to
  its `images[]` entry and, when that image declares a `mimeType`,
  rejects any value other than `image/ktx2` with
  `ExtensionStackTextureBasisuMimeType`. A target image that omits
  `mimeType` (the uri-only example) stays accepted — the spec only
  constrains the value when present. Three tests added to
  `tests/khr_texture_basisu.rs` (wrong-mime reject, `image/ktx2`
  accept, no-mime accept).

### Added (round 287)

- `KHR_gaussian_splatting` ellipse-kernel attribute-conformance
  validation per `docs/3d/gltf/extensions/KHR_gaussian_splatting.md`
  §"Ellipse Kernel" §"Attributes" + §"Spherical Harmonics Attributes".
  A new `validate_gaussian_splatting_attributes` pass in
  `src/validation.rs` runs for every primitive whose descriptor carries
  the base `"ellipse"` kernel and rejects: a missing required semantic
  (`POSITION` / `:ROTATION` / `:SCALE` / `:OPACITY` /
  `:SH_DEGREE_0_COEF_0`) with
  `ExtensionStackGaussianSplattingMissingAttribute`; an accessor whose
  `type` does not match the kernel table (ROTATION = VEC4, SCALE = VEC3,
  OPACITY = SCALAR, SH coefficients = VEC3) with
  `ExtensionStackGaussianSplattingAttributeType`; an accessor whose
  component-type + normalized form is outside the per-attribute allowed
  set with `ExtensionStackGaussianSplattingAttributeComponent`; and a
  partially-defined spherical-harmonics cascade (any used degree `l` in
  1..=3 missing a `COEF_0..2l`, or a skipped lower degree) with
  `ExtensionStackGaussianSplattingSHIncomplete`. Vendor-prefixed kernels
  defer the contract to the kernel-defining extension and skip the pass.
  11 new tests in `tests/khr_gaussian_splatting.rs`.

### Added (round 277)

- Camera property validation per core spec §5.12 + §5.13 + §5.14. A
  new `validate_cameras` pass in `src/validation.rs` runs inside
  `convert()` over every `cameras[i]` entry (referenced by a node or
  not) and rejects the spec's MUST-level violations with stable
  prefixes: `CameraProjectionExclusive` (perspective and orthographic
  blocks are mutually exclusive per §5.12), `CameraOrthographicXmag`
  / `CameraOrthographicYmag` (magnification MUST NOT be zero,
  §5.13.1/.2), `CameraOrthographicZfar` (`zfar > 0`, §5.13.3),
  `CameraOrthographicZRange` (`zfar > znear`, §5.13.3),
  `CameraOrthographicZnear` (`znear >= 0`, §5.13.4),
  `CameraPerspectiveYfov` (`yfov > 0`, §5.14.2),
  `CameraPerspectiveZnear` (`znear > 0`, §5.14.4),
  `CameraPerspectiveAspectRatio` (when defined, `> 0`, §5.14.1), and
  `CameraPerspectiveZfar` / `CameraPerspectiveZRange` (when defined,
  `zfar > 0` and `> znear`, §5.14.3). Non-finite values (NaN / ±∞)
  are rejected by the same rules so a NaN `znear` can't slip through
  the comparisons. SHOULD-level advice (non-negative magnification,
  `yfov < π`) is deliberately not enforced, and an undefined
  perspective `zfar` (infinite projection) stays valid. Covered by
  six unit tests in `src/validation.rs` plus the new
  `tests/camera_validation.rs` end-to-end suite (10 tests) that pins
  the decoder wiring.

### Added (round 269)

- `KHR_animation_pointer` Object-Model pointer-template registry +
  `bool` output lane. New `src/object_model.rs` module holds the
  pointer templates whose Object Model Data Type is not in the
  `float*` family, transcribed from the staged extension specs'
  §"Extending glTF 2.0 Asset Object Model" tables — today the single
  row `/nodes/{}/extensions/KHR_node_visibility/visible` → `bool`
  from `docs/3d/gltf/extensions/KHR_node_visibility.md`. Template
  matching treats `{}` as exactly one RFC 6901 §4 array-index token
  (digits, no leading zero); unmatched pointers keep the r261
  `float*` conversion branch unchanged. Per
  `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Output
  Accessor Component Types", a registry-matched `bool` channel
  decodes each output component with the truthiness rule (`0` →
  `false`, any other value → `true`) and surfaces JSON booleans in
  the `Scene3D::extras["KHR_animation_pointer"]` sidecar under a new
  `output_data_type: "bool"` key (absent key = `float*` lane, so
  r261-and-earlier sidecars round-trip unchanged); the encoder
  re-emits a SCALAR UNSIGNED_BYTE accessor holding canonical 0/1
  bytes with a STEP sampler and refuses malformed hand-authored
  bool sidecars (non-STEP interpolation, non-SCALAR kind,
  non-UNSIGNED_BYTE componentType). Three new
  `validate_extension_stack` rules enforce the spec MUSTs on decode:
  `ExtensionStackAnimationPointerBoolType` (the §Operation data-type
  table pins `bool` → SCALAR),
  `ExtensionStackAnimationPointerBoolComponentType` ("the output
  accessor component type MUST be unsigned byte"), and
  `ExtensionStackAnimationPointerBoolInterpolation` ("Animation
  samplers used with `int` or `bool` Object Model Data Types MUST
  use STEP interpolation" — an absent interpolation key defaults to
  LINEAR per §3.11 and is equally rejected). 8 new integration tests
  in `tests/khr_animation_pointer.rs` (bool decode, three rejection
  paths, default-LINEAR rejection, GLB round-trip with truthy-byte
  canonicalisation, float-lane fallback for unregistered pointers,
  encode-time STEP refusal) + 3 registry unit tests in
  `object_model::tests`. The `int` branch remains deferred: the core
  Object Model table (`ObjectModel.adoc`) is not staged under
  `docs/3d/gltf/` and no staged extension declares an `int`-typed
  mutable property, so there is no registry row to dispatch it.

### Added (round 261)

- `KHR_animation_pointer` non-FLOAT output accessor lanes — per
  `docs/3d/gltf/extensions/KHR_animation_pointer.md` §"Output Accessor
  Component Types" (`float*` Object Model Data Type branch), the decoder
  now accepts all six accessor `componentType` values for pointer
  channel outputs: FLOAT (5126) pass-through, normalised BYTE (5120) /
  UBYTE (5121) / SHORT (5122) / USHORT (5123) via the spec §3.6.2.2
  dequantisation equations (`f = max(c/127, -1)` / `f = c/255` / `f =
  max(c/32767, -1)` / `f = c/65535`), and non-normalised BYTE / UBYTE /
  SHORT / USHORT / UINT (5125) cast directly to `f32` per spec line 93
  ("`1` to `1.0`"). The `Scene3D::extras["KHR_animation_pointer"]`
  sidecar gains `output_component_type` + `output_normalized` keys
  recording the source accessor format; the encoder re-emits the same
  on-the-wire format via per-component quantisers (re-using the
  existing `quantize_u8` / `quantize_u16` / `quantize_i8` /
  `quantize_i16` helpers for the normalised lanes) and a new family of
  range-clamping casts (`truncate_to_{u8,u16,u32,i8,i16}`) for the
  non-normalised lanes. Sidecars omitting the new keys default to
  FLOAT + normalised=false, preserving r218 documents unchanged. The
  decoder rejects `componentType=5125` with `normalized=true` (no
  §3.6.2.2 row for normalised UINT) and the encoder symmetrically
  refuses the same combination. 12 new tests in
  `tests/khr_animation_pointer.rs` lock in: per-mode decode for all
  four normalised-integer lanes (including the §3.6.2.2 reserved-slot
  rule that clamps i8 `-128` and i16 `-32768` to `-1.0`), per-mode
  decode for UBYTE / SHORT / UINT non-normalised lanes, the
  normalised-UINT rejection, full encode→decode round-trips for
  UBYTE-normalised / SHORT-normalised / UINT-unnormalised that confirm
  the emitted JSON carries `"componentType":5121` / `5122` / `5125` and
  `"normalized":true` / `false` (not silently widened to FLOAT), and a
  legacy-sidecar test confirming r218 documents still encode as FLOAT.
  The `int` / `bool` Object Model Data Type branches require a
  pointer-string property registry to dispatch; deferred to a follow-up
  round (rolled into the README roadmap).

### Added (round 256)

- `accessor.sparse.values.bufferView` validator —
  `SparseValuesBufferViewTarget` / `SparseValuesBufferViewStride` /
  `SparseValuesBufferViewIndex` reject a sparse-accessor whose
  `values.bufferView` carries a `target` or `byteStride` property, or
  resolves out of range. Per glTF 2.0 spec §5.4.1 the sparse-values
  bufferView MUST NOT define `target` or `byteStride`; per §5.4 the
  override elements are "tightly packed", so a strided layout is
  semantically nonsensical and a `target` hint (ARRAY_BUFFER /
  ELEMENT_ARRAY_BUFFER) is equally wrong on a tightly-packed scratch
  block. This is the symmetric companion to the §5.3.1
  `sparse.indices.bufferView` validator landed in round 8 — the spec
  paragraph repeats the same MUST-NOT rule for the two sides of the
  sparse triple. Seven new tests in `validation::tests` lock in
  rejection for both target sentinels and a non-zero stride, an
  out-of-range bufferView index, acceptance of a clean sparse block,
  the no-op path for non-sparse accessors, and independence from the
  `sparse.indices.bufferView` rule (a stride on the indices side
  doesn't trigger the values-side validator). Wired into `convert()`
  alongside `validate_sparse_indices_buffer_views` so every decode
  path runs both checks before buffer materialisation.

### Added (round 249)

- `KHR_draco_mesh_compression` validator extension —
  `ExtensionStackDracoByteStride` rejects a per-primitive descriptor
  whose `bufferView` refers to a bufferView that defines
  `byteStride`. Per glTF 2.0 §5.11.4 a `byteStride` is reserved for
  vertex attribute data layouts ("Buffer views with other types of
  data MUST NOT define byteStride (unless such layout is explicitly
  enabled by an extension)"); the Draco descriptor's bufferView
  holds an opaque compressed payload, neither vertex attribute data
  nor an indexed array, and `KHR_draco_mesh_compression` does not
  enable a strided payload layout, so a stride on that bufferView is
  semantically nonsensical. The check has the same shape as the
  §5.3.1 sparse-indices `MUST NOT have byteStride` rule already in
  this validator. Three new tests in
  `tests/khr_draco_mesh_compression.rs` lock in the rejection for
  two distinct strides inside the `[4, 252]` generic range and the
  acceptance of a stride-less Draco bufferView.

### Added (round 246)

- `KHR_draco_mesh_compression` extension per
  `docs/3d/gltf/extensions/KHR_draco_mesh_compression.md` §"glTF
  Schema Updates" — the per-primitive descriptor block that redirects
  a mesh primitive's geometry to a Draco-compressed `bufferView`
  payload. The descriptor carries a `bufferView` indirection plus an
  `attributes` map pairing the parent primitive's attribute names
  (POSITION, NORMAL, …) with the Draco-side unique attribute IDs.
  The decoder surfaces the descriptor through
  `Primitive::extras["KHR_draco_mesh_compression"]` as a JSON object
  so the typed `oxideav_mesh3d::Primitive` round-trips without
  growing a bespoke compressed-payload slot. The encoder lifts the
  sidecar back into the typed `PrimitiveExtensions` block, emits the
  `bufferView` + `attributes` map verbatim, and appends
  `KHR_draco_mesh_compression` to `extensionsUsed` exactly once per
  document. The crate is a pass-through engine — the Draco bitstream
  inflate path is out of scope for this round — so the parent
  primitive's uncompressed-fallback accessors are processed through
  the usual accessor pipeline (per spec §"accessors": the parent
  accessors describe the decompressed data and remain authoritative
  for the uncompressed lane). A Draco-aware consumer layered above
  this crate can pick up the descriptor and inflate the compressed
  payload itself.
- §3.12 + §Conformance stack-validator coverage for
  `KHR_draco_mesh_compression`. Six failure modes surface with stable
  `ExtensionStack…` error prefixes for grep-ability alongside the
  existing extension-stack vocabulary:
  `ExtensionStackUsedNotDeclared` rejects descriptors without the
  `extensionsUsed` entry; `ExtensionStackDracoBufferView` rejects an
  out-of-range `bufferView`; `ExtensionStackDracoAttributes` rejects
  descriptor `attributes` keys that are not present in the parent
  primitive's own `attributes` map per spec §"attributes" subset
  rule; `ExtensionStackDracoAttributeId` rejects duplicate Draco-side
  attribute IDs within one descriptor per §"attributes" uniqueness
  rule; `ExtensionStackDracoMode` rejects primitive `mode` outside
  `{TRIANGLES (4), TRIANGLE_STRIP (5)}` per §"Restrictions on
  geometry type"; and `ExtensionStackDracoRequired` rejects the
  compressed-only shape (no uncompressed fallback attributes) when
  `KHR_draco_mesh_compression` is missing from `extensionsRequired`
  per §Conformance.
- New typed model node `KhrDracoMeshCompression` in `json_model.rs`
  alongside the extended `PrimitiveExtensions` block; new decoder
  stash + encoder lift passes in `json_to_scene.rs` /
  `scene_to_json.rs`; `emitted_draco_mesh_compression` tracking in
  the `convert_with_options` walk so the §3.12 declaration appears
  exactly once.
- 16 new tests in `tests/khr_draco_mesh_compression.rs` covering
  descriptor round-trip via GLB (bufferView + attributes map),
  absent-by-default omission, `extras` pass-through preservation,
  the §3.12 stack rule, all five rejection paths
  (`ExtensionStackDracoBufferView` / `ExtensionStackDracoAttributes`
  / `ExtensionStackDracoAttributeId` / `ExtensionStackDracoMode` /
  `ExtensionStackDracoRequired`), `mode` acceptance for both
  TRIANGLES and TRIANGLE_STRIP, and rejection paths for POINTS /
  LINE_LOOP / TRIANGLE_FAN.

### Added (round 243)

- `KHR_gaussian_splatting` extension per
  `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Extending
  Mesh Primitives" — the per-primitive descriptor block that flags a
  `POINTS` mesh primitive as a 3D Gaussian splat field. The descriptor
  carries four string-valued fields: `kernel` (required — the spec
  defines `"ellipse"`), `colorSpace` (required — `"srgb_rec709_display"`
  or `"lin_rec709_display"`), `projection` (optional, default
  `"perspective"`), and `sortingMethod` (optional, default
  `"cameraDistance"`).
  The decoder surfaces the descriptor through
  `Primitive::extras["KHR_gaussian_splatting"]` as a JSON object so
  the typed `oxideav_mesh3d::Primitive` round-trips without growing a
  bespoke splat slot. The encoder lifts the sentinel back into the
  typed `PrimitiveExtensions` block, emits the four fields verbatim,
  and appends `KHR_gaussian_splatting` to `extensionsUsed` exactly
  once per document. The custom attribute semantics
  (`KHR_gaussian_splatting:ROTATION` / `:SCALE` / `:OPACITY` /
  `:SH_DEGREE_l_COEF_n` per §"Ellipse Kernel" §"Attributes") flow
  through the standard accessor pipeline as raw attributes — this
  round delivers the descriptor handshake; the typed splat-field
  decode + the spherical-harmonics evaluator described in §"Lighting"
  remain for a follow-up.
- §3.12 stack validator coverage for `KHR_gaussian_splatting`:
  `ExtensionStackUsedNotDeclared` rejects a descriptor without the
  `extensionsUsed` entry (spec §3.12 + the extension's §"Extending
  Mesh Primitives" mandate). Four allowed-value rules cover the
  spec-defined strings while leaving forward-compat carve-outs open
  for vendor-extension-prefixed identifiers (`KHR_…`, `EXT_…`, vendor
  prefixes per the registry's namespacing convention):
  `ExtensionStackGaussianSplattingKernel`,
  `ExtensionStackGaussianSplattingColorSpace`,
  `ExtensionStackGaussianSplattingProjection`, and
  `ExtensionStackGaussianSplattingSortingMethod`. The
  ellipse-kernel-specific §"Ellipse Kernel" §"Dependencies on glTF"
  rule (mesh primitive `mode` MUST be `POINTS` for the base
  `"ellipse"` kernel) surfaces as
  `ExtensionStackGaussianSplattingMode` — the validator defers to a
  layered extension for any non-base kernel string so future
  triangle-based splat reconstructions can land without re-touching
  this crate.
- New typed model node `KhrGaussianSplatting` in `json_model.rs`
  alongside the extended `PrimitiveExtensions` block; new decoder
  stash + encoder lift passes in `json_to_scene.rs` /
  `scene_to_json.rs`; `emitted_gaussian_splatting` tracking in the
  `convert_with_options` walk so the §3.12 declaration appears
  exactly once.
- 15 new tests in `tests/khr_gaussian_splatting.rs` covering
  descriptor round-trip via GLB (kernel + colorSpace + projection +
  sortingMethod), absent-by-default omission, optional-field
  preservation (projection / sortingMethod absent means absent on
  encode — no synthesis), `extensionsUsed` emission, missing-`used`
  rejection, allowed-value rejection for each of kernel / colorSpace /
  projection / sortingMethod, ellipse-kernel POINTS-mode requirement
  (both explicit `mode: 4` and default-omitted `mode`),
  vendor-prefixed kernel accepted (carve-out), linear color-space
  accepted, vendor-prefixed kernel skips the mode check, and a
  multi-primitive scene appending `extensionsUsed` exactly once.

### Added (round 240)

- `KHR_meshopt_compression` extension per
  `docs/3d/gltf/extensions/KHR_meshopt_compression.md`
  §"Specifying compressed views" + §"Fallback buffers" + §"JSON
  schema updates" — per-bufferView compression descriptors +
  per-buffer `{ "fallback": true }` placeholder markers. The
  crate is a pass-through engine (the meshopt bitstream decoder
  in Appendix A is not implemented yet), so the extension is
  handled at the JSON descriptor level: the decoder captures
  every bufferView's `extensions.KHR_meshopt_compression` block
  into `Scene3D::extras["KHR_meshopt_compression"]
  .bufferViews["<bvi>"]` (carrying the full `buffer` /
  `byteOffset` / `byteLength` / `byteStride` / `count` / `mode`
  / optional `filter` descriptor) and the per-buffer fallback
  markers under `…fallbackBuffers` as an array of buffer
  indices. A uri-less fallback buffer is materialised as a
  zero-filled byte vector of the declared `byteLength` so
  downstream bufferView slicing remains safe; consumers wiring
  up a meshopt decoder lane later can inflate the real bytes
  into that region from the descriptor's compressed source
  range. On encode the sidecar is stripped from `scene.extras`
  and the descriptors are NOT re-emitted onto the freshly-built
  uncompressed bufferViews — documents written by this crate
  are always uncompressed (the compression is a load-time
  concern only).
- §3.12 stack validator coverage for `KHR_meshopt_compression`:
  `ExtensionStackUsedNotDeclared` (data block on any
  bufferView/buffer without the declaration);
  `ExtensionStackMeshoptRequired` (uri-less fallback buffer
  without `extensionsRequired` per spec §"Fallback buffers");
  `ExtensionStackMeshoptMode` / `ExtensionStackMeshoptFilter` /
  `ExtensionStackMeshoptLayout` / `ExtensionStackMeshoptStride`
  / `ExtensionStackMeshoptCount` (§"JSON schema updates"
  per-rule invariants); `ExtensionStackMeshoptBuffer` /
  `ExtensionStackMeshoptRange` (source buffer index + range
  bounds); `ExtensionStackMeshoptFallbackRef` (a fallback
  buffer referenced by a bufferView WITHOUT the extension) /
  `ExtensionStackMeshoptFallbackSource` (a descriptor's own
  `buffer` pointing at a fallback buffer).
- Added new typed model nodes `BufferViewExtensions`,
  `KhrMeshoptCompression`, `BufferExtensions`,
  `KhrMeshoptBufferFallback` in `json_model.rs`, plumbed
  `extensions: Option<…>` through `BufferView` + `Buffer`,
  taught `resolve_buffers` to recognise the fallback shape, and
  added the sidecar capture + strip passes in `json_to_scene.rs`
  / `scene_to_json.rs`.
- 23 new tests in `tests/khr_meshopt_compression.rs` covering
  descriptor lift, filter capture, fallback-buffer
  materialisation, encode strips sidecar, §3.12 used-not-declared
  rejection, fallback-without-required rejection, unknown
  mode/filter rejection, parent-layout-mismatch rejection, per
  mode byteStride / count invariants (ATTRIBUTES bounds,
  TRIANGLES divisibility-by-3, INDICES stride),
  TRIANGLES-with-non-NONE-filter rejection, per-filter
  byteStride invariants (QUATERNION, EXPONENTIAL,
  OCTAHEDRAL/COLOR), out-of-range `extension.buffer`, source
  range overrun, fallback buffer referenced by a plain
  bufferView, and descriptor `buffer` pointing at a fallback
  buffer. Bare documents without the extension stay unaffected.

### Added (round 233)

- `KHR_texture_basisu` extension per
  `docs/3d/gltf/extensions/KHR_texture_basisu.md` §glTF Schema
  Updates — per-texture indirection to a KTX v2 image with Basis
  Universal supercompression. The crate is a pass-through engine
  (no KTX2 / Basis transcode lane yet), so the decoder routes the
  texture's image load through one of the two spec-defined shapes:
  "with fallback" picks the base `texture.source` PNG/JPEG as the
  live image (the extension's KTX2 source is acknowledged but the
  PNG/JPEG path is the one we materialise), and "without
  fallback" loads the extension's KTX2 image as opaque bytes via
  the usual `BufferViewAsset` / `InMemoryAsset` route carrying the
  spec's `image/ktx2` MIME. Scene-texture indices loaded via the
  "without fallback" path are recorded under
  `Scene3D::extras["KHR_texture_basisu"].textures` so the encoder
  re-emits the same shape on write: `texture.source` omitted,
  `extensions.KHR_texture_basisu.source` pointing at the re-emitted
  image, and the extension declared in BOTH `extensionsUsed` AND
  `extensionsRequired` per the spec §"Using Without a Fallback".
  Added new typed model nodes `TextureExtensions` and
  `TextureBasisu` in `json_model.rs`, threaded a tuple return
  through `convert_texture` for the sidecar accumulation, and
  added the extension declaration emit gate to the encoder.
  Twelve new tests in `tests/khr_texture_basisu.rs` cover the
  with-fallback / without-fallback decode shapes, the
  sidecar-driven encode round-trip back to "without fallback", a
  regression guard that plain PNG textures don't grow phantom
  extensions, the externally-staged `image.ktx2` URI and a
  `data:image/ktx2;base64,...` URI shape, and three §3.12 stack
  rejection rules: `ExtensionStackUsedNotDeclared` (data block on
  any texture without the declaration),
  `ExtensionStackTextureBasisuSource` (out-of-range source image
  index), and `ExtensionStackTextureBasisuRequired` (no base
  fallback `source` requires `KHR_texture_basisu` in
  `extensionsRequired` per the spec example). All twelve pass.

### Added (round 230)

- `KHR_mesh_quantization` morph-target attribute decode + encode per
  `docs/3d/gltf/extensions/KHR_mesh_quantization.md` §Extending Morph
  Target Attributes. Morph deltas may now be stored as 8-bit / 16-bit
  signed integers (`POSITION` VEC3 BYTE / BYTE-normalized / SHORT /
  SHORT-normalized; `NORMAL` and `TANGENT` VEC3 BYTE-normalized /
  SHORT-normalized; `TEXCOORD_n` VEC2 BYTE / SHORT). Morph TANGENT
  stays VEC3 — the §Extending Morph Target Attributes table strips
  the handedness `W` from the delta since handedness can't be morphed
  (spec §3.7.2.2). Each non-FLOAT morph accessor is dequantised
  through the existing spec int→float equations and surfaces as f32
  deltas under the per-primitive `__morph_targets` sentinel; the
  original `(componentType, normalized)` tuple is stashed under a new
  per-primitive `__morph_attr_quant` sentinel keyed by
  `<target-index>.<attribute>` so the encoder can re-quantise on
  write without promoting to FLOAT. The encoder honours the sentinel
  on both the typed `Primitive.targets` path (POSITION / NORMAL /
  TANGENT VEC3) and the `__morph_targets` extras path
  (which additionally carries TEXCOORD_n VEC2), padding to the
  spec-mandated 4-byte element stride per §Extending Morph Target
  Attributes ("`VEC3` accessors need to be aligned to 4-byte
  boundaries; e.g. a `BYTE` normal is expected to have a stride of
  4"). `__morph_attr_quant` participates in the same
  `extensionsUsed` + `extensionsRequired` declaration gate as
  `__attr_quant` per §Overview ("the extension is not optional").
  Quantised morph accessors whose `(componentType, normalized)` pair
  falls outside the morph combo table are refused at decode time
  (`is_morph_attr_combo_allowed`), and the decode path itself is
  gated on the extension being declared in `extensionsUsed`. Six new
  tests in `tests/quantized_morph_targets.rs` exercise: SHORT-
  normalized POSITION dequantise via the spec equation
  (`f = max(c/32767, -1)` with the -32768 clamp), JSON round-trip
  preserving `extensionsUsed` + `extensionsRequired` and the
  SHORT/normalized accessor form, BYTE-normalized POSITION GLB
  round-trip within `1/127` precision, SHORT-normalized NORMAL +
  TANGENT VEC3 round-trip with morph TANGENT staying VEC3 in the
  re-encoded JSON, refusal when the extension isn't declared, and
  unnormalized-BYTE TEXCOORD_0 morph round-trip.

### Added (round 223)

- `KHR_mesh_quantization` encoder path — float→int re-emission of
  base mesh attributes recorded under the per-primitive
  `extras["__attr_quant"]` sentinel per
  `docs/3d/gltf/extensions/KHR_mesh_quantization.md` §Encoding
  Quantized Data. `POSITION` / `NORMAL` / `TANGENT` / `TEXCOORD_n`
  whose decoded form carried a non-FLOAT (componentType, normalized)
  pair are re-quantised through the spec's float→int table
  (BYTE `c = round(f * 127.0)`, UBYTE `c = round(f * 255.0)`, SHORT
  `c = round(f * 32767.0)`, USHORT `c = round(f * 65535.0)`), then
  written into the binary buffer with the spec-mandated 4-byte
  element stride (§Extending Mesh Attributes "a BYTE normal is
  expected to have a stride of 4, not 3"). POSITION `accessor.min`
  / `accessor.max` carry the quantised integer values per the
  Implementation Note in §Extending Mesh Attributes ("For quantized
  data, `accessor.min` and `accessor.max` properties also contain
  quantized values"). The (attribute, kind, componentType,
  normalized) tuple is gated against the `is_base_attr_combo_allowed`
  table — out-of-table combos fall back to the FLOAT encode path so
  the encoder never emits a non-spec form. The `__attr_quant`
  sentinel is stripped from per-primitive `extras` on write so it
  doesn't surface in the JSON output. The encoder declares
  `KHR_mesh_quantization` in BOTH `extensionsUsed` AND
  `extensionsRequired` per §Overview ("files that use the extension
  must specify it in extensionsRequired array - the extension is
  not optional"). Five new tests in
  `tests/quantized_attribute_encode.rs` exercise SHORT-normalized
  POSITION (extension declared + accessor stays SHORT/normalized +
  min/max integer-valued + decode-encode-decode within
  `2 / 32767` precision), BYTE-normalized NORMAL + UBYTE-normalized
  TEXCOORD_0 round-trip, BYTE-normalized TANGENT VEC4 round-trip,
  and FLOAT-only-scene-stays-FLOAT (no extensionsRequired surfacing).

### Added (round 218)

- `KHR_animation_pointer` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_animation_pointer.md`). Animation
  channels that drive arbitrary mutable glTF properties via a JSON
  Pointer (RFC 6901) per §"Extension Usage". Pointer-targeted channels
  carry `target.path = "pointer"` and store the pointer string at
  `target.extensions.KHR_animation_pointer.pointer`; because they
  don't bind to a node, the base spec would silently discard them, so
  the decoder siphons them into
  `Scene3D::extras["KHR_animation_pointer"]` as
  `{ "animations": [ { "animation": ai, "name": "...", "channels": [
  { "channel": ci, "pointer": "...", "interpolation": "...", "input":
  [...f32...], "output_kind": "SCALAR"|"VEC2"|…|"MAT4", "output":
  [...f32...] } ] } ] }`. The encoder lifts each entry back into the
  typed channel target (emitting fresh FLOAT-typed input + output
  accessors and a sampler) and appends `KHR_animation_pointer` to
  `extensionsUsed`. Round 218 carries the FLOAT output lane only —
  the spec's normalized-int / non-normalized-int / `bool` output
  conversion modes (§"Output Accessor Component Types") follow in a
  later round. The §3.12 stack validator rejects documents carrying
  the data block without the declaration
  (`ExtensionStackUsedNotDeclared`); rejects pointer channels with
  `target.node` set (`ExtensionStackAnimationPointerNode`); rejects
  the path/extension consistency violations
  (`ExtensionStackAnimationPointerPath` /
  `ExtensionStackAnimationPointerData`); rejects duplicate pointers
  within one animation (`ExtensionStackAnimationPointerDuplicate` —
  spec §Operation); and rejects malformed RFC 6901 prefixes
  (`ExtensionStackAnimationPointerSyntax`). Existing animation-channel
  path validation widens to accept `"pointer"`.

### Added (round 212)

- `KHR_xmp_json_ld` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_xmp_json_ld.md`). XMP (ISO 16684-1)
  metadata indirection: a root-level
  `extensions.KHR_xmp_json_ld.packets[]` roster of opaque JSON-LD
  packets (§"Defining XMP Metadata") plus a `{ "packet": N }`
  indirection on the `asset`, `scene`, `node`, `mesh`, or `material`
  object (§"Instantiating XMP metadata"). Decoder lifts the root
  roster into `Scene3D::extras["KHR_xmp_json_ld"] = { "packets": [...] }`
  with packets held verbatim (the spec restricts JSON-LD in
  §"Restrictions and Recommendations" but does not pin the namespace
  vocabulary), per-asset / per-primary-scene refs into
  `Scene3D::extras["__asset_xmp_packet"]` /
  `Scene3D::extras["__primary_scene_xmp_packet"]` as bare JSON
  numbers, per-node / per-material refs into
  `Node::extras["KHR_xmp_json_ld"]` /
  `Material::extras["KHR_xmp_json_ld"]`, and per-mesh refs into
  `primitive[0].extras["__mesh_xmp_packet"]` (mesh3d's `Mesh` has no
  `extras` field, matching the existing `__mesh_extras` /
  `__mesh_weights` side-channels). Encoder lifts each side channel
  back into the typed extension block and appends `KHR_xmp_json_ld`
  to `extensionsUsed` whenever any scope surfaces the data. New
  `validate_extension_stack` arm rejects documents carrying the data
  block without the declaration with
  `ExtensionStackUsedNotDeclared`, and additionally enforces the
  spec's indirection model by rejecting per-object `{ "packet": N }`
  references whose index lies outside the root `packets[]` array
  with `ExtensionStackXmpPacketIndex`. New `tests/khr_xmp_json_ld.rs`
  covers `.glb` round-trips for asset / scene / node / mesh /
  material packet refs, byte-for-byte packet content preservation,
  bare-roster (declarations only) documents, the missing-declaration
  rejection, and the out-of-range packet-index rejection.
- New `json_model::AssetExtensions`, `SceneExtensions`,
  `MeshExtensions`, `XmpPacketRef`, and `KhrXmpJsonLdRoot` shapes plus
  matching `extensions: Option<...>` field on `Asset`, `Scene`, and
  `Mesh`. The existing `MaterialExtensions` and `NodeExtensions`
  gained a `khr_xmp_json_ld: Option<XmpPacketRef>` field.

### Added (round 205)

- `KHR_materials_variants` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_variants.md`). The extension
  stores a root-level array of named material variants the document
  can switch between, paired with per-primitive `mappings` tables that
  pair a material index with the variant indices that should select it.
  Decoder reads the root `extensions.KHR_materials_variants.variants`
  roster and lifts it into `oxideav_mesh3d::Scene3D::extras
  ["KHR_materials_variants"]` as `{ "variants": [{ "name": "...",
  "extras": ... }, ...] }`; the per-primitive `extensions.KHR_materials_variants.mappings`
  list lifts into `oxideav_mesh3d::Primitive::extras["KHR_materials_variants"]`
  as `{ "mappings": [{ "material": idx, "variants": [idx, ...],
  "name": "...", "extras": ... }, ...] }`. Encoder lifts both back into
  the typed root + primitive extension blocks and appends
  `KHR_materials_variants` to `extensionsUsed` whenever the roster or
  any per-primitive mapping survives. New `validate_extension_stack`
  arm rejects documents carrying either data block without the
  declaration with the stable `ExtensionStackUsedNotDeclared` prefix;
  three further spec-explicit value-range checks reject mapping
  `material` indices outside the materials roster
  (`ExtensionStackVariantsMaterialIndex`), variant indices outside the
  root roster (`ExtensionStackVariantsIndex`), and per-primitive
  duplicate variant indices (`ExtensionStackVariantsDuplicate` — per
  the spec "Across the entire mappings array, each variant index must
  be used no more than one time"). New `tests/khr_materials_variants.rs`
  (11 tests) covers GLB round-trips for the roster + mappings, the
  `extensionsUsed` emission shape, omission when no variants are
  present, the §3.12 rejection path, the declared-decode path, the
  three value-range rejections, the docs-example sneaker mapping, the
  empty-roster edge case, the per-mapping `name`/`extras` passthrough,
  and the typed-JSON-shape sanity check. Six new unit tests in
  `validation.rs` cover each branch of the new validator directly.

### Added (round 199)

- `KHR_node_visibility` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_node_visibility.md`). The extension
  defines a single optional Boolean `visible` flag on a node, with a
  spec default of `true` per §Extending Nodes; a value of `false`
  hides the node and all its descendant subtree. Decoder reads the
  per-node `extensions.KHR_node_visibility.visible` field and lifts it
  into `oxideav_mesh3d::Node::extras["KHR_node_visibility"]` as a
  `Value::Bool` (a bare `{}` object resolves to the spec default of
  `true`). Encoder pulls the boolean back out of `Node::extras`,
  rebuilds the typed `KHR_node_visibility` extension object on the
  node, and appends `KHR_node_visibility` to `extensionsUsed`. New
  `validate_extension_stack` arm rejects nodes carrying the data block
  without the declaration with the stable `ExtensionStackUsedNotDeclared`
  prefix. The two per-node extensions (`KHR_lights_punctual` +
  `KHR_node_visibility`) coexist on a single node, exercised by an
  integration test. New `tests/khr_node_visibility.rs` (8 tests)
  covers `visible=false` and `visible=true` round-trips, the
  `extensionsUsed` emission shape, omission when no node sets the
  flag, the §3.12 rejection path, the declared-decode path, the bare
  `{}` → default-`true` resolution, and the coexistence with
  `KHR_lights_punctual` on the same node. Two new unit tests in
  `validation.rs` cover the `validate_extension_stack`
  rejection-and-acceptance arms directly.

## [0.0.2](https://github.com/OxideAV/oxideav-gltf/compare/v0.0.1...v0.0.2) - 2026-05-29

### Added

- KHR_mesh_quantization decode (quantized vertex attributes)
- KHR_materials_diffuse_transmission extension (round 164)
- KHR_materials_dispersion extension (chromatic-aberration scalar)
- KHR_materials_anisotropy extension (asymmetric specular lobe)
- KHR_texture_transform extension (per-textureInfo UV affine transform)
- KHR_materials_volume extension (round 120)
- KHR_materials_transmission extension (round 117)
- KHR_materials_sheen extension (round 114)
- KHR_materials_clearcoat extension (decode + encode + §3.12 validation)

### Other

- Add KHR_materials_iridescence extension (round 129)
- round 126: cargo-fuzz harness for glTF JSON + .glb binary parser
- Add KHR_materials_specular extension (decode + encode + §3.12 validation)
- add KHR_materials_ior extension (decode + encode + §3.12 validation)
- KHR_materials_emissive_strength — decode + encode + §3.12 validation (r98)
- KHR_materials_unlit — decode + encode + §3.12 validation (r93)
- round 8: accessor/bufferView fit + sparse-indices restriction validation
- Round 75: GLB hardening + JSON-to-scene validation extension
- Validate extension stack + animation paths; harden JSON parser (r7)

### Added (round 188)

- `KHR_mesh_quantization` decode support (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_mesh_quantization.md`). The extension
  widens the allowed vertex-attribute component types beyond `FLOAT`:
  `POSITION` / `NORMAL` / `TANGENT` / `TEXCOORD_n` accessors may now use
  8-/16-bit signed/unsigned integer storage (normalized or
  unnormalized). New `src/quantization.rs` module implements the spec's
  int→float dequantization table — `5120` BYTE `f = max(c/127, -1)`,
  `5121` UNSIGNED_BYTE `f = c/255`, `5122` SHORT `f = max(c/32767, -1)`,
  `5123` UNSIGNED_SHORT `f = c/65535` — plus the matching float→int
  helpers and the §Extending Mesh / Morph Target Attributes allowed-combo
  tables. The decoder (`json_to_scene.rs`) dispatches `read_attr_vec2`
  / `vec3` / `vec4` to the dequantizer when an attribute accessor is a
  non-`FLOAT` quantized type: normalized integers run the spec equation,
  unnormalized integers cast directly to `f32` (spec: "unnormalized
  integer 2 corresponds to 2.0"). A quantized base attribute is gated on
  `KHR_mesh_quantization` appearing in `extensionsUsed` AND the
  (componentType, normalized) pair being in the extension's allowed set
  for that attribute — otherwise the decode is rejected with a stable
  message. The base-spec §3.7.2.1 UNSIGNED_BYTE / UNSIGNED_SHORT
  *normalized* `TEXCOORD` types remain accepted without the extension.
  Each quantized attribute's storage form is recorded under the
  primitive's `extras["__attr_quant"]` sentinel (componentType +
  normalized, per attribute name) so a future encoder pass can
  round-trip the original quantized form; plain all-`FLOAT` primitives
  do not gain the sentinel. New `tests/khr_mesh_quantization.rs` (7
  tests) covers SHORT-normalized POSITION dequantization with the
  `-32768/32767 → -1.0` clamp, BYTE-normalized NORMAL, unnormalized
  SHORT TEXCOORD direct-cast, base-spec UBYTE-normalized TEXCOORD
  without the extension, the extension-required rejection path, the
  `__attr_quant` sentinel shape, and FLOAT-primitive sentinel absence.
  Encoder emission of quantized attributes is deferred to a follow-up
  round.

### Added (round 164)

- `KHR_materials_diffuse_transmission` extension (Khronos ratified —
  see `docs/3d/gltf/extensions/KHR_materials_diffuse_transmission.md`).
  Decoder reads the per-material
  `extensions.KHR_materials_diffuse_transmission` block carrying any of
  the four spec-defined keys (`diffuseTransmissionFactor`,
  `diffuseTransmissionTexture`, `diffuseTransmissionColorFactor`,
  `diffuseTransmissionColorTexture`) and lifts it into
  `oxideav_mesh3d::Material::extras["KHR_materials_diffuse_transmission"]`
  as a JSON `Value::Object`; a bare `{}` resolves to the spec defaults
  `diffuseTransmissionFactor = 0.0` (zero disables the layer) and
  `diffuseTransmissionColorFactor = [1, 1, 1]`. Texture infos
  round-trip with `index` + optional `texCoord` preserved. Encoder
  lifts the object back into the typed extensions block and appends
  `KHR_materials_diffuse_transmission` to `extensionsUsed`. §3.12
  stack validator additionally enforces the spec's implicit range
  constraints — `diffuseTransmissionFactor` MUST be finite and within
  `[0, 1]` (the spec defines it as a percentage with `1.0` meaning
  100% of penetrating light is transmitted —
  `ExtensionStackDiffuseTransmissionFactorRange`), and each component
  of `diffuseTransmissionColorFactor` MUST be finite and within
  `[0, 1]` (it is a "proportion of light at each color channel" —
  `ExtensionStackDiffuseTransmissionColorRange`) — and rejects
  materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`). New
  `tests/khr_materials_diffuse_transmission.rs` (13 tests) covers GLB
  round-trip of factor + colour, `extensionsUsed` emission, the
  bare-object default, the spec §"Extending Materials" sample, the
  §3.12 rejection path, factor-above-1.0 rejection, factor-negative
  rejection, colour-out-of-range rejection, explicit-zero round-trip,
  full-record GLB round-trip, and three-extension stack co-existence
  with `KHR_materials_volume` + `KHR_materials_transmission`. Seven
  new validator unit tests cover the declared/undeclared paths plus
  the factor range (zero default, above-one, negative, non-finite)
  and the colour range (negative, above-one).

### Added (round 158)

- `KHR_materials_dispersion` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_dispersion.md`). Decoder
  reads the per-material `extensions.KHR_materials_dispersion` block
  with its single spec-defined key (`dispersion`, storing `20/Vd`
  where `Vd` is the Abbe number — the same transform Adobe Standard
  Material and ASWF OpenPBR use) and lifts it into
  `oxideav_mesh3d::Material::extras["KHR_materials_dispersion"]` as a
  JSON `Value::Object`; a bare `{}` resolves to the spec default
  `dispersion = 0.0` (no dispersion, the backwards-compatibility
  default). Values above `1.0` are explicitly allowed for artistic
  exaggeration (Rutile = `2.04` is the spec-listed example). Encoder
  lifts the object back into the typed extensions block and appends
  `KHR_materials_dispersion` to `extensionsUsed`. §3.12 stack
  validator additionally enforces the spec's "Any value zero or
  larger is considered to be a valid dispersion value" rule —
  `dispersion` MUST be finite and `>= 0`
  (`ExtensionStackDispersionRange`) — and rejects materials carrying
  the data block without the declaration
  (`ExtensionStackUsedNotDeclared`). New
  `tests/khr_materials_dispersion.rs` (11 tests) covers GLB
  round-trip, `extensionsUsed` emission, the bare-object default,
  the spec §"Extending Materials" sample, the §3.12 rejection path,
  the negative-value rejection, the `> 1.0` artistic-exaggeration
  passthrough, explicit-zero round-trip, full-record GLB round-trip,
  and three-extension stack co-existence with `KHR_materials_volume`
  + `KHR_materials_ior`. Six new validator unit tests cover the
  declared/undeclared paths plus the `0`, `> 1`, negative, and
  non-finite range cases.

### Added (round 153)

- `KHR_materials_anisotropy` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_anisotropy.md`). Decoder reads
  the per-material `extensions.KHR_materials_anisotropy` block with the
  three spec-defined keys (`anisotropyStrength`, `anisotropyRotation`,
  `anisotropyTexture`) and lifts it into
  `oxideav_mesh3d::Material::extras["KHR_materials_anisotropy"]` as a
  JSON `Value::Object`; a bare `{}` resolves to the spec defaults
  (`anisotropyStrength = 0.0` — zero disables the asymmetric specular
  lobe — and `anisotropyRotation = 0.0` radians). `anisotropyTexture`
  is a plain `textureInfo` (round-trip `index` + optional `texCoord`
  preserved). Encoder lifts the object back into the typed extensions
  block and appends `KHR_materials_anisotropy` to `extensionsUsed`.
  §3.12 stack validator additionally enforces the spec's "dimensionless
  number in the range [0, 1]" range for `anisotropyStrength`
  (`ExtensionStackAnisotropyStrengthRange`) and a finite-value check on
  `anisotropyRotation` (`ExtensionStackAnisotropyRotationFinite`), and
  rejects materials carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`). New
  `tests/khr_materials_anisotropy.rs` (12 tests) covers GLB round-trip,
  `extensionsUsed` emission, the bare-object default, the spec
  §"Extending Materials" sample, textureInfo + texCoord round-trip,
  default-texCoord omission, the §3.12 rejection path, both strength
  range violations (`-0.5` and `1.5`), full-record GLB round-trip, and
  rotation > 2π passthrough.

### Added (round 132)

- `KHR_texture_transform` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_texture_transform.md`). Adds an optional
  `extensions` block to the `textureInfo` / `normalTextureInfo` /
  `occlusionTextureInfo` JSON structs carrying a `KHR_texture_transform`
  object with the four spec-defined fields `offset` (default `[0, 0]`),
  `rotation` (default `0`), `scale` (default `[1, 1]`), and `texCoord`.
  The decoder lifts the transform from each of the five core PBR texture
  slots (`baseColorTexture`, `metallicRoughnessTexture`, `normalTexture`,
  `occlusionTexture`, `emissiveTexture`) into
  `oxideav_mesh3d::Material::extras["KHR_texture_transform:<slot>"]`
  (slot ∈ `baseColor` / `metallicRoughness` / `normal` / `occlusion` /
  `emissive`) as a JSON `Value::Object`; a bare `{}` resolves to an empty
  record with consumers applying the spec defaults at use time. The
  encoder lifts each slot's transform back into the typed textureInfo
  extensions block and appends `KHR_texture_transform` to
  `extensionsUsed`. The §3.12 stack validator rejects textureInfos
  carrying the data block without the declaration
  (`ExtensionStackUsedNotDeclared`). The transform also passes through
  verbatim when nested inside another extension's textureInfo (e.g.
  `KHR_materials_specular.specularTexture`). New `tests/
  khr_texture_transform.rs` covers GLB round-trip on the baseColor /
  normal / emissive slots, `extensionsUsed` emission, the bare-object
  default, full-field decode (mirroring the spec's lower-left-quadrant
  90° example), and the §3.12 rejection path.

### Added (round 129)

- `KHR_materials_iridescence` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_iridescence.md`). Decoder reads
  `materials[i].extensions.KHR_materials_iridescence` and surfaces the
  full extension object through
  `oxideav_mesh3d::Material::extras["KHR_materials_iridescence"]` as a
  JSON `Value::Object` carrying any of the six spec-defined keys
  (`iridescenceFactor`, `iridescenceTexture`, `iridescenceIor`,
  `iridescenceThicknessMinimum`, `iridescenceThicknessMaximum`,
  `iridescenceThicknessTexture`) — a bare `{}` extension object resolves
  to the spec defaults `iridescenceFactor = 0.0` (a zero factor disables
  the whole iridescence layer per §Properties), `iridescenceIor = 1.3`,
  `iridescenceThicknessMinimum = 100.0`, `iridescenceThicknessMaximum =
  400.0` (all in nanometres). The spec explicitly allows
  `iridescenceThicknessMinimum > iridescenceThicknessMaximum`; the
  decoder passes inverted ranges through unmodified. `iridescenceTexture`
  / `iridescenceThicknessTexture` are `textureInfo` (round-trip `index`
  + optional `texCoord` preserved). Encoder lifts the object back into
  the typed JSON extension block and appends `KHR_materials_iridescence`
  to `extensionsUsed`. The §3.12 stack validator rejects materials
  carrying the data block without the declaration with
  `ExtensionStackUsedNotDeclared`. JSON model gains `MaterialIridescence`
  and a `MaterialExtensions.khr_materials_iridescence` field. Tests: 10
  integration (`khr_materials_iridescence.rs`) + 2 unit
  (`validation::tests`).

### Added (round 126)

- cargo-fuzz harness `fuzz/fuzz_targets/parse.rs`. Drives arbitrary
  attacker bytes through `GltfDecoder::decode` (magic-sniff +
  JSON-or-GLB dispatcher) and `glb::parse` (chunk walker) under
  libfuzzer-sys with AddressSanitizer. The contract under test is
  panic-freedom: every reachable parser path returns a `Result` for
  any input — chunk-length overflow, mismatched accessor count /
  componentType, buffer-view stride arithmetic, extension dispatch on
  unknown names, GLB header / chunk-alignment violations all surface
  as `Err`, never panic. Local soak (2 jobs, 124 s, ~13 k exec/s)
  reached 3.1 M iterations / coverage 1790 with `oom/timeout/crash:
  0/0/0`; no decoder changes were required. Round-7 validators
  (`check_json_byte_length`, `check_json_depth`,
  `validate_accessor_fits_bufferview`,
  `validate_bufferview_fits_buffer`) carry the panic-freedom invariant
  the harness re-verifies on attacker input.

### Added (round 120)

- `KHR_materials_volume` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_volume.md`). Decoder reads
  `materials[i].extensions.KHR_materials_volume` and surfaces the full
  extension object through
  `oxideav_mesh3d::Material::extras["KHR_materials_volume"]` as a JSON
  `Value::Object` carrying any of the four spec-defined keys
  (`thicknessFactor`, `thicknessTexture`, `attenuationDistance`,
  `attenuationColor`) — a bare `{}` extension object resolves to the
  spec defaults `thicknessFactor = 0.0` (thin-walled) and
  `attenuationColor = [1, 1, 1]`. `attenuationDistance` defaults to
  `+Infinity` per §Properties; JSON cannot encode non-finite numbers
  so the decoder leaves the key absent and consumers interpret
  missing-key as the +Infinity default. `thicknessTexture` is a
  `textureInfo` (round-trip `index` + optional `texCoord` preserved).
  Encoder lifts the object back into the typed JSON extension block
  and appends `KHR_materials_volume` to `extensionsUsed`. The §3.12
  stack validator rejects materials carrying the data block without
  the declaration with `ExtensionStackUsedNotDeclared`. JSON model
  gains `MaterialVolume` and a `MaterialExtensions.khr_materials_volume`
  field. Tests: 9 integration (`khr_materials_volume.rs`) + 2 unit
  (`validation::tests`).

### Added (round 114)

- `KHR_materials_sheen` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_sheen.md`). Decoder reads
  `materials[i].extensions.KHR_materials_sheen` and surfaces the full
  extension object through
  `oxideav_mesh3d::Material::extras["KHR_materials_sheen"]` as a JSON
  `Value::Object` carrying any of the four spec-defined keys
  (`sheenColorFactor`, `sheenColorTexture`, `sheenRoughnessFactor`,
  `sheenRoughnessTexture`) — a bare `{}` extension object resolves to the
  spec defaults `sheenColorFactor = [0, 0, 0]`, `sheenRoughnessFactor =
  0.0` (§Extending Materials §Sheen; the spec notes a zero
  `sheenColorFactor` disables the whole sheen layer). `sheenColorTexture`
  / `sheenRoughnessTexture` are `textureInfo` (round-trip `index` +
  optional `texCoord`). Encoder lifts the object back into the typed JSON
  extension block and appends `KHR_materials_sheen` to `extensionsUsed`.
  The §3.12 stack validator rejects materials carrying the data block
  without the declaration with `ExtensionStackUsedNotDeclared`. JSON
  model gains `MaterialSheen` and a `MaterialExtensions.khr_materials_sheen`
  field. Tests: 7 integration (`khr_materials_sheen.rs`) + 2 unit
  (`validation::tests`).

### Added (round 110)

- `KHR_materials_clearcoat` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_clearcoat.md`). Decoder reads
  `materials[i].extensions.KHR_materials_clearcoat` and surfaces the full
  extension object through
  `oxideav_mesh3d::Material::extras["KHR_materials_clearcoat"]` as a JSON
  `Value::Object` carrying any of the five spec-defined keys
  (`clearcoatFactor`, `clearcoatTexture`, `clearcoatRoughnessFactor`,
  `clearcoatRoughnessTexture`, `clearcoatNormalTexture`) — a bare `{}`
  extension object resolves to the spec defaults `clearcoatFactor = 0.0`,
  `clearcoatRoughnessFactor = 0.0` (§Extending Materials §Clearcoat; the
  spec notes a zero `clearcoatFactor` disables the whole clearcoat
  layer). `clearcoatTexture` / `clearcoatRoughnessTexture` are
  `textureInfo` (round-trip `index` + optional `texCoord`);
  `clearcoatNormalTexture` is a `normalTextureInfo`, so it additionally
  round-trips an optional `scale`. Encoder lifts the object back into the
  typed JSON extension block and appends `KHR_materials_clearcoat` to
  `extensionsUsed`. The §3.12 stack validator rejects materials carrying
  the data block without the declaration with
  `ExtensionStackUsedNotDeclared`. JSON model gains `MaterialClearcoat`
  and a `MaterialExtensions.khr_materials_clearcoat` field. Tests: 7
  integration (`khr_materials_clearcoat.rs`) + 2 unit
  (`validation::tests`).

### Added (round 105)

- `KHR_materials_specular` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_specular.md`). Decoder reads
  `materials[i].extensions.KHR_materials_specular` and surfaces the full
  extension object through
  `oxideav_mesh3d::Material::extras["KHR_materials_specular"]` as a JSON
  `Value::Object` carrying any of the four spec-defined keys
  (`specularFactor`, `specularTexture`, `specularColorFactor`,
  `specularColorTexture`) — a bare `{}` extension object resolves to the
  spec defaults `specularFactor = 1.0`, `specularColorFactor = [1, 1, 1]`
  (§Extending Materials). The spec explicitly allows
  `specularColorFactor` components above `1.0`, so we pass them through
  unclamped (clamping is a render-time concern per the Implementation
  §, not a decode-time one). `specularTexture` / `specularColorTexture`
  TextureInfo round-trips preserve both `index` and optional `texCoord`.
  Encoder lifts the object back into the typed JSON extension block and
  appends `KHR_materials_specular` to `extensionsUsed`. The §3.12 stack
  validator rejects materials carrying the data block without the
  declaration with `ExtensionStackUsedNotDeclared`. JSON model gains
  `MaterialSpecular` and a `MaterialExtensions.khr_materials_specular`
  field. Tests: 7 integration (`khr_materials_specular.rs`) + 2 unit
  (`validation::tests`).

### Added (round 102)

- `KHR_materials_ior` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_ior.md`). Decoder reads
  `materials[i].extensions.KHR_materials_ior.ior` and surfaces the
  scalar through `oxideav_mesh3d::Material::extras["KHR_materials_ior"]`
  as a JSON number — a bare `{}` extension object resolves to the spec
  default of `1.5` (§Extending Materials). The `ior == 0`
  specular-glossiness backwards-compatibility sentinel is carried
  through verbatim, not coerced. Encoder lifts the value back into the
  JSON extension object and appends `KHR_materials_ior` to
  `extensionsUsed`. The §3.12 stack validator rejects materials carrying
  the data block without the declaration with
  `ExtensionStackUsedNotDeclared`. JSON model gains `MaterialIor` and a
  `MaterialExtensions.khr_materials_ior` field. Tests: 7 integration
  (`khr_materials_ior.rs`) + 2 unit (`validation::tests`).

### Added (round 98)

- `KHR_materials_emissive_strength` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_emissive_strength.md`). Decoder
  reads `materials[i].extensions.KHR_materials_emissive_strength
  .emissiveStrength` and surfaces the scalar through
  `oxideav_mesh3d::Material::extras["KHR_materials_emissive_strength"]`
  as a JSON number — a bare `{}` extension object resolves to the
  spec default of `1.0` (§Parameters). Encoder lifts the value back into
  the JSON extension object and appends
  `KHR_materials_emissive_strength` to `extensionsUsed`. The §3.12 stack
  validator rejects materials carrying the data block without the
  declaration with `ExtensionStackUsedNotDeclared`. JSON model gains
  `MaterialEmissiveStrength` and a `MaterialExtensions
  .khr_materials_emissive_strength` field. Tests: 6 integration
  (`khr_materials_emissive_strength.rs`) + 2 unit (`validation::tests`).

### Added (round 93)

- `KHR_materials_unlit` extension (Khronos ratified — see
  `docs/3d/gltf/extensions/KHR_materials_unlit.md`). Decoder reads
  `materials[i].extensions.KHR_materials_unlit` and surfaces the flag
  through `oxideav_mesh3d::Material::extras["KHR_materials_unlit"] =
  Bool(true)`; encoder lifts the flag back into the JSON extension
  object (literal `{}`) and appends `KHR_materials_unlit` to
  `extensionsUsed`. The §3.12 stack validator rejects materials
  carrying the data block without the declaration with
  `ExtensionStackUsedNotDeclared`. JSON model gains
  `MaterialExtensions` + `MaterialUnlit` and a `Material.extensions`
  field. Tests: 5 integration (`khr_materials_unlit.rs`) + 2 unit
  (`validation::tests`).

### Added (round 8)

- Accessor-fit-in-bufferView validation per glTF 2.0 §3.6.2.4 line
  3104. The decoder now applies the spec's bound
  `accessor.byteOffset + EFFECTIVE_BYTE_STRIDE * (count - 1) +
  SIZE_OF_COMPONENT * NUMBER_OF_COMPONENTS <= bufferView.byteLength`
  to every accessor that references a bufferView, covering both
  tightly-packed and strided layouts. Failures surface as
  `Error::InvalidData` with stable prefixes:
  `AccessorFitBufferView` (overrun), `AccessorFitStride` (stride
  smaller than element), `AccessorFitComponentType` (unknown
  componentType), `AccessorFitElementType` (unknown `type`),
  `AccessorFitOverflow` (offset arithmetic overflowed u64).
- BufferView-fit-in-buffer validation per glTF 2.0 §5.11. The
  decoder now rejects `bufferView.byteOffset + byteLength >
  buffer.byteLength` with `BufferViewFitBuffer`, and rejects
  `bufferView.byteStride` outside the JSON-schema range `[4, 252]`
  (§5.11.4) with `BufferViewStrideRange`.
- Sparse-indices bufferView restriction validation per glTF 2.0
  §5.3.1. The decoder now rejects an `accessor.sparse.indices.bufferView`
  that carries a `target` (`SparseIndicesBufferViewTarget`) or a
  `byteStride` (`SparseIndicesBufferViewStride`) property; out-of-range
  bufferView indices surface as `SparseIndicesBufferViewIndex`.

### Added (round 7)

- Extension-stack consistency validation per glTF 2.0 §3.12. The
  decoder now rejects documents whose `extensionsRequired` is not a
  subset of `extensionsUsed`
  (`ExtensionStackRequiredNotListed`-prefixed `Error::InvalidData`)
  and documents that carry a `KHR_lights_punctual` data block (either
  at root scope or on a node) without listing the extension in
  `extensionsUsed` (`ExtensionStackUsedNotDeclared`).
- Animation channel target-path validation per glTF 2.0 §3.11. Each
  channel's `target.path` must be one of `"translation"` /
  `"rotation"` / `"scale"` / `"weights"`
  (`AnimationChannelPath`); the sampler index plus sampler.input /
  sampler.output accessor indices must be in range
  (`AnimationChannelSampler` / `AnimationChannelSamplerInput` /
  `AnimationChannelSamplerOutput`); and a `path == "weights"` channel
  MUST point at a node bound to a mesh whose primitives declare at
  least one morph target (`AnimationChannelWeightsNoMesh` /
  `AnimationChannelWeightsNoTargets`).
- Decoder fuzz hardening — two pre-serde checks bound the JSON
  payload before the recursive parser runs:
  - `validation::check_json_byte_length` rejects payloads larger
    than `MAX_JSON_BYTES` (128 MiB) with a `JsonTooLarge` prefix —
    binary buffers live in the BIN chunk, so the cap only applies to
    the textual JSON document.
  - `validation::check_json_depth` rejects payloads nesting deeper
    than `MAX_JSON_DEPTH` (256 levels) with a `JsonDepthExceeded`
    prefix. Linear-time scan that tracks `{`/`[` open + `}`/`]`
    close while respecting JSON string + escape syntax (a `[`
    inside `"..."` doesn't count). Defends against malicious
    1000-deep-array bombs that crash the recursive serde_json
    parser on stack overflow.
- Encoder also emits typed `Primitive.targets` (mesh3d ≥ 0.0.3)
  alongside the existing `__morph_targets` extras sentinel. Typed
  morph targets take precedence when both are present; the sentinel
  path stays for round-2 backwards compatibility.

## [0.0.1](https://github.com/OxideAV/oxideav-gltf/compare/v0.0.0...v0.0.1) - 2026-05-10

### Other

- Validate vertex-attribute data per spec §3.6.2.4 + §3.7.2.1 (r6)
- Sparse-encode mesh vertex attribute accessors (r5 item b)
- Sparse-encode skin.inverseBindMatrices accessors (r5 item a)
- Add encoder-side signed normalised-int animation outputs (r5 item c)
- Validate accessor min/max bounds per spec §3.6.2.1.5 (r4 item c)
- Add morph targets round-trip per spec §3.7.2.2 (r4 item b)
- Add encoder-side normalised-int animation outputs (r4 item a)
- Add sparse-encoding heuristic + normalised-int animation decode (r3)
- Add skins, animations, sparse accessors, multi-scene round-trip (r2)

### Added (round 6)

- Vertex-attribute compression validation per glTF 2.0 §3.6.2.4
  (data alignment) + §3.7.2.1 (semantic constraints). The decoder now
  rejects spec-non-conformant attribute layouts up-front with a stable
  `VertexAttribute…`-prefixed `Error::InvalidData` message. Six MUSTs
  enforced:
  - `accessor.byteOffset` MUST be a multiple of the component size
    (`VertexAttributeAlignment`);
  - vertex-attribute `accessor.byteOffset` and the optional
    `bufferView.byteStride` MUST also be multiples of 4
    (`VertexAttributeAlignment`);
  - all attribute accessors of one primitive MUST share `count`
    (`VertexAttributeCount`);
  - indices accessor MUST NOT contain the primitive-restart sentinel
    (255 / 65535 / 4294967295) for its component type
    (`VertexAttributeIndexRestart`);
  - TANGENT.w MUST be exactly ±1.0 (`VertexAttributeTangentW`);
  - all components of every COLOR_0 element MUST be in `[0.0, 1.0]`
    (`VertexAttributeColor0Range`).
- `crate::validation` module exposes the individual validators as
  reusable helpers (`validate_alignment`, `validate_attribute_counts`,
  `validate_index_no_restart`, `validate_tangent_w`,
  `validate_color0_range`) with their own unit tests.

### Changed (round 6)

- TANGENT no longer participates in the sparse-encoding heuristic.
  Spec §3.7.2.1 mandates `TANGENT.w == ±1.0`, so a zero-base sparse
  block (which would synthesise w=0 for every non-overridden slot) is
  inherently spec-non-conformant. The encoder now keeps TANGENT dense
  regardless of the sparse threshold, undoing one corner of r5 item b.

### Added (round 5)

- Sparse-encoding heuristic extended to mesh vertex-attribute
  accessors (POSITION / NORMAL / TANGENT / COLOR_n / WEIGHTS_0) per
  glTF 2.0 §3.6.2.3. The same threshold set via
  `GltfEncoder::with_sparse_threshold(f32)` now also gates these
  attributes: an element counts as "zero" iff every one of its
  components is exactly 0.0. POSITION accessors keep their
  spec-mandated min/max even on the sparse path (computed from the
  post-overlay data, which is identical to the dense data because
  the decoder applies overrides over the zero base before the bounds
  check). New `push_vec4_accessor_maybe_sparse` helper backs
  TANGENT / COLOR_n / WEIGHTS_0; POSITION + NORMAL re-use the
  existing `push_vec3_accessor_maybe_sparse` from r3.
- Sparse-encoding heuristic extended to `skin.inverseBindMatrices`
  (MAT4 FLOAT) accessors per glTF 2.0 §3.6.2.3. The same threshold
  gates IBM accessors: an IBM matrix counts as "zero" iff every one
  of its 16 components is exactly 0.0; when the all-zero fraction
  crosses the threshold the accessor is re-emitted as zero-base
  sparse with per-index overrides for the non-zero matrices.
  Heavily-symmetric rigs that carry placeholder zero matrices for
  unused joint slots shrink roughly proportionally to the zero
  fraction.
- Encoder-side signed normalised-int animation outputs — symmetric to
  r3 decode (which already accepts BYTE / SHORT). New `QuantizeMode`
  variants: `IByte` (5120 normalized; `f` × 127 with `-128` reserved
  per spec §3.6.2.2) and `IShort` (5122 normalized; `f` × 32767 with
  `-32768` reserved). Useful for rotation quaternions where the
  components span `[-1, 1]` and the unsigned modes would clamp every
  negative component to 0. Round-trip tolerance: `1/127` for IByte,
  `1/32767` for IShort.

### Added (round 4)

- Encoder-side normalised-int animation outputs — symmetric to r3
  decode. `GltfEncoder::with_quantize_animation(QuantizeMode)` selects
  the component type for ROTATION (VEC4) + MORPH_WEIGHTS (SCALAR)
  sampler outputs: `Float` (default, lossless), `UByte` (5121
  normalized, ×255), or `UShort` (5123 normalized, ×65535) per spec
  §3.6.2.2 dequantisation. TRANSLATION + SCALE remain FLOAT-only.
- Morph targets per spec §3.7.2.2 — `mesh.primitives[i].targets[t]`
  POSITION / NORMAL / TANGENT delta accessors decode + encode. The
  typed `oxideav_mesh3d::Primitive` model has no dedicated `targets`
  field yet (cross-crate change deferred to r5), so deltas round-trip
  via the `primitive.extras["__morph_targets"]` sentinel (and
  `mesh.weights` via `primitive[0].extras["__mesh_weights"]`) — same
  pattern as `__mesh_extras` from r2.
- Accessor `min` / `max` bounds validation per spec §3.6.2.1.5. The
  encoder fills missing POSITION min/max from the data (already true
  in earlier rounds, now also applied to morph-target POSITION
  deltas); the decoder validates declared bounds on VEC3 attribute
  accessors and surfaces a mismatch via an `AccessorBoundsMismatch`
  prefix on the `Error::InvalidData` message. (The shared
  `oxideav_core::Error` enum can't gain a new variant from a sibling
  crate; the prefix lets callers grep for the condition without an
  enum check — r5 followup is the typed variant.)

### Added (round 3)

- Sparse-encoding heuristic on `GltfEncoder` — opt in via
  `GltfEncoder::with_sparse_threshold(f32)`. FLOAT animation outputs
  whose zero-element fraction meets the threshold are re-emitted as
  zero-base + `accessor.sparse` overrides per glTF 2.0 §3.6.2.3.
  Applies to TRANSLATION (VEC3) and MORPH_WEIGHTS (SCALAR) outputs;
  ROTATION (VEC4) and SCALE (VEC3) stay dense because their semantic
  identity (`[0,0,0,1]` / `[1,1,1]`) isn't all-zero.
- Normalised-integer animation output accessors decode — ROTATION
  (VEC4) and MORPH_WEIGHTS (SCALAR) sampler outputs may carry
  `componentType` BYTE / UBYTE / SHORT / USHORT with `normalized: true`
  and are dequantised via the §3.6.2.2 equations
  (`f = max(c/127, -1)` / `f = c/255` / `f = max(c/32767, -1)` /
  `f = c/65535`). TRANSLATION + SCALE remain FLOAT-only per spec.
- New encoder knob: `EncodeOptions { sparse_threshold }` plus the
  helper `convert_with_options(scene, &opts)` next to the existing
  `convert(scene)`.

### Added (round 2)

- Skins + skeletons (`skins[]`, `inverseBindMatrices` accessor, joint
  roster, optional `skeleton` root node) per glTF 2.0 §3.7.3.
- Animations (`animations[]` with channels + samplers) per §3.11 —
  TRANSLATION / ROTATION / SCALE / WEIGHTS target paths, LINEAR +
  STEP + CUBICSPLINE interpolation modes.
- Sparse accessors (`accessor.sparse`) per §3.6.2.3 — decoded by
  materialising the base buffer and overlaying the per-index value
  overrides; the encoder emits dense storage.
- Multi-scene documents — secondary `scenes[]` preserved through
  round-trip via `Scene3D::extras["__additional_scenes"]`, with the
  active scene index honoured on both decode and encode.
- New accessor helpers: `materialise_accessor`, `read_mat4_f32`,
  `write_mat4_f32`, `read_sparse_indices`.

### Added (round 1)

- Initial release: pure-Rust glTF 2.0 codec implementing
  `oxideav_mesh3d::Mesh3DDecoder` + `oxideav_mesh3d::Mesh3DEncoder`.
- `.gltf` JSON read + write (full PBR material model, multi-primitive
  meshes, perspective + orthographic cameras, KHR_lights_punctual
  punctual-light extension, `extras` round-trip).
- `.glb` binary container read + write (12-byte header + JSON chunk +
  optional BIN chunk per Khronos §4.4).
- `BufferViewAsset`: `oxideav_mesh3d::AssetSource` impl that lazily
  reads image bytes out of a `.glb` BIN chunk by `(offset, length)`
  without copying the entire chunk.
- Format detection on the first 4 bytes (`b"glTF"` magic → binary,
  otherwise JSON).
- Default-on `registry` Cargo feature wires the decoder + encoder
  factories into `oxideav_mesh3d::Mesh3DRegistry`. `--no-default-features`
  builds against the standalone `oxideav-mesh3d` core only.
