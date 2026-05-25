//! Shared helpers for the `oxideav-gltf-fuzz` targets.
//!
//! Currently empty — the `parse` target is self-contained because the
//! glTF fuzz surface is decode-only (no encoder bootstrap, no oracle
//! cross-decode). The library exists so future targets that need a
//! shared corpus generator (e.g. a structured Arbitrary-driven Scene3D
//! synthesiser feeding the encoder + decoder roundtrip) can grow here
//! without re-wiring Cargo.
