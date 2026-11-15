//! Replays the Draco fixture corpus of `docs/3d/gltf/draco/fixtures/`
//! (vendored under `tests/fixtures/draco/`: `input.drc` + `expected.txt`
//! of each of the 38 fixtures) against the decoder, per the replay
//! workflow of `docs/3d/gltf/draco/draco-fixtures-and-traces.md` §5:
//! the decoded mesh is printed in the `expected.txt` form (§3) and must
//! match byte for byte — counts, attribute descriptors, per-point value
//! indices, component bit patterns and faces.
//!
//! When the documentation tree is present next to the crate (umbrella
//! checkout), the staged `trace.txt` derived-value events are replayed
//! too; a standalone checkout skips that test at run time.

use oxideav_gltf::draco::{self, DataType, DracoMesh, EncodingMethod, Limits};
use std::fmt::Write as _;
use std::path::PathBuf;

/// Every fixture directory of the corpus.
const FIXTURES: [&str; 38] = [
    "seq-tri-raw",
    "seq-quad-idxdiff",
    "seq-grid4-quant",
    "seq-grid4-rawint",
    "seq-grid17-idx16",
    "seq-grid17-idxdiff",
    "seq-colors",
    "eb-tri",
    "eb-quad",
    "eb-tetra",
    "eb-cube",
    "eb-cube-seams",
    "eb-cube-seams-valence",
    "eb-grid4-noquant",
    "eb-grid4-q-low",
    "eb-grid4-q-high",
    "eb-grid4-q16",
    "eb-torus",
    "eb-torus-normals-cl10",
    "eb-torus-valence",
    "eb-annulus",
    "eb-two-comp",
    "eb-colors",
    "eb-grid7-valence",
    "eb-grid17",
    "eb-metadata",
    "lvl-grid7-cl0",
    "lvl-grid7-cl1",
    "lvl-grid7-cl2",
    "lvl-grid7-cl3",
    "lvl-grid7-cl4",
    "lvl-grid7-cl5",
    "lvl-grid7-cl6",
    "lvl-grid7-cl7",
    "lvl-grid7-cl8",
    "lvl-grid7-cl9",
    "lvl-grid7-cl10",
    "glb-grid4",
];

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/draco")
        .join(name)
}

fn read_fixture(name: &str, file: &str) -> Vec<u8> {
    let path = fixture_dir(name).join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// C `%.9g` of a binary32 value.
fn format_g9(v: f32) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".to_owned()
        } else if v < 0.0 {
            "-inf".to_owned()
        } else {
            "inf".to_owned()
        };
    }
    // Nine significant digits, correctly rounded, in scientific form.
    let sci = format!("{:.8e}", f64::from(v));
    let (mantissa, exponent) = sci.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let (sign, digits) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest.replace('.', "")),
        None => ("", mantissa.replace('.', "")),
    };
    let strip = |s: &str| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s.to_owned()
        }
    };
    if !(-4..9).contains(&exponent) {
        let body = strip(&format!("{}.{}", &digits[..1], &digits[1..]));
        let esign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{body}e{esign}{:02}", exponent.abs());
    }
    let body = if exponent >= 0 {
        let split = exponent as usize + 1;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{}{}", "0".repeat((-exponent - 1) as usize), digits)
    };
    format!("{sign}{}", strip(&body))
}

fn format_component(out: &mut String, data_type: DataType, b: &[u8]) {
    match data_type {
        DataType::Float32 => {
            let v = f32::from_le_bytes(b.try_into().unwrap());
            write!(out, " {}[{:08x}]", format_g9(v), v.to_bits()).unwrap();
        }
        DataType::Int8 => write!(out, " {}", b[0] as i8).unwrap(),
        DataType::Uint8 | DataType::Bool => write!(out, " {}", b[0]).unwrap(),
        DataType::Int16 => write!(out, " {}", i16::from_le_bytes(b.try_into().unwrap())).unwrap(),
        DataType::Uint16 => write!(out, " {}", u16::from_le_bytes(b.try_into().unwrap())).unwrap(),
        DataType::Int32 => write!(out, " {}", i32::from_le_bytes(b.try_into().unwrap())).unwrap(),
        DataType::Uint32 => write!(out, " {}", u32::from_le_bytes(b.try_into().unwrap())).unwrap(),
        DataType::Int64 => write!(out, " {}", i64::from_le_bytes(b.try_into().unwrap())).unwrap(),
        DataType::Uint64 => write!(out, " {}", u64::from_le_bytes(b.try_into().unwrap())).unwrap(),
        DataType::Float64 => write!(out, " {}", f64::from_le_bytes(b.try_into().unwrap())).unwrap(),
        _ => unreachable!("data type not in the corpus"),
    }
}

/// Print a decoded mesh in the `expected.txt` form.
fn to_expected(mesh: &DracoMesh) -> String {
    let mut out = String::new();
    writeln!(out, "points {}", mesh.num_points).unwrap();
    writeln!(out, "faces {}", mesh.faces.len()).unwrap();
    writeln!(out, "attributes {}", mesh.attributes.len()).unwrap();
    for (i, a) in mesh.attributes.iter().enumerate() {
        writeln!(
            out,
            "attribute {i} semantic {} data_type {} components {} normalized {} unique_id {} values {}",
            a.semantic.code(),
            a.data_type.code(),
            a.num_components,
            u8::from(a.normalized),
            a.unique_id,
            a.value_count()
        )
        .unwrap();
        for p in 0..mesh.num_points as usize {
            write!(out, "a{i} p{p} v{}:", a.point_to_value[p]).unwrap();
            for b in a.point_bytes(p).chunks_exact(a.data_type.size()) {
                format_component(&mut out, a.data_type, b);
            }
            out.push('\n');
        }
    }
    for (n, f) in mesh.faces.iter().enumerate() {
        writeln!(out, "f{n}: {} {} {}", f[0], f[1], f[2]).unwrap();
    }
    out
}

#[test]
fn g9_formatting() {
    assert_eq!(format_g9(1.0), "1");
    assert_eq!(format_g9(0.50024426), "0.50024426");
    assert_eq!(format_g9(-0.25), "-0.25");
    assert_eq!(format_g9(1.0e-5), "9.99999975e-06");
    assert_eq!(format_g9(123456789.0), "123456792");
    assert_eq!(format_g9(1.0e10), "1e+10");
    assert_eq!(format_g9(0.000125), "0.000125000006");
}

#[test]
fn every_fixture_reproduces_expected_txt() {
    let mut failures = Vec::new();
    for name in FIXTURES {
        let drc = read_fixture(name, "input.drc");
        // Tolerate a checkout that rewrote line endings.
        let expected = String::from_utf8(read_fixture(name, "expected.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        match draco::decode(&drc) {
            Ok(mesh) => {
                let got = to_expected(&mesh);
                if got != expected {
                    let line = got
                        .lines()
                        .zip(expected.lines())
                        .position(|(a, b)| a != b)
                        .unwrap_or(got.lines().count().min(expected.lines().count()));
                    failures.push(format!(
                        "{name}: first difference at line {}:\n  got      {:?}\n  expected {:?}",
                        line + 1,
                        got.lines().nth(line),
                        expected.lines().nth(line)
                    ));
                }
                // ch. 01 §3: the payload ends at the last group's data; only the
                // glTF-embedded payload carries two padding bytes (ch. 09 §1).
                let padding = if name == "glb-grid4" { 2 } else { 0 };
                if mesh.bytes_consumed + padding != drc.len() {
                    failures.push(format!(
                        "{name}: consumed {} of {} bytes",
                        mesh.bytes_consumed,
                        drc.len()
                    ));
                }
                let method = if name.starts_with("seq-") || name == "lvl-grid7-cl0" {
                    EncodingMethod::Sequential
                } else {
                    EncodingMethod::Edgebreaker
                };
                if mesh.method != method {
                    failures.push(format!("{name}: unexpected method {:?}", mesh.method));
                }
            }
            Err(e) => failures.push(format!("{name}: decode failed: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixtures failed:\n{}",
        failures.len(),
        FIXTURES.len(),
        failures.join("\n")
    );
}

/// Kinds of derived-value trace lines the decoder emits
/// (`draco-fixtures-and-traces.md` §4.3).
const TRACE_KINDS: [&str; 11] = [
    "eb_symbol[",
    "start_face_interior ",
    "seam set=",
    "conn face ",
    "seq[",
    "residual[",
    "para entry=",
    "cmp entry=",
    "uv_pred[",
    "normal_pred[",
    "integer[",
];

/// The staged traces are not vendored (they are large); replay them from
/// the documentation tree when the crate sits inside the umbrella checkout.
#[test]
fn staged_trace_events_are_reproduced() {
    let docs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/3d/gltf/draco/fixtures");
    if !docs.is_dir() {
        eprintln!("skipping: {} not present", docs.display());
        return;
    }
    let mut checked = 0usize;
    for name in FIXTURES {
        let trace_path = docs.join(name).join("trace.txt");
        let Ok(staged) = std::fs::read_to_string(&trace_path) else {
            eprintln!("skipping {name}: no trace.txt");
            continue;
        };
        let drc = read_fixture(name, "input.drc");
        let (mesh, events) = draco::decode_traced(&drc, &Limits::new());
        mesh.unwrap_or_else(|e| panic!("{name}: {e}"));
        // Compact and elided traces drop lines, so per kind the staged lines
        // must appear in the decoder's event list in the same order.
        for kind in TRACE_KINDS {
            let mut ours = events.iter().filter(|l| l.starts_with(kind));
            for (n, line) in staged.lines().enumerate() {
                let line = line.trim_start();
                if !line.starts_with(kind) {
                    continue;
                }
                assert!(
                    ours.any(|l| l == line),
                    "{name}: trace.txt line {} not reproduced in order: {line:?}",
                    n + 1
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 4_000, "only {checked} trace lines were compared");
}

#[test]
fn metadata_block_is_surfaced() {
    let mesh = draco::decode(&read_fixture("eb-metadata", "input.drc")).unwrap();
    let meta = mesh.metadata.expect("metadata block");
    // ch. 01 §2: two attribute records (unique ids 1 and 2) with three
    // entries each, and an empty geometry-level record.
    let ids: Vec<u32> = meta.attributes.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [1, 2]);
    for (_, record) in &meta.attributes {
        assert_eq!(record.entries.len(), 3);
        assert!(record.children.is_empty());
    }
    assert!(meta.geometry.entries.is_empty() && meta.geometry.children.is_empty());
    let all: Vec<&draco::MetadataRecord> = meta.attributes.iter().map(|(_, r)| r).collect();
    assert!(all.iter().any(|r| r.get("name") == Some(b"material")));
    assert!(
        all.iter()
            .any(|r| r.get("blue") == Some(&1u32.to_le_bytes()[..]))
    );
    for name in FIXTURES.iter().filter(|n| **n != "eb-metadata") {
        let mesh = draco::decode(&read_fixture(name, "input.drc")).unwrap();
        assert!(mesh.metadata.is_none(), "{name}");
    }
}

#[test]
fn glb_payload_matches_extracted_payload() {
    // `glb-grid4/input.drc` is the payload extracted from `input.glb`.
    let glb = read_fixture("glb-grid4", "input.glb");
    let drc = read_fixture("glb-grid4", "input.drc");
    let at = glb
        .windows(5)
        .position(|w| w == b"DRACO")
        .expect("payload inside the GLB");
    assert_eq!(&glb[at..at + drc.len()], &drc[..]);
    let mesh = draco::decode(&glb[at..]).unwrap();
    assert_eq!(mesh.bytes_consumed, 310);
    assert_eq!((mesh.num_points, mesh.faces.len()), (16, 18));
    let ids: Vec<u32> = mesh.attributes.iter().map(|a| a.unique_id).collect();
    assert_eq!(ids, [1, 2, 0]);
}
