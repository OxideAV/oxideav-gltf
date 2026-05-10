//! [`GltfDecoder`] — sniffs `b"glTF"` magic and dispatches to the
//! `.glb` binary path or the JSON parser.

use oxideav_mesh3d::{Mesh3DDecoder, Scene3D};

use crate::error::{invalid, Result};
use crate::glb;
use crate::json_model::GltfRoot;
use crate::json_to_scene;

/// Decode `.gltf` (UTF-8 JSON) or `.glb` (binary container) bytes
/// into a [`Scene3D`].
#[derive(Debug, Default)]
pub struct GltfDecoder {
    _priv: (),
}

impl GltfDecoder {
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl Mesh3DDecoder for GltfDecoder {
    fn decode(&mut self, bytes: &[u8]) -> Result<Scene3D> {
        // Format detection: first 4 bytes are b"glTF" → binary.
        if bytes.len() >= 4 && &bytes[..4] == b"glTF" {
            let payload = glb::parse(bytes)?;
            let root: GltfRoot = serde_json::from_slice(payload.json)
                .map_err(|e| invalid(format!("glb: JSON chunk parse error: {e}")))?;
            return json_to_scene::convert(&root, payload.bin);
        }
        // Otherwise assume JSON.
        let root: GltfRoot = serde_json::from_slice(bytes)
            .map_err(|e| invalid(format!("gltf: JSON parse error: {e}")))?;
        json_to_scene::convert(&root, None)
    }
}
