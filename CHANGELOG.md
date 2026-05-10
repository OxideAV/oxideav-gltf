# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
