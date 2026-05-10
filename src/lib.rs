//! Pure-Rust glTF 2.0 + `.glb` codec — implements
//! [`oxideav_mesh3d::Mesh3DDecoder`] and
//! [`oxideav_mesh3d::Mesh3DEncoder`] for the JSON and binary
//! container variants.
//!
//! ## Quick start
//!
//! ```no_run
//! use oxideav_gltf::{GltfDecoder, GltfEncoder};
//! use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder};
//!
//! let bytes = std::fs::read("scene.glb").unwrap();
//! let mut dec = GltfDecoder::new();
//! let scene = dec.decode(&bytes).unwrap();
//!
//! let mut enc = GltfEncoder::new(); // .glb by default
//! let out = enc.encode(&scene).unwrap();
//! std::fs::write("roundtrip.glb", out).unwrap();
//! ```
//!
//! ## Format detection
//!
//! [`GltfDecoder::decode`](GltfDecoder) sniffs the first 4 bytes:
//! `b"glTF"` (0x46 0x54 0x6C 0x67) → `.glb` binary container,
//! anything else is parsed as JSON.
//!
//! ## Standalone build
//!
//! `oxideav-core` is gated behind the default-on `registry` cargo
//! feature. Drop the framework dependency with:
//!
//! ```toml
//! oxideav-gltf = { version = "0.0", default-features = false }
//! ```
//!
//! The decoder, encoder, and all helper types remain available; only
//! [`register`] (the framework glue) is feature-gated.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod accessor;
pub mod asset_source;
pub mod decoder;
pub mod encoder;
pub mod error;
pub mod glb;
pub mod json_model;
pub mod json_to_scene;
pub mod scene_to_json;
pub mod validation;

pub use asset_source::BufferViewAsset;
pub use decoder::GltfDecoder;
pub use encoder::{json_encoder, GltfEncoder, OutputFlavour, QuantizeMode};
pub use error::{Error, Result};

/// Register the `.gltf` + `.glb` decoder and encoder factories with
/// `registry`. Only available with the default `registry` feature.
#[cfg(feature = "registry")]
pub fn register(registry: &mut oxideav_mesh3d::Mesh3DRegistry) {
    registry.register_decoder(
        "gltf",
        &["gltf", "glb"],
        Box::new(|| Box::new(GltfDecoder::new())),
    );
    registry.register_encoder(
        "gltf",
        &["gltf"],
        Box::new(|| Box::new(GltfEncoder::with_output(OutputFlavour::JsonEmbedded))),
    );
    registry.register_encoder(
        "glb",
        &["glb"],
        Box::new(|| Box::new(GltfEncoder::with_output(OutputFlavour::Glb))),
    );
}
