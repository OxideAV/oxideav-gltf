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
- Skins + skeletons (joint roster, inverseBindMatrices accessor,
  optional skeleton root) per spec §3.7.3 — `node.skin` round-trips
- Animations (channels + samplers) per spec §3.11 — translation /
  rotation / scale / weights paths, LINEAR + STEP + CUBICSPLINE
  interpolation
- Sparse accessors per spec §3.6.2.3 (decode-only — encoder emits
  dense storage)
- Multi-scene documents — secondary `scenes[]` are preserved through
  round-trip via `Scene3D::extras["__additional_scenes"]`; the active
  scene index is honoured on both decode and encode
- Textures with samplers + images (buffer-view-backed images via
  `BufferViewAsset` for zero-copy slicing into the `.glb` BIN chunk;
  `data:` URI base64 inlining; external URI passthrough)
- `extras` round-trip on root, scenes, nodes, materials, primitives

## Round 3 (planned)

- KHR_audio_emitter wiring against `oxideav_mesh3d::AudioSource` /
  `AudioEmitter` (blocked on docs/3d/gltf/extensions/ entries)
- Material PBR-extension surfaces: KHR_materials_ior,
  _emissive_strength, _clearcoat, _sheen, _transmission
  (blocked on docs/3d/gltf/extensions/ entries)
- KHR_texture_transform UV transform on texture references
- Sparse-encoding heuristic — auto-detect when a re-emitted accessor
  would benefit from sparse storage

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

## License

[MIT](LICENSE)
