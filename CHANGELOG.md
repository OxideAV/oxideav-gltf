# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2](https://github.com/OxideAV/oxideav-gltf/compare/v0.0.1...v0.0.2) - 2026-05-20

### Other

- round 8: accessor/bufferView fit + sparse-indices restriction validation
- Round 75: GLB hardening + JSON-to-scene validation extension
- Validate extension stack + animation paths; harden JSON parser (r7)

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
