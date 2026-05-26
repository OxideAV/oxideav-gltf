# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
