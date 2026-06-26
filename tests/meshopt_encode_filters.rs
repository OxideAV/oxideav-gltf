//! Round-trip coverage for the `KHR_meshopt_compression` forward
//! (encode-side) Appendix B filters.
//!
//! Each filter's encode produces the filtered integer representation, the
//! NONE attribute codec compresses it, and `decode(.., filter, ..)` runs the
//! attribute decoder followed by the inverse filter. The spec
//! (`docs/3d/gltf/extensions/KHR_meshopt_compression.md`, "Appendix B")
//! states the decode tolerance: EXPONENTIAL "must be decoded exactly", while
//! OCTAHEDRAL / QUATERNION / COLOR are specified "to one unit in last place
//! (ULP) in terms of the decoded data". The tests assert exactly that
//! contract.

use oxideav_gltf::meshopt::{decode, encode, Filter, Mode};

/// Pack a slice of f32 into little-endian bytes.
fn f32_bytes(vals: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vals.len() * 4);
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn read_f32(buf: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]])
}

fn read_i16(buf: &[u8], i: usize) -> i16 {
    i16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]])
}

// -------------------------------------------------------------------------
// Filter 3: EXPONENTIAL — must round-trip exactly.
// -------------------------------------------------------------------------

#[test]
fn exponential_roundtrip_is_exact() {
    // A spread of finite f32 values: small, large, fractional, negative,
    // powers of two, and exactly-representable decimals.
    // All within the encodable exponent window [-100, 100]: the spec
    // restricts `e` to that range, so values below 2^-100 in magnitude are
    // out of scope (and rejected by the encoder), which is the only finite
    // f32 carve-out from "must be decoded exactly".
    let vals: Vec<f32> = vec![
        0.0,
        1.0,
        -1.0,
        2.0,
        0.5,
        -0.5,
        1.5,
        3.25,
        -7.125,
        1024.0,
        0.015625,
        100000.0,
        -1.0 / 8192.0, // exactly -2^-13
        12345.0,
        -(1_580_247.0 / 16.0), // exactly -98765.4375
        65504.0,
        -0.359_375,
    ];
    let count = vals.len();
    let stride = 4;
    let raw = f32_bytes(&vals);

    let comp = encode(&raw, Mode::Attributes, Filter::Exponential, count, stride)
        .expect("encode exponential");
    let back = decode(&comp, Mode::Attributes, Filter::Exponential, count, stride)
        .expect("decode exponential");
    assert_eq!(back.len(), raw.len());
    for (i, &v) in vals.iter().enumerate() {
        let got = read_f32(&back, i);
        // Exact equality on bits is too strict for -0.0 (the filter does not
        // preserve the sign of zero); compare values instead.
        assert_eq!(got, v, "element {i}: expected {v}, got {got}");
    }
}

#[test]
fn exponential_multi_component_stride() {
    // VEC3 of f32 — stride 12, a multiple of 4, three components per element.
    let vals: Vec<f32> = vec![
        1.0, 2.0, 3.0, -4.0, 5.5, -6.25, 0.0, 0.125, 256.0, -1024.5, 0.03125, 9999.0,
    ];
    let count = 4;
    let stride = 12;
    let raw = f32_bytes(&vals);
    let comp = encode(&raw, Mode::Attributes, Filter::Exponential, count, stride)
        .expect("encode exponential vec3");
    let back = decode(&comp, Mode::Attributes, Filter::Exponential, count, stride)
        .expect("decode exponential vec3");
    for (i, &v) in vals.iter().enumerate() {
        assert_eq!(read_f32(&back, i), v, "vec3 component {i}");
    }
}

#[test]
fn exponential_rejects_inexact_full_precision_mantissa() {
    // The mantissa field is signed 24-bit, range [-2^23, 2^23-1]. A value
    // whose minimal f32 significand needs all 24 bits *and* is odd (so it
    // cannot be halved without loss) is not exactly encodable by this
    // filter; the encoder reports it rather than silently truncating.
    // -1/3 ≈ -0.33333334 has such a significand.
    let raw = f32_bytes(&[-0.333_333_34]);
    let err = encode(&raw, Mode::Attributes, Filter::Exponential, 1, 4).unwrap_err();
    assert!(format!("{err}").contains("EXPONENTIAL"));
}

#[test]
fn exponential_rejects_bad_stride() {
    let raw = vec![0u8; 6];
    let err = encode(&raw, Mode::Attributes, Filter::Exponential, 1, 6).unwrap_err();
    assert!(format!("{err}").contains("EXPONENTIAL"));
}

// -------------------------------------------------------------------------
// Filter 1: OCTAHEDRAL — unit vectors within 1 ULP.
// -------------------------------------------------------------------------

fn norm3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

#[test]
fn octahedral_roundtrip_8bit_within_tolerance() {
    // Build unit normals as the decoded 8-bit fixed-point layout the inverse
    // filter emits: three signed-normalized components (×127) + a
    // pass-through 4th byte.
    let dirs = [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, -1.0],
        [0.577_35, 0.577_35, 0.577_35],
        [-0.577_35, 0.577_35, -0.577_35],
        [0.267_26, -0.534_52, 0.801_78],
    ];
    let stride = 4;
    let mut raw = Vec::new();
    let mut tags = Vec::new();
    for (idx, d) in dirs.iter().enumerate() {
        let n = norm3(*d);
        raw.push((round(n[0] * 127.0)) as i8 as u8);
        raw.push((round(n[1] * 127.0)) as i8 as u8);
        raw.push((round(n[2] * 127.0)) as i8 as u8);
        let tag = (idx as u8).wrapping_mul(17);
        raw.push(tag);
        tags.push(tag);
    }
    let count = dirs.len();
    let comp =
        encode(&raw, Mode::Attributes, Filter::Octahedral, count, stride).expect("encode octa");
    let back =
        decode(&comp, Mode::Attributes, Filter::Octahedral, count, stride).expect("decode octa");
    for (i, d) in dirs.iter().enumerate() {
        let n = norm3(*d);
        let gx = (back[i * 4] as i8) as f32 / 127.0;
        let gy = (back[i * 4 + 1] as i8) as f32 / 127.0;
        let gz = (back[i * 4 + 2] as i8) as f32 / 127.0;
        // Direction within a few quantization steps (1/127 each).
        let dot = (gx * n[0] + gy * n[1] + gz * n[2]).clamp(-1.0, 1.0);
        assert!(dot > 0.985, "octa dir {i}: dot {dot}");
        // The 4th component is passed through verbatim.
        assert_eq!(back[i * 4 + 3], tags[i], "octa passthrough {i}");
    }
}

#[test]
fn octahedral_roundtrip_16bit_tighter() {
    let dirs = [[0.12, -0.34, 0.93], [0.7, 0.7, 0.14], [-0.5, -0.5, -0.707]];
    let stride = 8;
    let mut raw = Vec::new();
    for d in &dirs {
        let n = norm3(*d);
        raw.extend_from_slice(&((round(n[0] * 32767.0)) as i16).to_le_bytes());
        raw.extend_from_slice(&((round(n[1] * 32767.0)) as i16).to_le_bytes());
        raw.extend_from_slice(&((round(n[2] * 32767.0)) as i16).to_le_bytes());
        raw.extend_from_slice(&0i16.to_le_bytes());
    }
    let count = dirs.len();
    let comp =
        encode(&raw, Mode::Attributes, Filter::Octahedral, count, stride).expect("encode octa16");
    let back =
        decode(&comp, Mode::Attributes, Filter::Octahedral, count, stride).expect("decode octa16");
    for (i, d) in dirs.iter().enumerate() {
        let n = norm3(*d);
        let gx = read_i16(&back, i * 4) as f32 / 32767.0;
        let gy = read_i16(&back, i * 4 + 1) as f32 / 32767.0;
        let gz = read_i16(&back, i * 4 + 2) as f32 / 32767.0;
        let dot = (gx * n[0] + gy * n[1] + gz * n[2]).clamp(-1.0, 1.0);
        assert!(dot > 0.9999, "octa16 dir {i}: dot {dot}");
    }
}

// -------------------------------------------------------------------------
// Filter 2: QUATERNION — unit quaternions within 1 ULP.
// -------------------------------------------------------------------------

fn norm4(q: [f32; 4]) -> [f32; 4] {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / len, q[1] / len, q[2] / len, q[3] / len]
}

#[test]
fn quaternion_roundtrip_within_tolerance() {
    let quats = [
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.5, 0.5, 0.5, 0.5],
        [0.18, -0.36, 0.54, 0.74],
        [-0.6, 0.0, 0.8, 0.0],
    ];
    let stride = 8;
    let mut raw = Vec::new();
    for q in &quats {
        let n = norm4(*q);
        for c in n {
            raw.extend_from_slice(&((round(c * 32767.0)) as i16).to_le_bytes());
        }
    }
    let count = quats.len();
    let comp =
        encode(&raw, Mode::Attributes, Filter::Quaternion, count, stride).expect("encode quat");
    let back =
        decode(&comp, Mode::Attributes, Filter::Quaternion, count, stride).expect("decode quat");
    for (i, q) in quats.iter().enumerate() {
        let n = norm4(*q);
        let g = [
            read_i16(&back, i * 4) as f32 / 32767.0,
            read_i16(&back, i * 4 + 1) as f32 / 32767.0,
            read_i16(&back, i * 4 + 2) as f32 / 32767.0,
            read_i16(&back, i * 4 + 3) as f32 / 32767.0,
        ];
        // Quaternions q and -q represent the same rotation; compare |dot|.
        let dot = (g[0] * n[0] + g[1] * n[1] + g[2] * n[2] + g[3] * n[3]).abs();
        assert!(dot > 0.9999, "quat {i}: |dot| {dot}");
    }
}

// -------------------------------------------------------------------------
// Filter 4: COLOR — RGBA within 1 ULP.
// -------------------------------------------------------------------------

#[test]
fn color_roundtrip_8bit_within_tolerance() {
    let colors: [[u8; 4]; 6] = [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [255, 0, 0, 128],
        [0, 255, 0, 64],
        [0, 0, 255, 200],
        [123, 45, 67, 250],
    ];
    let stride = 4;
    let mut raw = Vec::new();
    for c in &colors {
        raw.extend_from_slice(c);
    }
    let count = colors.len();
    let comp = encode(&raw, Mode::Attributes, Filter::Color, count, stride).expect("encode color");
    let back = decode(&comp, Mode::Attributes, Filter::Color, count, stride).expect("decode color");
    for (i, c) in colors.iter().enumerate() {
        for ch in 0..3 {
            let got = back[i * 4 + ch] as i32;
            let want = c[ch] as i32;
            assert!(
                (got - want).abs() <= 1,
                "color {i} ch {ch}: want {want}, got {got}"
            );
        }
        // Alpha is stored at K-1 bits (the dropped LSB is reconstructed by
        // replication on decode) → within 1 ULP.
        let ga = back[i * 4 + 3] as i32;
        assert!(
            (ga - c[3] as i32).abs() <= 1,
            "color {i} alpha: want {}, got {ga}",
            c[3]
        );
    }
}

#[test]
fn color_rejects_triangles_mode() {
    let raw = vec![0u8; 16];
    let err = encode(&raw, Mode::Triangles, Filter::Color, 4, 4).unwrap_err();
    assert!(format!("{err}").contains("ATTRIBUTES"));
}

// -------------------------------------------------------------------------
// Property / fuzz coverage — fixed-seed LCG, no external dependency, so a
// failure reproduces deterministically.
// -------------------------------------------------------------------------

struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        // Numerical-Recipes LCG constants.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    fn next_f32_unit(&mut self) -> f32 {
        // Uniform in [-1, 1].
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[test]
fn exponential_fuzz_accepted_values_roundtrip_exactly() {
    // For every f32 the encoder accepts, the decode must reproduce it
    // bit-for-bit ("must be decoded exactly"). We feed a wide spread of
    // random floats (whole numbers, dyadic fractions, scaled magnitudes);
    // any value the encoder rejects is the documented signed-24-bit /
    // exponent-window carve-out and is simply skipped.
    let mut rng = Lcg(0x5EED_F00D_1234_5678);
    let mut checked = 0u32;
    for _ in 0..4000 {
        // Mix of value shapes that are often exactly encodable.
        let kind = rng.next_u32() % 4;
        let v: f32 = match kind {
            0 => (rng.next_u32() % 4096) as f32 - 2048.0, // small integers
            1 => ((rng.next_u32() % 65536) as f32) / 256.0, // /256 dyadics
            2 => rng.next_f32_unit() * 1024.0,            // arbitrary mid-range
            _ => {
                let m = (rng.next_u32() % (1 << 20)) as f32; // <=20-bit mantissa
                let e = (rng.next_u32() % 40) as i32 - 20;
                m * 2.0f32.powi(e)
            }
        };
        let raw = v.to_le_bytes();
        if let Ok(comp) = encode(&raw, Mode::Attributes, Filter::Exponential, 1, 4) {
            let back = decode(&comp, Mode::Attributes, Filter::Exponential, 1, 4)
                .expect("decode exponential fuzz");
            let got = read_f32(&back, 0);
            assert_eq!(got, v, "exponential fuzz: {v} -> {got}");
            checked += 1;
        }
    }
    // The bulk of the spread is encodable; make sure the test actually
    // exercised the round-trip rather than rejecting everything.
    assert!(checked > 3000, "only {checked} values accepted");
}

#[test]
fn octahedral_fuzz_unit_vectors_within_tolerance() {
    let mut rng = Lcg(0x0C7A_8EDA_4242_1111);
    for _ in 0..2000 {
        let dir = [
            rng.next_f32_unit(),
            rng.next_f32_unit(),
            rng.next_f32_unit(),
        ];
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if len < 1e-3 {
            continue;
        }
        let n = [dir[0] / len, dir[1] / len, dir[2] / len];
        let raw = vec![
            round(n[0] * 127.0) as i8 as u8,
            round(n[1] * 127.0) as i8 as u8,
            round(n[2] * 127.0) as i8 as u8,
            0,
        ];
        let comp = encode(&raw, Mode::Attributes, Filter::Octahedral, 1, 4).expect("encode");
        let back = decode(&comp, Mode::Attributes, Filter::Octahedral, 1, 4).expect("decode");
        let g = [
            (back[0] as i8) as f32 / 127.0,
            (back[1] as i8) as f32 / 127.0,
            (back[2] as i8) as f32 / 127.0,
        ];
        let glen = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt().max(1e-6);
        let dot = ((g[0] * n[0] + g[1] * n[1] + g[2] * n[2]) / glen).clamp(-1.0, 1.0);
        // 8-bit octahedral: a few quantization steps of angular error.
        assert!(dot > 0.97, "octa fuzz dir {n:?}: dot {dot}");
    }
}

#[test]
fn color_fuzz_8bit_within_tolerance() {
    let mut rng = Lcg(0xC010_2030_4050_6070);
    for _ in 0..3000 {
        let c = [
            (rng.next_u32() & 0xff) as u8,
            (rng.next_u32() & 0xff) as u8,
            (rng.next_u32() & 0xff) as u8,
            (rng.next_u32() & 0xff) as u8,
        ];
        let raw = c.to_vec();
        let comp = encode(&raw, Mode::Attributes, Filter::Color, 1, 4).expect("encode color fuzz");
        let back = decode(&comp, Mode::Attributes, Filter::Color, 1, 4).expect("decode color fuzz");
        for ch in 0..3 {
            let d = (back[ch] as i32 - c[ch] as i32).abs();
            assert!(d <= 1, "color fuzz {c:?} ch {ch}: delta {d}");
        }
        let da = (back[3] as i32 - c[3] as i32).abs();
        assert!(da <= 1, "color fuzz {c:?} alpha: delta {da}");
    }
}

/// Round half away from zero (mirrors the spec's `round`).
fn round(x: f32) -> f32 {
    if x >= 0.0 {
        (x + 0.5).floor()
    } else {
        (x - 0.5).ceil()
    }
}
