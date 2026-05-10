//! Error helpers — re-export `oxideav_mesh3d::Error` (which itself
//! re-exports `oxideav_core::Error` under the `registry` feature, or
//! its standalone enum without). Both feature paths expose
//! `invalid(...)` and `unsupported(...)` constructors, so the helpers
//! here are uniform across builds.

pub use oxideav_mesh3d::{Error, Result};

/// Helper to construct an [`Error::InvalidData`].
pub fn invalid(msg: impl Into<String>) -> Error {
    Error::invalid(msg)
}

/// Helper to construct an [`Error::Unsupported`].
pub fn unsupported(msg: impl Into<String>) -> Error {
    Error::unsupported(msg)
}
