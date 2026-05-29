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
- KHR_mesh_quantization decode (Khronos ratified) — quantized vertex
  attributes from `docs/3d/gltf/extensions/KHR_mesh_quantization.md`.
  `POSITION` / `NORMAL` / `TANGENT` / `TEXCOORD_n` accessors may store
  8-/16-bit signed/unsigned integers (normalized or unnormalized) in
  place of `FLOAT`. The decoder dequantizes per the spec int→float
  table — BYTE `f = max(c/127, -1)`, UNSIGNED_BYTE `f = c/255`, SHORT
  `f = max(c/32767, -1)`, UNSIGNED_SHORT `f = c/65535`; unnormalized
  integers cast directly to `f32`. A quantized base attribute is gated
  on `KHR_mesh_quantization` being declared in `extensionsUsed` and the
  (componentType, normalized) pair being in the extension's allowed set
  for that attribute; the base-spec UNSIGNED_BYTE / UNSIGNED_SHORT
  *normalized* `TEXCOORD` types stay accepted without the extension.
  Each quantized attribute's storage form (componentType + normalized)
  is recorded under the primitive `extras["__attr_quant"]` sentinel for
  a future encoder round-trip. Encoder emission of quantized attributes
  is not yet wired (see roadmap)
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

- KHR_mesh_quantization encoder — the decode path (dequantisation of
  int8/int16 POSITION / NORMAL / TANGENT / TEXCOORD) landed in round
  188; the remaining work is the float→int emit path (re-quantising
  attributes recorded under the `__attr_quant` sentinel and appending
  the extension to `extensionsUsed` + `extensionsRequired`), plus
  decode of quantized morph-target attributes (§Extending Morph Target
  Attributes)
- KHR_audio_emitter wiring against `oxideav_mesh3d::AudioSource` /
  `AudioEmitter`

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
