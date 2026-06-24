//! GLB-stored BIN chunk length validation (Khronos glTF 2.0 §3.6.1.2).
//!
//! The spec allows the BIN chunk to be *up to 3 bytes* larger than the
//! JSON-declared `buffer.byteLength` (so a writer need not re-update the
//! length after applying the chunk's 4-byte zero padding). A surplus of
//! 4 or more bytes is a genuine mismatch, not padding, and MUST be
//! rejected. A deficit (BIN shorter than declared) is likewise invalid.

use oxideav_gltf::{glb, GltfDecoder};
use oxideav_mesh3d::Mesh3DDecoder;

/// Build a `.glb` whose single uri-less `buffer[0]` declares
/// `byteLength == declared` while the actual BIN chunk carries
/// `bin_len` bytes (before the encoder's own 4-byte padding).
fn glb_with_bin(declared: usize, bin_len: usize) -> Vec<u8> {
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\"}},\"buffers\":[{{\"byteLength\":{declared}}}]}}"
    );
    let bin = vec![0u8; bin_len];
    glb::encode(json.as_bytes(), Some(&bin))
}

#[test]
fn accepts_bin_padding_up_to_three_bytes() {
    // declared 5, BIN 8 (5 + 3 padding bytes) — the maximum slack the
    // spec sanctions. The encoder pads the 8-byte BIN to 8 (already a
    // multiple of 4), so the surplus is exactly 3.
    let glb = glb_with_bin(5, 8);
    let mut dec = GltfDecoder::new();
    assert!(
        dec.decode(&glb).is_ok(),
        "a 3-byte BIN surplus is spec-legal padding"
    );
}

#[test]
fn accepts_bin_exact_length() {
    let glb = glb_with_bin(4, 4);
    let mut dec = GltfDecoder::new();
    assert!(dec.decode(&glb).is_ok());
}

#[test]
fn rejects_bin_surplus_of_four_or_more() {
    // declared 4, BIN 8 → surplus 4 → not padding.
    let glb = glb_with_bin(4, 8);
    let mut dec = GltfDecoder::new();
    let err = dec.decode(&glb).unwrap_err().to_string();
    assert!(
        err.contains("GlbBufferLength"),
        "expected GlbBufferLength, got: {err}"
    );
}

#[test]
fn rejects_bin_shorter_than_declared() {
    // declared 16, BIN 4 → deficit, the chunk cannot satisfy the
    // accessor pipeline.
    let glb = glb_with_bin(16, 4);
    let mut dec = GltfDecoder::new();
    assert!(dec.decode(&glb).is_err());
}
