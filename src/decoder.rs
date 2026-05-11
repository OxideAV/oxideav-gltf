//! [`GltfDecoder`] — sniffs `b"glTF"` magic and dispatches to the
//! `.glb` binary path or the JSON parser.

use oxideav_mesh3d::{Mesh3DDecoder, Scene3D};

use crate::error::{invalid, Result};
use crate::glb;
use crate::json_model::GltfRoot;
use crate::json_to_scene;
use crate::validation::{check_json_byte_length, check_json_depth};

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
            // Fuzz hardening (round 7): reject pathological JSON
            // (oversized + over-nested) BEFORE serde_json sees it.
            // Both are recursive parsers internally — without these
            // caps, a malicious 1000-deep array bomb crashes the
            // stack and a multi-GB declaration runs the allocator
            // dry.
            check_json_byte_length(payload.json)?;
            check_json_depth(payload.json)?;
            let root: GltfRoot = serde_json::from_slice(payload.json)
                .map_err(|e| invalid(format!("glb: JSON chunk parse error: {e}")))?;
            return json_to_scene::convert(&root, payload.bin);
        }
        // Otherwise assume JSON.
        check_json_byte_length(bytes)?;
        check_json_depth(bytes)?;
        let root: GltfRoot = serde_json::from_slice(bytes)
            .map_err(|e| invalid(format!("gltf: JSON parse error: {e}")))?;
        json_to_scene::convert(&root, None)
    }
}
