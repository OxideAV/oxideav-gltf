# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
