//! [`GltfEncoder`] — emits `.glb` (binary container, default) or
//! `.gltf` (JSON + base64 buffer/data URI) bytes from a [`Scene3D`].
//!
//! Output flavour is controlled by [`GltfEncoder::with_output`]:
//!
//! * [`OutputFlavour::Glb`] (default) packs everything into one
//!   self-contained `.glb` file (single buffer → BIN chunk).
//! * [`OutputFlavour::JsonEmbedded`] emits a `.gltf` JSON document
//!   with the binary buffer inlined as a `data:application/octet-stream;base64,...`
//!   URI. Output is one stand-alone file.

use oxideav_mesh3d::{Mesh3DEncoder, Scene3D};

use crate::error::{invalid, Result};
use crate::glb;
use crate::scene_to_json::{convert_with_options, EncodeOptions, EncodedScene};

/// Container flavour for [`GltfEncoder::encode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputFlavour {
    /// `.glb` — self-contained binary container, JSON chunk + BIN chunk.
    #[default]
    Glb,
    /// `.gltf` — JSON document; binary buffer inlined as a base64 `data:` URI.
    JsonEmbedded,
}

/// Quantisation mode for animation output accessors that the spec
/// permits in normalised-integer form (ROTATION VEC4, MORPH_WEIGHTS
/// SCALAR — see glTF 2.0 §3.11 + §3.6.2.2).
///
/// `Float` is the lossless default. The unsigned modes `UByte` /
/// `UShort` encode each f32 to nearest u8 / u16 (×255 / ×65535,
/// clamped to `[0, 1]`) with `normalized: true`. The signed modes
/// `IByte` / `IShort` encode each f32 in `[-1, 1]` to i8 / i16 in
/// `[-127, 127]` / `[-32767, 32767]` (the `-128` / `-32768` slots
/// stay reserved per spec §3.6.2.2 so the dequantised range stays
/// symmetric: `f = max(c / 127, -1)` / `f = max(c / 32767, -1)`).
/// Use the signed modes for true signed-range data such as rotation
/// quaternions; the unsigned modes clamp negative inputs to 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QuantizeMode {
    /// Emit FLOAT (5126) — the default, lossless.
    #[default]
    Float,
    /// Emit UNSIGNED_BYTE (5121) `normalized: true` — values × 255.
    UByte,
    /// Emit UNSIGNED_SHORT (5123) `normalized: true` — values × 65535.
    UShort,
    /// Emit BYTE (5120) `normalized: true` — values × 127, clamp range
    /// `[-127, 127]` (`-128` stays reserved per spec §3.6.2.2).
    IByte,
    /// Emit SHORT (5122) `normalized: true` — values × 32767, clamp
    /// range `[-32767, 32767]` (`-32768` stays reserved).
    IShort,
}

/// Serialise a [`Scene3D`] into glTF bytes.
#[derive(Debug, Default)]
pub struct GltfEncoder {
    pub output: OutputFlavour,
    /// When set, FLOAT vec/scalar accessors whose zero-element fraction
    /// is at least this value (in `[0.0, 1.0]`) are emitted using
    /// `accessor.sparse` storage (no base bufferView; the decoder
    /// initialises to zero and overlays the indices+values pairs) per
    /// glTF 2.0 §3.6.2.3.
    ///
    /// Tune via [`GltfEncoder::with_sparse_threshold`]. `None` (the
    /// default) emits dense storage unconditionally — matches r2
    /// behaviour.
    pub sparse_threshold: Option<f32>,
    /// Quantisation mode for ROTATION + MORPH_WEIGHTS animation
    /// sampler outputs. `Float` (default) emits FLOAT (5126); the
    /// other modes pick a normalised-int component type per spec §3.11.
    pub quantize_animation: QuantizeMode,
    /// When `true`, index bufferViews are post-compressed with
    /// `KHR_meshopt_compression` (INDICES mode); the uncompressed data
    /// stays in a fallback buffer and the document declares the
    /// extension as required. Most useful with
    /// [`OutputFlavour::JsonEmbedded`]. Enable via
    /// [`GltfEncoder::with_meshopt_compression`]. `false` by default.
    pub meshopt_compress_indices: bool,
}

impl GltfEncoder {
    pub fn new() -> Self {
        Self {
            output: OutputFlavour::Glb,
            sparse_threshold: None,
            quantize_animation: QuantizeMode::Float,
            meshopt_compress_indices: false,
        }
    }

    pub fn with_output(output: OutputFlavour) -> Self {
        Self {
            output,
            sparse_threshold: None,
            quantize_animation: QuantizeMode::Float,
            meshopt_compress_indices: false,
        }
    }

    /// Enable the sparse-encoding heuristic. `threshold` is the
    /// fraction of base-value (zero) entries above which a FLOAT
    /// accessor is emitted using `accessor.sparse` storage. A value
    /// of `0.5` is a sensible default; accessors where more than half
    /// the entries are zero almost always shrink under sparse
    /// encoding. `threshold` is clamped to `[0.0, 1.0]`. `0.0` means
    /// "always sparse"; `1.0` means "only when every entry is zero".
    pub fn with_sparse_threshold(mut self, threshold: f32) -> Self {
        self.sparse_threshold = Some(threshold.clamp(0.0, 1.0));
        self
    }

    /// Pick a quantisation mode for ROTATION (VEC4) and MORPH_WEIGHTS
    /// (SCALAR) animation sampler outputs. See [`QuantizeMode`].
    /// Sparse storage takes precedence: when an output also satisfies
    /// the sparse threshold, the encoder still emits FLOAT sparse
    /// (mixing quantisation with sparse-base-zero would lose the f32
    /// rest values for non-zero overrides).
    pub fn with_quantize_animation(mut self, mode: QuantizeMode) -> Self {
        self.quantize_animation = mode;
        self
    }

    /// Enable `KHR_meshopt_compression` of eligible bufferViews on
    /// write: index views (INDICES mode, `u16`/`u32`) and dense vertex
    /// attribute views (ATTRIBUTES mode, element stride a multiple of 4
    /// in `[4, 256]`). The uncompressed bytes stay in the packed BIN
    /// buffer and the compressed payloads go in a separate `data:`-URI
    /// buffer, so the document is self-contained, stays readable without
    /// the extension, and round-trips through this crate's decoder
    /// (which inflates the descriptors back to the uncompressed data).
    pub fn with_meshopt_compression(mut self, enable: bool) -> Self {
        self.meshopt_compress_indices = enable;
        self
    }
}

impl Mesh3DEncoder for GltfEncoder {
    fn encode(&mut self, scene: &Scene3D) -> Result<Vec<u8>> {
        let opts = EncodeOptions {
            sparse_threshold: self.sparse_threshold,
            quantize_animation: self.quantize_animation,
            meshopt_compress_indices: self.meshopt_compress_indices,
        };
        let EncodedScene { mut root, bin } = convert_with_options(scene, &opts)?;
        match self.output {
            OutputFlavour::Glb => {
                // Buffer 0 has no `uri` — its bytes ARE the BIN chunk.
                let json = serde_json::to_vec(&root)
                    .map_err(|e| invalid(format!("gltf encode JSON: {e}")))?;
                let bin_opt = if bin.is_empty() {
                    None
                } else {
                    Some(bin.as_slice())
                };
                Ok(glb::encode(&json, bin_opt))
            }
            OutputFlavour::JsonEmbedded => {
                if !bin.is_empty() {
                    use base64::Engine as _;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
                    if let Some(buf) = root.buffers.first_mut() {
                        buf.uri = Some(format!("data:application/octet-stream;base64,{b64}"));
                    }
                }
                serde_json::to_vec_pretty(&root)
                    .map_err(|e| invalid(format!("gltf encode JSON: {e}")))
            }
        }
    }
}

/// Construct a JSON-flavour [`GltfEncoder`] — convenience for tests
/// and explicit `.gltf` callers.
pub fn json_encoder() -> GltfEncoder {
    GltfEncoder::with_output(OutputFlavour::JsonEmbedded)
}
