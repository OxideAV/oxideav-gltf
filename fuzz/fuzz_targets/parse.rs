#![no_main]

//! Panic-freedom fuzz target for the `oxideav-gltf` parser.
//!
//! Feeds arbitrary attacker-controlled bytes through every public
//! decoder entry point and asserts that none of them panic, abort,
//! debug-overflow, or index out of bounds. The return values are
//! intentionally discarded — the contract under test is *the call
//! returns*, not what it returns.
//!
//! Classic glTF parser danger spots this target drives:
//!
//! * **GLB header + chunk-length overflow** — the 12-byte GLB header
//!   declares a total `length`; each chunk inside declares its own
//!   `chunkLength`. Both fields are `u32` and the walker MUST cap
//!   `data_start + chunk_len` against the container without panicking
//!   on the arithmetic. Per spec §4.4.3.1 `chunkLength` MUST also be a
//!   multiple of 4, and §4.4.3.2 / §4.4.3.3 reserve slots 0 and 1 for
//!   the JSON and (optional) BIN chunks; all four rules surface as
//!   `Err`, never a panic.
//! * **Magic-sniff bypass** — `GltfDecoder::decode` peeks the first 4
//!   bytes for `b"glTF"`. A 3-byte input MUST be sniffed as JSON
//!   (the slice index would panic without the length guard) and a
//!   4-byte `b"glTF"` followed by nothing MUST bail with the
//!   "too short for 12-byte header" GLB error rather than panic on
//!   the missing version + length u32s.
//! * **JSON nesting / size bombs** — `check_json_byte_length` +
//!   `check_json_depth` run BEFORE `serde_json::from_slice` so a
//!   1000-deep `[[[[...` array bomb or a 1 GiB declaration cannot
//!   blow the parser's recursive descent stack or the allocator.
//!   Round 7 fuzz hardening; the target re-exercises both caps.
//! * **Buffer-view stride arithmetic** — `accessor::locate` and
//!   `accessor::materialise_accessor` compute
//!   `byte_offset + stride * (count - 1) + element_size`. Without
//!   `checked_*` arithmetic this overflows `usize` and either wraps
//!   to a small value (admitting an out-of-bounds slice) or hits an
//!   `attempt to multiply with overflow` debug panic. The fuzz
//!   target drives these through the high-level convert path (which
//!   runs `validate_accessor_fits_bufferview` first — that validator
//!   uses `u64::checked_mul` / `checked_add` to surface the overflow
//!   as `AccessorFitOverflow`) and trusts the validators to catch
//!   the malicious offsets before the per-element walker runs.
//! * **Accessor count / componentType mismatch** — every
//!   per-attribute reader (`read_vec_f32::<N>`, `read_scalar_f32`,
//!   `read_vec4_u16`, `read_indices_u32`) calls
//!   `try_into().unwrap()` on per-element slices. The element-size
//!   guard at the top of each function MUST reject mismatched widths
//!   BEFORE the `unwrap()` runs — a `VEC3` accessor wired to a
//!   `MAT4` bufferView would otherwise blow up inside the slice
//!   conversion. The harness exercises the full
//!   `Mesh3DDecoder::decode` path which routes through these
//!   readers via `json_to_scene::convert`.
//! * **Extension dispatch** — `extensionsUsed` / `extensionsRequired`
//!   and the per-extension JSON deserialisers
//!   (`KHR_lights_punctual`, the eight `KHR_materials_*` extensions,
//!   …) MUST accept any serde-deserialisable shape without
//!   panicking. Unknown extension names are tolerated; malformed
//!   values surface as `Err(Error::InvalidData)`. The §3.12 stack
//!   validator additionally rejects data blocks declared without an
//!   `extensionsUsed` entry — again, `Err`, never panic.
//! * **Base64 data: URI decoder** — `decode_data_uri` is reached
//!   when a buffer / image carries a `data:` URI; the base64 crate's
//!   `STANDARD.decode` returns `Result` for malformed input but the
//!   *prefix* arithmetic (`uri.find(',')`, `uri.strip_prefix("data:")`)
//!   walks UTF-8 boundaries. The decoder accepts the URI as `&str`
//!   so the JSON parser has already validated UTF-8; the harness
//!   re-exercises this via the JSON path.
//!
//! The target does NOT exercise the encoder: there's no
//! attacker-controlled `Scene3D` to feed it, and the
//! roundtrip-from-decoded-input pattern would just re-validate
//! `scene_to_json`'s output against `json_to_scene`'s acceptance —
//! useful, but a separate target. This target keeps a tight focus on
//! the decoder + chunk walker.

use libfuzzer_sys::fuzz_target;
use oxideav_gltf::glb;
use oxideav_gltf::GltfDecoder;
use oxideav_mesh3d::Mesh3DDecoder;

fuzz_target!(|data: &[u8]| {
    // 1. High-level decoder. Sniffs `b"glTF"` and dispatches into the
    //    GLB chunk walker or the raw-JSON path. This is the surface a
    //    .gltf / .glb consumer actually hits — every parser bug
    //    eventually surfaces here.
    let mut dec = GltfDecoder::new();
    let _ = dec.decode(data);

    // 2. Low-level GLB chunk walker. Called directly so we exercise
    //    inputs the magic sniff would reject (the slice `&data[..4]`
    //    is only checked when `data.len() >= 4`; the 0/1/2/3-byte
    //    inputs go through the JSON branch in the high-level decoder
    //    and never reach `glb::parse`).
    let _ = glb::parse(data);

    // 3. Sub-byte-offset GLB parse. Walk the chunk parser at small
    //    offsets so framing alignment relative to the container start
    //    gets exercised against attacker bytes. Cap at 16 to keep
    //    iteration cost bounded.
    let cap = data.len().min(16);
    for offset in 1..cap {
        let _ = glb::parse(&data[offset..]);
    }

    // 4. Truncation. A common attacker move is to declare a long body
    //    in the header but ship a short one — re-run the high-level
    //    decoder on every prefix length so the walker hits the
    //    truncated-chunk path at every chunk boundary.
    let trunc_cap = data.len().min(64);
    for take in 0..trunc_cap {
        let mut dec = GltfDecoder::new();
        let _ = dec.decode(&data[..take]);
    }
});
