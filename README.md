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
- `extras` round-trip on root, scenes, nodes, materials, primitives

## Round 6 (planned)

- KHR_audio_emitter wiring against `oxideav_mesh3d::AudioSource` /
  `AudioEmitter` (blocked on docs/3d/gltf/extensions/ entries)
- Material PBR-extension surfaces: KHR_materials_ior,
  _emissive_strength, _clearcoat, _sheen, _transmission
  (blocked on docs/3d/gltf/extensions/ entries)
- KHR_texture_transform UV transform on texture references
- Migrate morph targets to typed `Primitive.targets` field once
  `oxideav-mesh3d` 0.0.3+ ships (currently uses the
  `__morph_targets` extras sentinel)

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
