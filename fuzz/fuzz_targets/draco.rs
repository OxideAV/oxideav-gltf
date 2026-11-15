#![no_main]

//! Fuzz target for the raw Draco compressed-mesh bitstream decoder
//! (`oxideav_gltf::draco`).
//!
//! Contract under test:
//!
//! * **No panic, abort, overflow or out-of-bounds access** on any input —
//!   every count / size in the bitstream is attacker-controlled and must be
//!   validated before it sizes an allocation or indexes a table.
//! * **Bounded work**: the entropy coder can describe a symbol in zero
//!   bytes, so declared counts are not bounded by the payload size; the
//!   decoder's `Limits` are what bounds them. The target decodes under
//!   small limits so an input cannot buy more than a few megabytes.
//! * **Well-formed output on success**: face indices and point-to-value
//!   maps stay in range and value tables have whole entries.
//!
//! The input is decoded three ways: as given, and behind a well-formed
//! 11-byte header for each mesh encoding method, so that mutations spend
//! their time in the connectivity and attribute sections rather than on
//! the magic.
//!
//! Seed corpus: the staged payloads, e.g.
//!
//! ```bash
//! mkdir -p fuzz/corpus/draco
//! for d in tests/fixtures/draco/*/; do
//!     cp "$d/input.drc" "fuzz/corpus/draco/$(basename "$d").drc"
//! done
//! cargo +nightly fuzz run draco -- -max_total_time=300
//! ```

use libfuzzer_sys::fuzz_target;
use oxideav_gltf::draco::{self, DracoMesh, Limits};

fn check(mesh: &DracoMesh) {
    for face in &mesh.faces {
        for &i in face {
            assert!(i < mesh.num_points, "face index out of range");
        }
    }
    for a in &mesh.attributes {
        let size = a.value_size();
        assert!(size > 0);
        assert_eq!(a.values.len() % size, 0, "partial value in table");
        assert_eq!(a.point_to_value.len(), mesh.num_points as usize);
        let count = a.value_count();
        for &v in &a.point_to_value {
            assert!((v as usize) < count, "point maps past the value table");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let limits = Limits::new()
        .with_max_points(1 << 14)
        .with_max_faces(1 << 14);
    if let Ok(mesh) = draco::decode_with_limits(data, &limits) {
        check(&mesh);
    }
    let mut framed = Vec::with_capacity(data.len() + 11);
    for method in [0u8, 1] {
        framed.clear();
        framed.extend_from_slice(b"DRACO\x02\x02\x01");
        framed.push(method);
        framed.extend_from_slice(&[0, 0]);
        framed.extend_from_slice(data);
        if let Ok(mesh) = draco::decode_with_limits(&framed, &limits) {
            check(&mesh);
        }
    }
});
