//! Accept / reject behaviour of the Draco decoder on damaged payloads.
//!
//! * The reader verdicts of `docs/3d/gltf/draco/spec/01-container.md` §4:
//!   single-byte edits of `eb-quad/input.drc` with the verdict a black-box
//!   decoder gave on each (the edits are fully described there; the edited
//!   files are not staged, so they are rebuilt here).
//! * Hostile input: every truncation of every staged payload is an error,
//!   and no single-byte substitution anywhere in a staged payload makes
//!   the decoder panic, overflow or produce an ill-formed mesh.

use oxideav_gltf::draco::{self, DracoMesh, Limits, Semantic};
use std::path::PathBuf;

fn payload(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/draco")
        .join(name)
        .join("input.drc");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn fixture_names() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/draco");
    let mut names: Vec<String> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 38);
    names
}

/// Value table and point-to-value map of one attribute.
type AttributeData = (Vec<u8>, Vec<u32>);

/// Geometry of a decoded mesh, for "output unchanged" comparisons.
fn geometry(m: &DracoMesh) -> (u32, Vec<[u32; 3]>, Vec<AttributeData>) {
    (
        m.num_points,
        m.faces.clone(),
        m.attributes
            .iter()
            .map(|a| (a.values.clone(), a.point_to_value.clone()))
            .collect(),
    )
}

fn edited(base: &[u8], offset: usize, bytes: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    out[offset..offset + bytes.len()].copy_from_slice(bytes);
    out
}

// Offsets inside `eb-quad/input.drc` (its `trace.txt` read lines).
const MAGIC_LAST: usize = 4;
const VERSION_MAJOR: usize = 5;
const GEOMETRY_TYPE: usize = 7;
const ENCODING_METHOD: usize = 8;
const FLAGS: usize = 9;
const SYMBOL_CODING: usize = 11;
const SEMANTIC: usize = 28;
const DATA_TYPE: usize = 29;
const CODING_KIND: usize = 33;
const PREDICTION_METHOD: usize = 34;
const PREDICTION_TRANSFORM: usize = 35;
const QUANT_BITS: usize = 79;

#[test]
fn reader_verdicts_on_edited_eb_quad() {
    let base = payload("eb-quad");
    assert_eq!(base.len(), 80);
    let reference = draco::decode(&base).unwrap();

    let rejected: Vec<(&str, Vec<u8>)> = vec![
        ("magic last byte changed", edited(&base, MAGIC_LAST, b"0")),
        ("version 2.3", edited(&base, VERSION_MAJOR, &[2, 3])),
        ("version 3.0", edited(&base, VERSION_MAJOR, &[3, 0])),
        ("version 0.2", edited(&base, VERSION_MAJOR, &[0, 2])),
        ("version 2.1", edited(&base, VERSION_MAJOR, &[2, 1])),
        ("version 1.2", edited(&base, VERSION_MAJOR, &[1, 2])),
        ("geometry type 0", edited(&base, GEOMETRY_TYPE, &[0])),
        ("geometry type 2", edited(&base, GEOMETRY_TYPE, &[2])),
        ("encoding method 2", edited(&base, ENCODING_METHOD, &[2])),
        ("symbol coding 1", edited(&base, SYMBOL_CODING, &[1])),
        ("symbol coding 3", edited(&base, SYMBOL_CODING, &[3])),
        ("last byte removed", base[..base.len() - 1].to_vec()),
        ("data type 0", edited(&base, DATA_TYPE, &[0])),
        ("data type 12", edited(&base, DATA_TYPE, &[12])),
        ("semantic 5", edited(&base, SEMANTIC, &[5])),
        ("coding kind 4", edited(&base, CODING_KIND, &[4])),
        (
            "prediction method 7",
            edited(&base, PREDICTION_METHOD, &[7]),
        ),
        (
            "prediction transform 0",
            edited(&base, PREDICTION_TRANSFORM, &[0]),
        ),
        ("quantization bits 0", edited(&base, QUANT_BITS, &[0])),
        ("quantization bits 31", edited(&base, QUANT_BITS, &[31])),
    ];
    for (what, bytes) in &rejected {
        assert!(draco::decode(bytes).is_err(), "{what} must be rejected");
    }

    // Accepted with unchanged output.
    let mut appended = base.clone();
    appended.extend_from_slice(&[1, 2, 3]);
    for (what, bytes) in [
        ("flags 0x0001", edited(&base, FLAGS, &[0x01, 0x00])),
        ("flags 0x4000", edited(&base, FLAGS, &[0x00, 0x40])),
        ("3 bytes appended", appended),
    ] {
        let mesh = draco::decode(&bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(geometry(&mesh), geometry(&reference), "{what}");
        assert_eq!(mesh.bytes_consumed, 80, "{what}");
        assert!(mesh.metadata.is_none(), "{what}");
    }

    // Semantic 0 -> 4: accepted, only the reported semantic changes.
    let mesh = draco::decode(&edited(&base, SEMANTIC, &[4])).unwrap();
    assert_eq!(mesh.attributes[0].semantic, Semantic::Generic);
    assert_eq!(geometry(&mesh), geometry(&reference));

    // Quantization bits 30: accepted (the values change scale).
    let mesh = draco::decode(&edited(&base, QUANT_BITS, &[30])).unwrap();
    assert_eq!(mesh.faces, reference.faces);
    assert_eq!(
        mesh.attributes[0].point_to_value,
        reference.attributes[0].point_to_value
    );
}

#[test]
fn unspecified_inputs_are_rejected_not_guessed() {
    // `spec/10-roadmap.md`: a decoder built from the chapters rejects what
    // they do not specify. Prediction method 2 belongs to earlier bitstream
    // versions and has no stated rule; prediction method 3 likewise.
    let base = payload("eb-quad");
    for method in [2u8, 3] {
        let err = draco::decode(&edited(&base, PREDICTION_METHOD, &[method])).unwrap_err();
        assert!(
            matches!(err, oxideav_gltf::Error::Unsupported(_)),
            "method {method}: {err}"
        );
    }
    // Point clouds and older bitstream versions are unsupported, not
    // malformed.
    for bytes in [
        edited(&base, GEOMETRY_TYPE, &[0]),
        edited(&base, VERSION_MAJOR, &[2, 1]),
    ] {
        assert!(matches!(
            draco::decode(&bytes).unwrap_err(),
            oxideav_gltf::Error::Unsupported(_)
        ));
    }
}

fn assert_well_formed(mesh: &DracoMesh) {
    for face in &mesh.faces {
        assert!(face.iter().all(|&i| i < mesh.num_points));
    }
    for a in &mesh.attributes {
        assert_eq!(a.values.len() % a.value_size(), 0);
        assert_eq!(a.point_to_value.len(), mesh.num_points as usize);
        let count = a.value_count();
        assert!(a.point_to_value.iter().all(|&v| (v as usize) < count));
    }
}

#[test]
fn every_truncation_is_an_error() {
    for name in fixture_names() {
        let bytes = payload(&name);
        let full = draco::decode(&bytes).unwrap().bytes_consumed;
        for len in 0..full {
            assert!(
                draco::decode(&bytes[..len]).is_err(),
                "{name}: prefix of {len} / {full} bytes decoded"
            );
        }
    }
}

#[test]
fn single_byte_substitutions_never_panic() {
    // Small limits: a substituted count must not buy a large allocation.
    let limits = Limits::new().with_max_points(4096).with_max_faces(4096);
    let mut decoded = 0usize;
    let mut rejected = 0usize;
    for name in fixture_names() {
        let base = payload(&name);
        // The larger payloads are swept with a stride to keep the test quick
        // in debug builds.
        let stride = match base.len() {
            0..=600 => 1,
            601..=1200 => 3,
            _ => 7,
        };
        let mut bytes = base.clone();
        for offset in (0..base.len()).step_by(stride) {
            let original = base[offset];
            for value in [
                0x00,
                0x01,
                0x7f,
                0x80,
                0xff,
                original.wrapping_add(1),
                original ^ 0x40,
            ] {
                if value == original {
                    continue;
                }
                bytes[offset] = value;
                match draco::decode_with_limits(&bytes, &limits) {
                    Ok(mesh) => {
                        assert_well_formed(&mesh);
                        decoded += 1;
                    }
                    Err(_) => rejected += 1,
                }
            }
            bytes[offset] = original;
        }
    }
    // Both outcomes occur: many substitutions only change values.
    assert!(decoded > 1000 && rejected > 1000, "{decoded} / {rejected}");
}

#[test]
fn limits_bound_declared_counts() {
    let bytes = payload("eb-grid17"); // 272 points, 480 faces
    let ok = Limits::new().with_max_points(272).with_max_faces(480);
    assert!(draco::decode_with_limits(&bytes, &ok).is_ok());
    assert!(draco::decode_with_limits(&bytes, &ok.with_max_points(271)).is_err());
    assert!(draco::decode_with_limits(&bytes, &ok.with_max_faces(479)).is_err());
    let bytes = payload("seq-grid17-idx16");
    assert!(draco::decode_with_limits(&bytes, &ok).is_ok());
    assert!(draco::decode_with_limits(&bytes, &ok.with_max_points(271)).is_err());
    assert!(draco::decode_with_limits(&bytes, &ok.with_max_faces(479)).is_err());
    // Seam sets split vertices into points: 8 vertices, 24 points.
    let bytes = payload("eb-cube-seams");
    assert!(draco::decode_with_limits(&bytes, &Limits::new().with_max_points(24)).is_ok());
    assert!(draco::decode_with_limits(&bytes, &Limits::new().with_max_points(23)).is_err());
}
