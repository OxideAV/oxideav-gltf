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
use crate::scene_to_json::{convert, EncodedScene};

/// Container flavour for [`GltfEncoder::encode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputFlavour {
    /// `.glb` — self-contained binary container, JSON chunk + BIN chunk.
    #[default]
    Glb,
    /// `.gltf` — JSON document; binary buffer inlined as a base64 `data:` URI.
    JsonEmbedded,
}

/// Serialise a [`Scene3D`] into glTF bytes.
#[derive(Debug, Default)]
pub struct GltfEncoder {
    pub output: OutputFlavour,
}

impl GltfEncoder {
    pub fn new() -> Self {
        Self {
            output: OutputFlavour::Glb,
        }
    }

    pub fn with_output(output: OutputFlavour) -> Self {
        Self { output }
    }
}

impl Mesh3DEncoder for GltfEncoder {
    fn encode(&mut self, scene: &Scene3D) -> Result<Vec<u8>> {
        let EncodedScene { mut root, bin } = convert(scene)?;
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
