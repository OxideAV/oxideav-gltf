//! `KHR_gaussian_splatting` spherical-harmonics colour evaluator.
//!
//! The `KHR_gaussian_splatting` extension (see
//! `docs/3d/gltf/extensions/KHR_gaussian_splatting.md` §"Lighting" /
//! §"Calculating color from Spherical Harmonics" / §"Fallback
//! Behavior") stores each splat's colour as real spherical-harmonic
//! (SH) coefficients on the `KHR_gaussian_splatting:SH_DEGREE_l_COEF_n`
//! attributes. The zeroth-order coefficient is always present and
//! encodes the diffuse colour; higher degrees (1..=3) add the
//! view-dependent (specular / ambient-occlusion) component.
//!
//! The base crate already routes those attributes through the standard
//! accessor pipeline as raw `VEC3` data (see the descriptor handshake
//! in `json_to_scene.rs` / `scene_to_json.rs`). This module is the
//! pure-math layer a renderer layered above this crate calls to turn
//! the raw coefficients into a colour:
//!
//! * [`diffuse_color`] — the view-independent diffuse colour from the
//!   degree-0 coefficient alone (§"Calculating color from Spherical
//!   Harmonics", the `Color_diffuse = SH_{0,0} * 0.2820947917738781 +
//!   0.5` equation).
//! * [`color_0_fallback`] — the `COLOR_0` RGBA fallback a non-splat
//!   renderer paints onto the sparse point cloud (§"Fallback
//!   Behavior"): the degree-0 diffuse colour clamped to `[0, 1]`,
//!   carrying the splat opacity in alpha.
//! * [`evaluate`] — the full view-dependent colour from up to 45
//!   coefficients (degrees 0..=3) and a normalised view direction
//!   (§"Calculating color from Spherical Harmonics", the
//!   `Color_final` equation), using the exact §"Appendix A: Table of
//!   Constants" basis-function constants.
//!
//! All constants are transcribed verbatim from the spec's
//! §"Calculating color from Spherical Harmonics" `Color_final` listing
//! and the §"Appendix A: Table of Constants" table; no constant is
//! recomputed from `sqrt`/`PI` so the output matches the spec's stated
//! decimal literals bit-for-bit.

/// `Y_{0,0}` normalisation constant `½·√(1/π) ≈ 0.282095`, used to
/// scale the degree-0 coefficient into the diffuse colour. Verbatim
/// from the §"Calculating color from Spherical Harmonics"
/// `Color_diffuse` equation and the §"Appendix A: Table of Constants"
/// `Y_{0,0}` row. (The decimal literals carry more precision than
/// `f32` resolves; they are transcribed verbatim from the spec table
/// and `f32` rounding suffices — hence the module-level
/// `excessive_precision` allow.)
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
pub const SH_C0: f32 = 0.282_094_791_773_878_1;

/// `Y_{1,*}` basis constant `√(3/4π) ≈ 0.488603`. The negative sign on
/// the `m = -1` / `m = +1` lanes (the Condon–Shortley phase `(-1)^m`)
/// is folded into the per-lane multipliers below, matching the
/// `Color_SH_1` listing. §"Appendix A: Table of Constants".
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C1: f32 = 0.488_602_511_902_919_9;

// Degree-2 basis constants — §"Appendix A: Table of Constants".
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C2_0: f32 = 1.092_548_430_592_079; // |Y_{2,-2}| = |Y_{2,-1}| = |Y_{2,1}|
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C2_1: f32 = 0.315_391_565_252_520_0; // Y_{2,0}
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C2_2: f32 = 0.546_274_215_296_039_5; // Y_{2,2}

// Degree-3 basis constants — §"Appendix A: Table of Constants".
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C3_0: f32 = 0.590_043_589_926_643_5; // |Y_{3,-3}| = |Y_{3,3}|
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C3_1: f32 = 2.890_611_442_640_554; // Y_{3,-2}
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C3_2: f32 = 0.457_045_799_464_465_7; // |Y_{3,-1}| = |Y_{3,1}|
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C3_3: f32 = 0.373_176_332_590_115_4; // Y_{3,0}
#[allow(clippy::excessive_precision)] // spec text values; f32 round suffices
const SH_C3_4: f32 = 1.445_305_721_320_277; // Y_{3,2}

/// The `0.5` bias the forward training pass applies to the
/// zeroth-order component and the rendering pass re-applies when
/// reconstructing colour (§"Calculating color from Spherical
/// Harmonics").
pub const SH_BIAS: f32 = 0.5;

/// An RGB colour triple, linear per the glTF colour convention.
pub type Rgb = [f32; 3];

/// An RGBA colour quadruple.
pub type Rgba = [f32; 4];

/// The number of SH coefficients (each a `VEC3`) a given maximum
/// degree `l` requires: `(l + 1)^2`. Degree 0 → 1, degree 1 → 4,
/// degree 2 → 9, degree 3 → 16 (§"Lighting": `(2l + 1)` coefficients
/// per degree, summed over degrees `0..=l`).
#[must_use]
pub fn coef_count(degree: u8) -> usize {
    let d = degree as usize + 1;
    d * d
}

/// Diffuse colour from the degree-0 coefficient alone, per
/// §"Calculating color from Spherical Harmonics":
/// `Color_diffuse = SH_{0,0} · 0.2820947917738781 + 0.5`.
///
/// This is the view-independent base colour; it is *not* clamped (the
/// clamp is a `COLOR_0`-fallback concern — see [`color_0_fallback`]).
#[must_use]
pub fn diffuse_color(sh0: Rgb) -> Rgb {
    [
        sh0[0] * SH_C0 + SH_BIAS,
        sh0[1] * SH_C0 + SH_BIAS,
        sh0[2] * SH_C0 + SH_BIAS,
    ]
}

/// Colour space declared on a splat primitive's `KHR_gaussian_splatting`
/// descriptor (`colorSpace`), per §"Color Space" §"Available Color
/// Spaces". Only the two base-extension values affect the `COLOR_0`
/// fallback transfer function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    /// `"srgb_rec709_display"` — the diffuse colour is sRGB-encoded, so
    /// the `COLOR_0` fallback (which carries linear values per the glTF
    /// spec) must be sRGB-decoded to linear (§"Fallback Behavior"
    /// note).
    SrgbRec709Display,
    /// `"lin_rec709_display"` — already linear; the `COLOR_0` fallback
    /// uses the clamped diffuse colour directly.
    LinRec709Display,
}

impl ColorSpace {
    /// Parse the spec `colorSpace` string. Returns `None` for any value
    /// outside the two base-extension color spaces (a vendor extension
    /// may define more — the fallback is undefined for those here).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "srgb_rec709_display" => Some(Self::SrgbRec709Display),
            "lin_rec709_display" => Some(Self::LinRec709Display),
            _ => None,
        }
    }
}

/// Standard sRGB → linear electro-optical transfer function (IEC
/// 61966-2-1), used to decode an `srgb_rec709_display` diffuse colour
/// into the linear values the glTF `COLOR_0` attribute carries
/// (§"Fallback Behavior" note).
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The `COLOR_0` RGBA fallback colour a renderer that does not support
/// Gaussian splatting paints onto the sparse point cloud, per
/// §"Fallback Behavior".
///
/// The diffuse colour ([`diffuse_color`]) is clamped to `[0, 1]`; when
/// the splat's `colorSpace` is `srgb_rec709_display` the clamped value
/// is sRGB-decoded to linear because `COLOR_0` carries linear values
/// per the glTF spec. The splat `opacity` becomes the alpha channel.
///
/// `opacity` is clamped to `[0, 1]` too — `COLOR_0` alpha is a colour
/// channel.
#[must_use]
pub fn color_0_fallback(sh0: Rgb, opacity: f32, color_space: ColorSpace) -> Rgba {
    let diffuse = diffuse_color(sh0);
    let mut out = [0.0_f32; 4];
    for (o, d) in out.iter_mut().zip(diffuse.iter()) {
        let clamped = d.clamp(0.0, 1.0);
        *o = match color_space {
            ColorSpace::SrgbRec709Display => srgb_to_linear(clamped),
            ColorSpace::LinRec709Display => clamped,
        };
    }
    out[3] = opacity.clamp(0.0, 1.0);
    out
}

/// Evaluate the full view-dependent splat colour from its SH
/// coefficients and a view direction, per §"Calculating color from
/// Spherical Harmonics" (`Color_final`).
///
/// `coeffs` holds the `VEC3` SH coefficients packed lowest-order to
/// highest within each degree, degrees ascending — exactly the
/// attribute order `SH_DEGREE_0_COEF_0`, `SH_DEGREE_1_COEF_0..2`,
/// `SH_DEGREE_2_COEF_0..4`, `SH_DEGREE_3_COEF_0..6`. The number of
/// coefficients consumed is `coef_count(degree)`:
///
/// * degree 0 → 1 coefficient (diffuse only — equivalent to
///   [`diffuse_color`]),
/// * degree 1 → 4, degree 2 → 9, degree 3 → 16.
///
/// `degree` is clamped to the implicit degree supported by the supplied
/// slice (if fewer coefficients are passed than `coef_count(degree)`,
/// the evaluation falls back to the largest fully-supplied degree).
///
/// `dir` is the normalised viewing direction (camera → splat); the spec
/// states `r = 1` for lighting, so `dir` is used directly without
/// re-normalising. The `0.5` bias is applied once to the final sum.
///
/// The output is *not* clamped — clamping is the caller's display
/// concern.
#[must_use]
pub fn evaluate(coeffs: &[Rgb], degree: u8, dir: [f32; 3]) -> Rgb {
    // Cap the requested degree at what the slice actually carries.
    let mut deg = degree.min(3);
    while deg > 0 && coeffs.len() < coef_count(deg) {
        deg -= 1;
    }
    if coeffs.is_empty() {
        return [SH_BIAS, SH_BIAS, SH_BIAS];
    }

    let [x, y, z] = dir;
    let mut acc = [
        coeffs[0][0] * SH_C0,
        coeffs[0][1] * SH_C0,
        coeffs[0][2] * SH_C0,
    ];

    // Accumulate `coeffs[i] * weight` into `acc` for each channel.
    let mut add = |i: usize, w: f32| {
        let c = coeffs[i];
        acc[0] += c[0] * w;
        acc[1] += c[1] * w;
        acc[2] += c[2] * w;
    };

    if deg >= 1 {
        // Color_SH_1 — §"Calculating color from Spherical Harmonics".
        add(1, -SH_C1 * y);
        add(2, SH_C1 * z);
        add(3, -SH_C1 * x);
    }
    if deg >= 2 {
        // Color_SH_2.
        let (xx, yy, zz) = (x * x, y * y, z * z);
        add(4, SH_C2_0 * x * y);
        add(5, -SH_C2_0 * y * z);
        add(6, SH_C2_1 * (2.0 * zz - xx - yy));
        add(7, -SH_C2_0 * x * z);
        add(8, SH_C2_2 * (xx - yy));
    }
    if deg >= 3 {
        // Color_SH_3.
        let (xx, yy, zz) = (x * x, y * y, z * z);
        add(9, -SH_C3_0 * y * (3.0 * xx - yy));
        add(10, SH_C3_1 * x * y * z);
        add(11, -SH_C3_2 * y * (4.0 * zz - xx - yy));
        add(12, SH_C3_3 * z * (2.0 * zz - 3.0 * xx - 3.0 * yy));
        add(13, -SH_C3_2 * x * (4.0 * zz - xx - yy));
        add(14, SH_C3_4 * z * (xx - yy));
        add(15, -SH_C3_0 * x * (xx - 3.0 * yy));
    }

    [acc[0] + SH_BIAS, acc[1] + SH_BIAS, acc[2] + SH_BIAS]
}

/// One 3D Gaussian splat, reconstructed from the per-vertex
/// `KHR_gaussian_splatting` ellipse-kernel attributes (§"Ellipse Kernel"
/// §"Attributes").
///
/// The base extension stores a splat field as a `POINTS` primitive whose
/// vertex attributes carry one splat per vertex: `POSITION` (the splat
/// mean), `KHR_gaussian_splatting:ROTATION` (a unit quaternion),
/// `KHR_gaussian_splatting:SCALE` (the per-axis Gaussian spread),
/// `KHR_gaussian_splatting:OPACITY` (linear `[0, 1]` alpha), and the
/// `KHR_gaussian_splatting:SH_DEGREE_l_COEF_n` spherical-harmonics colour
/// coefficients. [`SplatField::from_attributes`] gathers those parallel
/// per-vertex arrays into a `Vec<Splat>` so a renderer above this crate
/// can iterate splats directly instead of re-deriving the per-vertex
/// indexing.
#[derive(Debug, Clone, PartialEq)]
pub struct Splat {
    /// Splat mean in the primitive's local space (the `POSITION`
    /// attribute), per §"Ellipse Kernel" §"Attributes".
    pub position: [f32; 3],
    /// Unit quaternion `[x, y, z, w]` in the usual glTF order (the
    /// `KHR_gaussian_splatting:ROTATION` attribute). The spec guarantees
    /// the stored value is already normalised (§"Ellipse Kernel"
    /// §"Attributes": "renderers can use quaternion values directly
    /// without renormalization").
    pub rotation: [f32; 4],
    /// Per-axis Gaussian spread (the `KHR_gaussian_splatting:SCALE`
    /// attribute). Linear, non-negative per §"Ellipse Kernel"
    /// §"Attributes".
    pub scale: [f32; 3],
    /// Linear opacity in `[0, 1]` (the `KHR_gaussian_splatting:OPACITY`
    /// attribute) per §"Ellipse Kernel" §"Attributes".
    pub opacity: f32,
    /// Spherical-harmonics colour coefficients (`VEC3` each) packed
    /// lowest-order `m` to highest within each degree, degrees ascending
    /// — exactly the order [`evaluate`] consumes. Index 0 is the
    /// degree-0 diffuse coefficient; for a degree-`l` field there are
    /// `coef_count(l)` entries.
    pub sh: Vec<Rgb>,
}

impl Splat {
    /// Highest spherical-harmonics degree this splat carries, derived
    /// from `self.sh.len()` (`(l + 1)^2` coefficients for degree `l`).
    /// A splat with only the mandatory degree-0 coefficient returns `0`.
    #[must_use]
    pub fn sh_degree(&self) -> u8 {
        match self.sh.len() {
            n if n >= 16 => 3,
            n if n >= 9 => 2,
            n if n >= 4 => 1,
            _ => 0,
        }
    }

    /// View-independent diffuse colour from the degree-0 coefficient
    /// (delegates to [`diffuse_color`]). Returns mid-grey when the splat
    /// somehow carries no SH coefficient.
    #[must_use]
    pub fn diffuse(&self) -> Rgb {
        match self.sh.first() {
            Some(&sh0) => diffuse_color(sh0),
            None => [SH_BIAS, SH_BIAS, SH_BIAS],
        }
    }

    /// Full view-dependent colour for a normalised view direction
    /// (delegates to [`evaluate`] over all SH coefficients this splat
    /// carries).
    #[must_use]
    pub fn color(&self, dir: [f32; 3]) -> Rgb {
        evaluate(&self.sh, self.sh_degree(), dir)
    }

    /// The `COLOR_0` RGBA fallback for this splat (delegates to
    /// [`color_0_fallback`] using the splat's degree-0 colour, its
    /// opacity, and the field's colour space).
    #[must_use]
    pub fn color_0_fallback(&self, color_space: ColorSpace) -> Rgba {
        let sh0 = self.sh.first().copied().unwrap_or([0.0; 3]);
        color_0_fallback(sh0, self.opacity, color_space)
    }
}

/// A decoded 3D Gaussian splat field — the typed counterpart of a
/// `KHR_gaussian_splatting` ellipse-kernel `POINTS` primitive.
#[derive(Debug, Clone, PartialEq)]
pub struct SplatField {
    /// Per-vertex splats, one per `POSITION` entry.
    pub splats: Vec<Splat>,
}

impl SplatField {
    /// Gather the parallel per-vertex ellipse-kernel attribute arrays
    /// into one splat per vertex, per §"Ellipse Kernel" §"Attributes".
    ///
    /// `sh_coefficients[i]` is the `VEC3` array for the `i`-th SH
    /// coefficient in `evaluate` order (index 0 = `SH_DEGREE_0_COEF_0`,
    /// index 1 = `SH_DEGREE_1_COEF_0`, …). Each inner array carries one
    /// value per vertex. The spec requires the mandatory degree-0
    /// coefficient and a per-degree completeness contract (validated
    /// upstream), so callers pass the coefficients already gathered in
    /// canonical order.
    ///
    /// Returns `None` when the parallel arrays disagree on length (any
    /// of `rotation` / `scale` / `opacity` / each SH coefficient must
    /// carry exactly one value per `position`); a well-formed,
    /// spec-validated primitive always has matching counts.
    #[must_use]
    pub fn from_attributes(
        position: &[[f32; 3]],
        rotation: &[[f32; 4]],
        scale: &[[f32; 3]],
        opacity: &[f32],
        sh_coefficients: &[Vec<Rgb>],
    ) -> Option<Self> {
        let n = position.len();
        if rotation.len() != n || scale.len() != n || opacity.len() != n {
            return None;
        }
        for coef in sh_coefficients {
            if coef.len() != n {
                return None;
            }
        }
        let mut splats = Vec::with_capacity(n);
        for i in 0..n {
            let sh: Vec<Rgb> = sh_coefficients.iter().map(|c| c[i]).collect();
            splats.push(Splat {
                position: position[i],
                rotation: rotation[i],
                scale: scale[i],
                opacity: opacity[i],
                sh,
            });
        }
        Some(Self { splats })
    }

    /// Reconstruct a [`SplatField`] from a primitive's `POSITION`
    /// attribute and the `primitive.extras["__gaussian_splats"]` sidecar
    /// record the decoder parks for an `"ellipse"`-kernel splat
    /// primitive.
    ///
    /// The sidecar is a JSON object with keys `rotation` (array of
    /// 4-arrays), `scale` (array of 3-arrays), `opacity` (array of
    /// numbers), and `sh` (array of per-coefficient arrays of 3-arrays,
    /// in [`evaluate`] order). `positions` is the primitive's own
    /// `POSITION` data — it is *not* duplicated in the sidecar.
    ///
    /// Returns `None` when the sidecar is malformed or the parallel
    /// arrays disagree on length.
    #[must_use]
    pub fn from_extras(positions: &[[f32; 3]], sidecar: &serde_json::Value) -> Option<Self> {
        let obj = sidecar.as_object()?;
        let rotation = read_vec4_array(obj.get("rotation")?)?;
        let scale = read_vec3_array(obj.get("scale")?)?;
        let opacity: Vec<f32> = obj
            .get("opacity")?
            .as_array()?
            .iter()
            .map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Option<Vec<f32>>>()?;
        let sh_outer = obj.get("sh")?.as_array()?;
        let sh: Vec<Vec<Rgb>> = sh_outer
            .iter()
            .map(read_vec3_array)
            .collect::<Option<Vec<Vec<Rgb>>>>()?;
        Self::from_attributes(positions, &rotation, &scale, &opacity, &sh)
    }

    /// Number of splats in the field.
    #[must_use]
    pub fn len(&self) -> usize {
        self.splats.len()
    }

    /// Whether the field carries no splats.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.splats.is_empty()
    }
}

/// Parse a JSON array of 3-element numeric arrays into `Vec<[f32; 3]>`.
fn read_vec3_array(v: &serde_json::Value) -> Option<Vec<[f32; 3]>> {
    v.as_array()?
        .iter()
        .map(|e| {
            let a = e.as_array()?;
            if a.len() != 3 {
                return None;
            }
            Some([
                a[0].as_f64()? as f32,
                a[1].as_f64()? as f32,
                a[2].as_f64()? as f32,
            ])
        })
        .collect()
}

/// Parse a JSON array of 4-element numeric arrays into `Vec<[f32; 4]>`.
fn read_vec4_array(v: &serde_json::Value) -> Option<Vec<[f32; 4]>> {
    v.as_array()?
        .iter()
        .map(|e| {
            let a = e.as_array()?;
            if a.len() != 4 {
                return None;
            }
            Some([
                a[0].as_f64()? as f32,
                a[1].as_f64()? as f32,
                a[2].as_f64()? as f32,
                a[3].as_f64()? as f32,
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for f32 accumulation across the SH series.
    const EPS: f32 = 1e-6;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPS
    }

    fn rgb_close(a: Rgb, b: Rgb) -> bool {
        a.iter().zip(b.iter()).all(|(&x, &y)| close(x, y))
    }

    #[test]
    fn coef_count_matches_spec_per_degree() {
        // §"Lighting": 1 / 4 / 9 / 16 cumulative coefficients.
        assert_eq!(coef_count(0), 1);
        assert_eq!(coef_count(1), 4);
        assert_eq!(coef_count(2), 9);
        assert_eq!(coef_count(3), 16);
    }

    #[test]
    fn diffuse_color_uses_spec_equation() {
        // Color_diffuse = SH_{0,0} * 0.2820947917738781 + 0.5.
        let sh0 = [1.0, -2.0, 0.0];
        let got = diffuse_color(sh0);
        assert!(close(got[0], 1.0 * SH_C0 + 0.5));
        assert!(close(got[1], -2.0 * SH_C0 + 0.5));
        assert!(close(got[2], 0.5));
    }

    #[test]
    fn zero_coefficients_give_mid_grey() {
        // SH all zero → only the 0.5 bias survives.
        assert!(rgb_close(diffuse_color([0.0; 3]), [0.5; 3]));
        assert!(rgb_close(
            evaluate(&[[0.0; 3]], 0, [0.0, 0.0, 1.0]),
            [0.5; 3]
        ));
    }

    #[test]
    fn evaluate_degree0_equals_diffuse_color() {
        // With degree 0 only, evaluate() reduces to diffuse_color()
        // (no view-dependent terms).
        let sh0 = [0.3, -0.7, 1.2];
        let coeffs = [sh0];
        for dir in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            assert!(rgb_close(evaluate(&coeffs, 0, dir), diffuse_color(sh0)));
        }
    }

    #[test]
    fn degree1_term_matches_hand_computed() {
        // Color_SH_1 for a unit-X view direction: only the m=+1 lane
        // (SH_1_1 * x * -SH_C1) contributes; m=-1 (·y) and m=0 (·z)
        // vanish.
        let sh0 = [0.0; 3];
        let sh1_m1 = [1.0, 1.0, 1.0];
        let sh1_0 = [1.0, 1.0, 1.0];
        let sh1_p1 = [1.0, 1.0, 1.0];
        let coeffs = [sh0, sh1_m1, sh1_0, sh1_p1];
        let dir = [1.0, 0.0, 0.0];
        let got = evaluate(&coeffs, 1, dir);
        // acc = 0 (deg0) + (-SH_C1 * 1.0) on m=+1 lane, + bias.
        let expect = -SH_C1 + SH_BIAS;
        assert!(rgb_close(got, [expect; 3]), "{got:?}");
    }

    #[test]
    fn degree_capped_to_supplied_coefficients() {
        // Asking for degree 3 with only a degree-0 slice must fall back
        // to degree 0 rather than index out of bounds.
        let coeffs = [[0.4, 0.4, 0.4]];
        let got = evaluate(&coeffs, 3, [0.577, 0.577, 0.577]);
        assert!(rgb_close(got, diffuse_color([0.4, 0.4, 0.4])));
    }

    #[test]
    fn empty_coefficients_give_mid_grey() {
        assert!(rgb_close(evaluate(&[], 0, [0.0, 0.0, 1.0]), [0.5; 3]));
    }

    #[test]
    fn color_0_fallback_linear_clamps_and_carries_opacity() {
        // lin_rec709_display: clamped diffuse, no transfer decode.
        // sh0 chosen so the diffuse colour straddles [0,1].
        let bright = (1.5 - 0.5) / SH_C0; // diffuse = 1.5 → clamp to 1.0
        let dark = (-0.5 - 0.5) / SH_C0; // diffuse = -0.5 → clamp to 0.0
        let sh0 = [bright, dark, 0.0]; // mid channel diffuse = 0.5
        let got = color_0_fallback(sh0, 0.8, ColorSpace::LinRec709Display);
        assert!(close(got[0], 1.0));
        assert!(close(got[1], 0.0));
        assert!(close(got[2], 0.5));
        assert!(close(got[3], 0.8));
    }

    #[test]
    fn color_0_fallback_srgb_decodes_to_linear() {
        // srgb_rec709_display: the clamped diffuse colour is sRGB and
        // must be decoded to linear. A mid-grey diffuse 0.5 sRGB decodes
        // to ~0.214.
        let sh0 = [0.0, 0.0, 0.0]; // diffuse = 0.5 on every channel
        let got = color_0_fallback(sh0, 1.0, ColorSpace::SrgbRec709Display);
        let expect = srgb_to_linear(0.5);
        assert!(rgb_close([got[0], got[1], got[2]], [expect; 3]));
        // sRGB 0.5 → linear must be well below 0.5.
        assert!(got[0] < 0.25 && got[0] > 0.2);
        assert!(close(got[3], 1.0));
    }

    #[test]
    fn color_0_fallback_opacity_clamped() {
        let got = color_0_fallback([0.0; 3], 1.7, ColorSpace::LinRec709Display);
        assert!(close(got[3], 1.0));
        let got = color_0_fallback([0.0; 3], -0.3, ColorSpace::LinRec709Display);
        assert!(close(got[3], 0.0));
    }

    #[test]
    fn srgb_transfer_endpoints() {
        // sRGB transfer is identity at 0 and 1.
        assert!(close(srgb_to_linear(0.0), 0.0));
        assert!(close(srgb_to_linear(1.0), 1.0));
    }

    #[test]
    fn color_space_parse_round_trip() {
        assert_eq!(
            ColorSpace::parse("srgb_rec709_display"),
            Some(ColorSpace::SrgbRec709Display)
        );
        assert_eq!(
            ColorSpace::parse("lin_rec709_display"),
            Some(ColorSpace::LinRec709Display)
        );
        assert_eq!(ColorSpace::parse("xyz"), None);
    }

    #[test]
    fn full_degree3_is_finite_and_symmetric_at_z_axis() {
        // A z-axis view with all-equal coefficients: degree-1 m=0 lane
        // (·z·+SH_C1) is the only odd-degree contributor along z; the
        // result must be finite and equal across channels.
        let coeffs: Vec<Rgb> = (0..16).map(|_| [0.1, 0.1, 0.1]).collect();
        let got = evaluate(&coeffs, 3, [0.0, 0.0, 1.0]);
        assert!(got.iter().all(|c| c.is_finite()));
        assert!(close(got[0], got[1]) && close(got[1], got[2]));
    }
}
