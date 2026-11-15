//! Synthetic Draco payloads, written by a small test-side encoder derived
//! from the same specification as the decoder
//! (`docs/3d/gltf/draco/spec/`), to reach what the staged corpus cannot:
//!
//! * rANS precisions above 12, frequency tokens with two extra bytes and a
//!   4-byte initial state (ch. 03 §1–§2; the corpus is precision 12 only);
//! * plain indices stored as varint and as u32 (ch. 04 §2);
//! * meshes large enough to time the decoder.
//!
//! These are **self-consistency** tests: they prove the decoder inverts an
//! encoder built from the specification's prose, not that either matches
//! an independent producer. The staged fixtures (`draco_fixtures.rs`) are
//! the conformance evidence.
//!
//! `DRACO_SYNTH_FACES=<n>` sets the size of the large meshes (default
//! 20 000 faces); run with `--release -- --nocapture` to see timings.

use oxideav_gltf::draco::{self, DracoMesh, Limits};
use std::time::Instant;

// ---------------------------------------------------------------------
// Byte-level writers (ch. 02).

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// Bit run writer: least-significant bit first.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    used: usize,
}

impl Bits {
    fn put(&mut self, value: u32, n: u32) {
        for i in 0..n {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                *self.bytes.last_mut().unwrap() |= 1 << (self.used % 8);
            }
            self.used += 1;
        }
    }
}

/// Deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u32) -> u32 {
        (self.next() >> 33) as u32 % n
    }
}

// ---------------------------------------------------------------------
// Entropy encoder (inverse of ch. 03).

/// Frequencies summing to `1 << precision`, at least 1 for every symbol
/// that occurs.
fn frequencies(symbols: &[u32], precision: u32) -> Vec<u32> {
    let alphabet = symbols.iter().copied().max().unwrap_or(0) as usize + 1;
    let mut counts = vec![0u64; alphabet];
    for &s in symbols {
        counts[s as usize] += 1;
    }
    let total = 1u64 << precision;
    let n = symbols.len() as u64;
    let mut freq: Vec<u32> = counts
        .iter()
        .map(|&c| {
            if c == 0 {
                0
            } else {
                (c * total / n).max(1) as u32
            }
        })
        .collect();
    let mut sum: u64 = freq.iter().map(|&f| u64::from(f)).sum();
    assert!(
        counts.iter().filter(|&&c| c > 0).count() as u64 <= total,
        "more distinct symbols than the rANS total"
    );
    // Hand the rounding difference to the most frequent symbols.
    let mut order: Vec<usize> = (0..alphabet).filter(|&i| freq[i] > 0).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(freq[i]));
    if sum < total {
        freq[order[0]] += (total - sum) as u32;
        sum = total;
    }
    for &i in &order {
        if sum == total {
            break;
        }
        let take = (sum - total).min(u64::from(freq[i]) - 1);
        freq[i] -= take as u32;
        sum -= take;
    }
    assert_eq!(sum, total, "cannot normalise frequencies");
    freq
}

/// Frequency table serialisation (§1.1).
fn write_table(out: &mut Vec<u8>, freq: &[u32]) {
    varint(out, freq.len() as u64);
    let mut i = 0;
    while i < freq.len() {
        let f = freq[i];
        if f == 0 {
            let run = freq[i..].iter().take(64).take_while(|&&f| f == 0).count();
            out.push((((run - 1) as u8) << 2) | 3);
            i += run;
            continue;
        }
        if f < 1 << 6 {
            out.push((f << 2) as u8);
        } else if f < 1 << 14 {
            out.push((((f & 0x3f) << 2) | 1) as u8);
            out.push((f >> 6) as u8);
        } else {
            assert!(f < 1 << 22);
            out.push((((f & 0x3f) << 2) | 2) as u8);
            out.push((f >> 6) as u8);
            out.push((f >> 14) as u8);
        }
        i += 1;
    }
}

/// rANS stream for `symbols` (§1.2–§1.3 run backwards): the decoder's
/// step `state' = q * freq + r - cum` with `state = q * P + r` is inverted
/// symbol by symbol from the last to the first, moving low bytes out
/// whenever the predecessor state would leave `[L, 256 L)`.
fn rans_stream(symbols: &[u32], freq: &[u32], precision: u32) -> Vec<u8> {
    let mut cum = vec![0u32; freq.len()];
    for i in 1..freq.len() {
        cum[i] = cum[i - 1] + freq[i - 1];
    }
    let low = 4u64 << precision;
    let mut x = low;
    let mut bytes = Vec::new();
    for &s in symbols.iter().rev() {
        let f = u64::from(freq[s as usize]);
        assert!(f > 0);
        while x >= f * 1024 {
            bytes.push(x as u8);
            x >>= 8;
        }
        x = ((x / f) << precision) + x % f + u64::from(cum[s as usize]);
    }
    // Final state: (k + 1) bytes, k in the top two bits of the last one.
    let raw = x - low;
    let k = (0..4u32)
        .find(|k| raw < 1u64 << (8 * (k + 1) - 2))
        .expect("state fits four bytes");
    let tagged = raw | (u64::from(k) << (8 * (k + 1) - 2));
    bytes.extend_from_slice(&tagged.to_le_bytes()[..k as usize + 1]);
    bytes
}

fn write_stream(out: &mut Vec<u8>, stream: &[u8]) {
    varint(out, stream.len() as u64);
    out.extend_from_slice(stream);
}

/// Symbol sequence, raw scheme (§3.2) with bit-length parameter `b`.
fn write_raw_sequence(out: &mut Vec<u8>, values: &[u32], b: u32) {
    let precision = (3 * b / 2).clamp(12, 20);
    out.push(1);
    out.push(b as u8);
    let freq = frequencies(values, precision);
    write_table(out, &freq);
    write_stream(out, &rans_stream(values, &freq, precision));
}

/// Symbol sequence, tagged scheme (§3.1): the tag of a tuple is the bit
/// length of its largest component.
fn write_tagged_sequence(out: &mut Vec<u8>, values: &[u32], c: usize) {
    let tags: Vec<u32> = values
        .chunks(c)
        .map(|t| 32 - t.iter().copied().max().unwrap().leading_zeros())
        .collect();
    out.push(0);
    let freq = frequencies(&tags, 12);
    write_table(out, &freq);
    write_stream(out, &rans_stream(&tags, &freq, 12));
    let mut run = Bits::default();
    for (tuple, &t) in values.chunks(c).zip(&tags) {
        for &v in tuple {
            run.put(v, t);
        }
    }
    out.extend_from_slice(&run.bytes);
}

fn fold(v: i32) -> u32 {
    if v >= 0 {
        (v as u32) << 1
    } else {
        (((-(v + 1)) as u32) << 1) | 1
    }
}

// ---------------------------------------------------------------------
// Payload builders.

fn header(method: u8) -> Vec<u8> {
    let mut out = b"DRACO\x02\x02\x01".to_vec();
    out.push(method);
    out.extend_from_slice(&[0, 0]);
    out
}

#[derive(Clone, Copy, Debug)]
enum IndexCoding {
    Plain,
    DifferencesRaw(u32),
    DifferencesTagged,
}

/// Method 0 payload: `indices` over `num_points` points, one verbatim
/// float32 position attribute whose point `p` is `(p, 2p, 3p)`.
fn sequential_payload(num_points: u32, indices: &[u32], coding: IndexCoding) -> Vec<u8> {
    let mut out = header(0);
    varint(&mut out, (indices.len() / 3) as u64);
    varint(&mut out, u64::from(num_points));
    match coding {
        IndexCoding::Plain => {
            out.push(1);
            for &i in indices {
                if num_points < 256 {
                    out.push(i as u8);
                } else if num_points < 65536 {
                    out.extend_from_slice(&(i as u16).to_le_bytes());
                } else if num_points < 1 << 21 {
                    varint(&mut out, u64::from(i));
                } else {
                    out.extend_from_slice(&i.to_le_bytes());
                }
            }
        }
        IndexCoding::DifferencesRaw(_) | IndexCoding::DifferencesTagged => {
            out.push(0);
            let mut previous = 0i64;
            let values: Vec<u32> = indices
                .iter()
                .map(|&i| {
                    let delta = i64::from(i) - previous;
                    previous = i64::from(i);
                    ((delta.unsigned_abs() as u32) << 1) | u32::from(delta < 0)
                })
                .collect();
            match coding {
                IndexCoding::DifferencesRaw(b) => write_raw_sequence(&mut out, &values, b),
                _ => write_tagged_sequence(&mut out, &values, 1),
            }
        }
    }
    out.push(1); // one group
    varint(&mut out, 1);
    out.extend_from_slice(&[0, 9, 3, 0]);
    varint(&mut out, 0);
    out.push(0); // verbatim
    for p in 0..num_points {
        for k in 1..=3u32 {
            out.extend_from_slice(&((p * k) as f32).to_le_bytes());
        }
    }
    out
}

/// Skewed random indices: mostly small steps, sometimes a jump anywhere.
fn random_indices(rng: &mut Rng, num_points: u32, faces: usize) -> Vec<u32> {
    let mut at = 0u32;
    (0..faces * 3)
        .map(|_| {
            at = match rng.below(10) {
                0..=5 => at,
                6..=7 => (at + 1) % num_points,
                _ => rng.below(num_points),
            };
            at
        })
        .collect()
}

fn assert_sequential(mesh: &DracoMesh, num_points: u32, indices: &[u32]) {
    assert_eq!(mesh.num_points, num_points);
    let got: Vec<u32> = mesh.faces.iter().flatten().copied().collect();
    assert_eq!(got, indices);
    let a = &mesh.attributes[0];
    assert_eq!(a.value_count(), num_points as usize);
    for p in [0, num_points / 2, num_points - 1] {
        let v = a.point_f32(p as usize).unwrap();
        assert_eq!(v, [p as f32, (2 * p) as f32, (3 * p) as f32]);
    }
}

#[test]
fn raw_scheme_at_every_precision() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    // Raw-scheme parameter B → rANS precision clamp(3B/2, 12, 20).
    for (b, precision) in [
        (1u32, 12u32),
        (4, 12),
        (8, 12),
        (9, 13),
        (10, 15),
        (11, 16),
        (12, 18),
        (13, 19),
        (14, 20),
        (18, 20),
    ] {
        assert_eq!((3 * b / 2).clamp(12, 20), precision);
        // |delta| < 2^(b-1) keeps every sign-magnitude value below 2^b.
        let num_points = 1u32 << (b - 1);
        let faces = (num_points as usize * 2).min(40_000);
        let indices = random_indices(&mut rng, num_points, faces);
        let payload = sequential_payload(num_points, &indices, IndexCoding::DifferencesRaw(b));
        let mesh = draco::decode(&payload).unwrap_or_else(|e| panic!("B={b}: {e}"));
        assert_sequential(&mesh, num_points, &indices);
        assert_eq!(mesh.bytes_consumed, payload.len(), "B={b}");
    }
}

#[test]
fn tagged_scheme_with_wide_values() {
    let mut rng = Rng(7);
    for num_points in [2u32, 300, 70_000, 3_000_000] {
        // Ch. 04 §1: the face count may not exceed a third of the bytes
        // that follow, here dominated by 12 bytes of position per point.
        let faces = 5_000.min(num_points as usize * 4 - 1);
        let indices = random_indices(&mut rng, num_points, faces);
        let payload = sequential_payload(num_points, &indices, IndexCoding::DifferencesTagged);
        let limits = Limits::new();
        let mesh = draco::decode_with_limits(&payload, &limits).unwrap();
        assert_sequential(&mesh, num_points, &indices);
    }
}

#[test]
fn plain_indices_at_every_width() {
    let mut rng = Rng(11);
    // u8, u16, varint (65536 <= points < 2^21) and u32 storage.
    for num_points in [200u32, 60_000, 90_000, (1 << 21) + 5] {
        let indices = random_indices(&mut rng, num_points, 3_000);
        let payload = sequential_payload(num_points, &indices, IndexCoding::Plain);
        let mesh = draco::decode(&payload).unwrap();
        assert_sequential(&mesh, num_points, &indices);
        assert_eq!(mesh.bytes_consumed, payload.len());
    }
}

/// Binary decoder stream whose every bit decodes to 1: with `prob_zero` 0
/// the 1-probability is 256/256 and the state never changes (ch. 03 §4).
const ALL_ONES: [u8; 3] = [0x00, 0x01, 0x00];
/// Binary decoder stream holding a single 0 bit (`eb-quad@20..22`).
const ONE_ZERO: [u8; 3] = [0xff, 0x01, 0x11];

/// Method 1 payload for a triangle strip of `faces` faces grown by random
/// R / L symbols, with quantised positions (parallelogram prediction),
/// texture coordinates (texture-coordinate prediction) and octahedral
/// normals (geometric-normal prediction). All residuals are random, so the
/// geometry is noise; the decoding work is that of a real mesh.
///
/// With `seams`, one seam set marks every interior edge, the positions
/// stay per-vertex and the other two attributes form a per-corner group.
fn strip_payload(faces: usize, seams: bool, traversal_order: u8, rng: &mut Rng) -> Vec<u8> {
    let symbols = faces;
    let vertices = faces + 2;
    let mut out = header(1);
    out.push(0); // standard symbol coding
    varint(&mut out, vertices as u64);
    varint(&mut out, faces as u64);
    out.push(u8::from(seams));
    varint(&mut out, symbols as u64);
    varint(&mut out, 0); // split symbols
    varint(&mut out, 0); // split events
    let mut run = Bits::default();
    run.put(1, 1);
    run.put(3, 2); // E
    for _ in 1..symbols {
        run.put(1, 1);
        run.put(1 + rng.below(2), 2); // L or R
    }
    varint(&mut out, run.bytes.len() as u64);
    out.extend_from_slice(&run.bytes);
    out.extend_from_slice(&ONE_ZERO); // one boundary start face
    if seams {
        out.extend_from_slice(&ALL_ONES);
    }

    // Groups.
    let descriptors: [&[u8]; 3] = [&[0, 9, 3, 0, 0], &[3, 9, 2, 0, 1], &[1, 9, 3, 0, 2]];
    if seams {
        out.push(2);
        out.extend_from_slice(&[0xff, 0, traversal_order]);
        out.extend_from_slice(&[0, 1, 0]);
        varint(&mut out, 1);
        out.extend_from_slice(descriptors[0]);
        out.push(2);
        varint(&mut out, 2);
        out.extend_from_slice(descriptors[1]);
        out.extend_from_slice(descriptors[2]);
        out.extend_from_slice(&[2, 3]);
    } else {
        out.push(1);
        out.extend_from_slice(&[0xff, 0, traversal_order]);
        varint(&mut out, 3);
        for d in descriptors {
            out.extend_from_slice(d);
        }
        out.extend_from_slice(&[2, 2, 3]);
    }

    // Entry counts: one per vertex, or one per corner when every interior
    // edge is a seam.
    let vertex_entries = vertices;
    let other_entries = if seams { faces * 3 } else { vertices };

    // Position values: parallelogram + wrap, tagged scheme.
    out.extend_from_slice(&[1, 1, 1]);
    let residuals: Vec<u32> = (0..vertex_entries * 3)
        .map(|_| fold(rng.below(41) as i32 - 20))
        .collect();
    write_tagged_sequence(&mut out, &residuals, 3);
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&16383i32.to_le_bytes());
    let position_parameters = |out: &mut Vec<u8>| {
        for v in [0.0f32, 0.0, 0.0, 10.0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.push(14);
    };
    if seams {
        // The position group is complete: its parameters come before the
        // next group's values (ch. 05 §4).
        position_parameters(&mut out);
    }

    // Texture coordinates: method 5 + wrap, raw scheme.
    out.extend_from_slice(&[5, 1, 1]);
    let residuals: Vec<u32> = (0..other_entries * 2)
        .map(|_| fold(rng.below(7) as i32 - 3))
        .collect();
    write_raw_sequence(&mut out, &residuals, 3);
    out.extend_from_slice(&(other_entries as i32).to_le_bytes());
    out.extend_from_slice(&ALL_ONES); // orientations: all true
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&4095i32.to_le_bytes());

    // Normals: method 6 + canonicalised octahedral transform, tagged.
    out.extend_from_slice(&[6, 3, 1]);
    let residuals: Vec<u32> = (0..other_entries * 2).map(|_| rng.below(6)).collect();
    write_tagged_sequence(&mut out, &residuals, 2);
    out.extend_from_slice(&1023i32.to_le_bytes());
    out.extend_from_slice(&511i32.to_le_bytes());
    out.extend_from_slice(&ALL_ONES); // flip flags

    // Transform parameters.
    if !seams {
        position_parameters(&mut out);
    }
    for v in [0.0f32, 0.0, 1.0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.push(12);
    out.push(10);
    out
}

fn large_faces() -> usize {
    std::env::var("DRACO_SYNTH_FACES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000)
}

/// Decode `runs` times; returns `(minimum, median)` in milliseconds. The
/// minimum is the figure to compare across builds on a busy machine.
fn time_ms(payload: &[u8], limits: &Limits, runs: usize) -> (f64, f64) {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            let mesh = draco::decode_with_limits(payload, limits).unwrap();
            let dt = t.elapsed().as_secs_f64() * 1e3;
            std::hint::black_box(mesh);
            dt
        })
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (times[0], times[times.len() / 2])
}

#[test]
fn large_strip_meshes_decode() {
    let faces = large_faces();
    let limits = Limits::new()
        .with_max_points(u32::MAX)
        .with_max_faces(u32::MAX);
    for (seams, order) in [(false, 0u8), (false, 1), (true, 0), (true, 1)] {
        let mut rng = Rng(0xdead_beef ^ u64::from(order) ^ (u64::from(seams) << 8));
        let payload = strip_payload(faces, seams, order, &mut rng);
        let mesh = draco::decode_with_limits(&payload, &limits)
            .unwrap_or_else(|e| panic!("seams={seams} order={order}: {e}"));
        assert_eq!(mesh.bytes_consumed, payload.len());
        assert_eq!(mesh.faces.len(), faces);
        let points = if seams { faces * 3 } else { faces + 2 };
        assert_eq!(mesh.num_points as usize, points);
        assert_eq!(mesh.attributes.len(), 3);
        assert_eq!(mesh.attributes[0].value_count(), faces + 2);
        assert_eq!(mesh.attributes[1].value_count(), points);
        assert_eq!(mesh.attributes[2].value_count(), points);
        for a in &mesh.attributes {
            assert!(
                a.point_to_value
                    .iter()
                    .all(|&v| (v as usize) < a.value_count())
            );
        }
        // Positions stay inside the quantisation box, normals are unit.
        for p in (0..points).step_by(points / 500 + 1) {
            let pos = mesh.attributes[0].point_f32(p).unwrap();
            assert!(pos.iter().all(|v| (0.0..=10.001).contains(v)));
            let n = mesh.attributes[2].point_f32(p).unwrap();
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-4, "normal length {len}");
        }
        // Decoding is deterministic.
        let again = draco::decode_with_limits(&payload, &limits).unwrap();
        assert_eq!(again.faces, mesh.faces);
        assert_eq!(again.attributes[1].values, mesh.attributes[1].values);

        let (min, median) = time_ms(&payload, &limits, 15);
        println!(
            "strip faces={faces} seams={seams} order={order}: {} bytes, 15 runs: min {min:.3} ms, \
             median {median:.3} ms ({:.1} M faces/s)",
            payload.len(),
            faces as f64 / min / 1e3
        );
    }
}

#[test]
fn large_sequential_mesh_decodes() {
    // At most 2^17 points keeps every sign-magnitude difference below 2^18,
    // the largest raw-scheme parameter; the face count stays under a third
    // of the 12 position bytes per point (ch. 04 §1).
    let num_points = (large_faces() as u32 / 2).clamp(4, 1 << 17);
    let faces = large_faces().min(num_points as usize * 4 - 1);
    let mut rng = Rng(42);
    let indices = random_indices(&mut rng, num_points, faces);
    let limits = Limits::new()
        .with_max_points(u32::MAX)
        .with_max_faces(u32::MAX);
    for coding in [
        IndexCoding::Plain,
        IndexCoding::DifferencesTagged,
        IndexCoding::DifferencesRaw(18),
    ] {
        let payload = sequential_payload(num_points, &indices, coding);
        let mesh = draco::decode_with_limits(&payload, &limits).unwrap();
        assert_sequential(&mesh, num_points, &indices);
        let (min, median) = time_ms(&payload, &limits, 15);
        println!(
            "sequential faces={faces} {coding:?}: {} bytes, 15 runs: min {min:.3} ms, median \
             {median:.3} ms",
            payload.len()
        );
    }
}

#[test]
fn staged_fixture_decode_times() {
    // Per-fixture timing (median of 200 decodes), for the README table.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/draco");
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 38);
    for name in names {
        let payload = std::fs::read(root.join(&name).join("input.drc")).unwrap();
        let mesh = draco::decode(&payload).unwrap();
        let (min, median) = time_ms(&payload, &Limits::new(), 200);
        println!(
            "{name}: {} bytes, {} points, {} faces, 200 runs: min {:.2} µs, median {:.2} µs",
            payload.len(),
            mesh.num_points,
            mesh.faces.len(),
            min * 1e3,
            median * 1e3
        );
    }
}

// ---------------------------------------------------------------------
// Attribute paths no staged fixture executes (`spec/10-roadmap.md` §4):
// integer attributes of other widths, blocks without prediction, raw
// integers of width 4, and a mesh prediction code inside a method 0
// stream.

/// `(descriptor bytes, coding kind, value block, transform parameters)`.
type ExtraAttribute<'a> = (&'a [u8], u8, Vec<u8>, Vec<u8>);

/// Method 0 payload with `points` points, one degenerate face, a verbatim
/// position attribute (padding the stream so the face count passes the
/// ch. 04 §1 bound) and the given extra attributes: `(descriptor bytes,
/// coding kind, value block bytes, transform parameter bytes)`.
fn sequential_with_attributes(points: u32, extra: &[ExtraAttribute<'_>]) -> Vec<u8> {
    let mut out = header(0);
    varint(&mut out, 1);
    varint(&mut out, u64::from(points));
    out.push(1);
    out.extend_from_slice(&[0, 0, 0]);
    out.push(1);
    varint(&mut out, extra.len() as u64 + 1);
    out.extend_from_slice(&[0, 9, 3, 0, 0]);
    for (descriptor, ..) in extra {
        out.extend_from_slice(descriptor);
    }
    out.push(0);
    for (_, kind, ..) in extra {
        out.push(*kind);
    }
    for p in 0..points {
        for k in 1..=3u32 {
            out.extend_from_slice(&((p * k) as f32).to_le_bytes());
        }
    }
    for (_, _, values, _) in extra {
        out.extend_from_slice(values);
    }
    for (.., parameters) in extra {
        out.extend_from_slice(parameters);
    }
    out
}

#[test]
fn integer_attributes_without_prediction_and_wide_raw_integers() {
    const POINTS: u32 = 6;
    // uint8 × 4, no prediction, tagged scheme.
    let bytes: Vec<i32> = (0..POINTS as i32 * 4).map(|i| (i * 37) % 256).collect();
    let mut block_u8 = vec![0xfe, 1]; // method -2, entropy coded
    let folded: Vec<u32> = bytes.iter().map(|&v| fold(v)).collect();
    write_tagged_sequence(&mut block_u8, &folded, 4);

    // int16 × 2, no prediction, raw integers of width 4.
    let shorts: Vec<i32> = (0..POINTS as i32 * 2).map(|i| i * 5000 - 30_000).collect();
    let mut block_i16 = vec![0xfe, 0, 4];
    for &v in &shorts {
        block_i16.extend_from_slice(&fold(v).to_le_bytes());
    }

    // uint32 × 1, difference + wrap, raw integers of width 2.
    let words: Vec<i32> = vec![1000, 1003, 990, 990, 2047, 0];
    let mut block_u32 = vec![0, 1, 0, 2];
    let mut previous = 0i32;
    for &v in &words {
        block_u32.extend_from_slice(&(fold(v - previous) as u16).to_le_bytes());
        previous = v;
    }
    block_u32.extend_from_slice(&0i32.to_le_bytes());
    block_u32.extend_from_slice(&2047i32.to_le_bytes());

    let payload = sequential_with_attributes(
        POINTS,
        &[
            (&[2, 2, 4, 1, 1], 1, block_u8, Vec::new()),
            (&[4, 3, 2, 0, 2], 1, block_i16, Vec::new()),
            (&[4, 6, 1, 0, 3], 1, block_u32, Vec::new()),
        ],
    );
    let mesh = draco::decode(&payload).unwrap();
    assert_eq!(mesh.bytes_consumed, payload.len());
    assert_eq!(mesh.attributes.len(), 4);

    let a = mesh.attribute_by_unique_id(1).unwrap();
    assert!(a.normalized);
    let want: Vec<u8> = bytes.iter().map(|&v| v as u8).collect();
    assert_eq!(a.values, want);

    let a = mesh.attribute_by_unique_id(2).unwrap();
    let want: Vec<u8> = shorts
        .iter()
        .flat_map(|&v| (v as i16).to_le_bytes())
        .collect();
    assert_eq!(a.values, want);

    let a = mesh.attribute_by_unique_id(3).unwrap();
    let want: Vec<u8> = words
        .iter()
        .flat_map(|&v| (v as u32).to_le_bytes())
        .collect();
    assert_eq!(a.values, want);
}

#[test]
fn mesh_prediction_code_in_a_sequential_stream_is_difference() {
    // Ch. 05 §6.3: a method 0 stream that carries a mesh prediction code is
    // decoded with difference prediction.
    const POINTS: u32 = 5;
    let residuals: Vec<u32> = [3, 0, 7, -2, 1, 1, 0, -5, 4, 2, 2, -1, 0, 0, 9]
        .iter()
        .map(|&v| fold(v))
        .collect();
    let mut parameters = Vec::new();
    for v in [-1.0f32, 0.0, 2.0, 8.0] {
        parameters.extend_from_slice(&v.to_le_bytes());
    }
    parameters.push(10);
    let build = |method: u8| {
        let mut block = vec![method, 1, 1];
        write_raw_sequence(&mut block, &residuals, 5);
        block.extend_from_slice(&0i32.to_le_bytes());
        block.extend_from_slice(&1023i32.to_le_bytes());
        sequential_with_attributes(POINTS, &[(&[4, 9, 3, 0, 1], 2, block, parameters.clone())])
    };
    let reference = draco::decode(&build(0)).unwrap();
    for method in [1u8, 4, 5, 6] {
        let mesh = draco::decode(&build(method)).unwrap();
        assert_eq!(
            mesh.attributes[1].values, reference.attributes[1].values,
            "method {method}"
        );
    }
    // Difference prediction with the wrap transform, by hand: entry 0 is
    // (3, 0, 7); entry 1 adds (-2, 1, 1).
    let delta = 8.0f32 / 1023.0;
    let first = reference.attributes[1].point_f32(0).unwrap();
    assert_eq!(first, [3.0 * delta - 1.0, 0.0, 7.0 * delta + 2.0]);
    let second = reference.attributes[1].point_f32(1).unwrap();
    assert_eq!(second, [delta - 1.0, delta, 8.0 * delta + 2.0]);
}
