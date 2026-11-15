//! Prediction transforms and predictors of integer value blocks:
//! the wrap transform and difference prediction
//! (`docs/3d/gltf/draco/spec/05-attributes.md` §6.4–§6.5) and the
//! mesh-based predictors with the canonicalised octahedral transform
//! (`docs/3d/gltf/draco/spec/08-mesh-prediction.md`).

use super::edgebreaker::{NIL, for_each_corner_of_vertex, next, prev, swing_left, swing_right};
use super::entropy::BitDecoder;
use super::reader::Reader;
use super::sequencing::{Sequence, View};
use crate::error::{Result, invalid};

/// Wrap transform parameters (ch. 05 §6.4).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wrap {
    min: i32,
    max: i32,
    span: i64,
}

impl Wrap {
    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self> {
        let min = r.i32()?;
        let max = r.i32()?;
        if max < min {
            return Err(invalid("draco: wrap transform with max < min"));
        }
        Ok(Self {
            min,
            max,
            span: i64::from(max) - i64::from(min) + 1,
        })
    }

    #[inline]
    fn combine(&self, pred: i32, res: i32) -> i32 {
        let p = pred.clamp(self.min, self.max);
        let mut v = i64::from(p) + i64::from(res);
        if v > i64::from(self.max) {
            v -= self.span;
        } else if v < i64::from(self.min) {
            v += self.span;
        }
        v as i32
    }
}

/// Canonicalised octahedral transform parameters (ch. 08 §6.1).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Octahedral {
    max_q: i32,
    /// `M = max_q - 1`.
    m: i32,
    /// `center = M div 2`.
    center: i32,
}

impl Octahedral {
    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self> {
        let max_q = r.i32()?;
        let _center_stored = r.i32()?;
        let bit_length = 32 - max_q.leading_zeros();
        if max_q < 0 || max_q % 2 == 0 || !(2..=30).contains(&bit_length) {
            return Err(invalid("draco: bad octahedral transform range"));
        }
        let m = max_q - 1;
        Ok(Self {
            max_q,
            m,
            center: m / 2,
        })
    }

    /// The *invert* map of §6.2.
    fn invert(&self, a: i32, b: i32) -> (i32, i32) {
        let (sa, sb) = if a >= 0 && b >= 0 {
            (1, 1)
        } else if a <= 0 && b <= 0 {
            (-1, -1)
        } else {
            (if a > 0 { 1 } else { -1 }, if b > 0 { 1 } else { -1 })
        };
        let ca = sa * self.center;
        let cb = sb * self.center;
        let mut ua = a.wrapping_mul(2).wrapping_sub(ca);
        let mut ub = b.wrapping_mul(2).wrapping_sub(cb);
        if sa * sb >= 0 {
            (ua, ub) = (ub.wrapping_neg(), ua.wrapping_neg());
        } else {
            (ua, ub) = (ub, ua);
        }
        ua = ua.wrapping_add(ca);
        ub = ub.wrapping_add(cb);
        (ua / 2, ub / 2)
    }

    fn fold(&self, v: i32) -> i32 {
        if v > self.center {
            v.wrapping_sub(self.max_q)
        } else if v < -self.center {
            v.wrapping_add(self.max_q)
        } else {
            v
        }
    }

    /// Combine a prediction with a (non-negative) residual (§6.2).
    fn combine(&self, pred: [i32; 2], res: [i32; 2]) -> [i32; 2] {
        let mut ps = pred[0].wrapping_sub(self.center);
        let mut pt = pred[1].wrapping_sub(self.center);
        let in_diamond = i64::from(ps).abs() + i64::from(pt).abs() <= i64::from(self.center);
        if !in_diamond {
            (ps, pt) = self.invert(ps, pt);
        }
        let in_bottom_left = (ps == 0 && pt == 0) || (ps < 0 && pt <= 0);
        let rot = match (ps.signum(), pt.signum()) {
            (0, 0) => 0,
            (0, 1) => 3,
            (0, _) => 1,
            (1, -1) => 1,
            (1, _) => 2,
            (_, 1) => 3,
            _ => 0,
        };
        if !in_bottom_left {
            (ps, pt) = rotate(ps, pt, rot);
        }
        let mut os = self.fold(ps.wrapping_add(res[0]));
        let mut ot = self.fold(pt.wrapping_add(res[1]));
        if !in_bottom_left {
            (os, ot) = rotate(os, ot, (4 - rot) % 4);
        }
        if !in_diamond {
            (os, ot) = self.invert(os, ot);
        }
        [os.wrapping_add(self.center), ot.wrapping_add(self.center)]
    }
}

fn rotate(a: i32, b: i32, n: u32) -> (i32, i32) {
    match n {
        0 => (a, b),
        1 => (b, a.wrapping_neg()),
        2 => (a.wrapping_neg(), b.wrapping_neg()),
        _ => (b.wrapping_neg(), a),
    }
}

/// Transform combined with a predictor.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Transform {
    Wrap(Wrap),
    Octahedral(Octahedral),
}

impl Transform {
    /// Replace the residual tuple `d` by `combine(pred, d)`.
    #[inline]
    fn apply(&self, pred: &[i32], d: &mut [i32]) {
        match self {
            Transform::Wrap(w) => {
                for (v, &p) in d.iter_mut().zip(pred) {
                    *v = w.combine(p, *v);
                }
            }
            Transform::Octahedral(o) => {
                let out = o.combine([pred[0], pred[1]], [d[0], d[1]]);
                d[..2].copy_from_slice(&out);
            }
        }
    }
}

/// Tables of the attribute group being reconstructed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshRefs<'a> {
    pub(crate) view: View<'a>,
    pub(crate) seq: &'a Sequence,
}

impl MeshRefs<'_> {
    /// `vertex_entry[V(c)]`; `NIL` (greater than any entry) when unvisited.
    #[inline]
    fn entry_of(&self, c: u32) -> u32 {
        self.seq.vertex_entry[self.view.vertex[c as usize] as usize]
    }
}

/// Integer positions reachable from a group's entries (ch. 07 §5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Positions<'a> {
    /// Reconstructed integers of the position attribute, 3 per value.
    pub(crate) values: &'a [i32],
    /// Point-to-value map of the position attribute.
    pub(crate) point_to_value: &'a [u32],
}

impl Positions<'_> {
    /// `pos_of_entry(e)` for an entry of the group described by `seq`.
    #[inline]
    fn of_entry(&self, seq: &Sequence, e: u32) -> Option<[i64; 3]> {
        let p = *seq.entry_point.get(e as usize)?;
        let i = *self.point_to_value.get(p as usize)? as usize * 3;
        let v = self.values.get(i..i + 3)?;
        Some([i64::from(v[0]), i64::from(v[1]), i64::from(v[2])])
    }
}

fn missing_position() -> crate::error::Error {
    invalid("draco: predictor needs a position that is not available")
}

/// Difference prediction (ch. 05 §6.5).
pub(crate) fn difference(d: &mut [i32], c: usize, tf: &Transform) {
    if c == 0 || d.is_empty() {
        return;
    }
    let zero = vec![0i32; c];
    tf.apply(&zero, &mut d[..c]);
    for e in 1..d.len() / c {
        let (before, rest) = d.split_at_mut(e * c);
        tf.apply(&before[(e - 1) * c..], &mut rest[..c]);
    }
}

/// Parallelogram rule (ch. 08 §1): writes the prediction for entry `e`
/// across the edge opposite `corner` into `pred` and returns `true`, or
/// returns `false` when there is none.
#[inline]
fn parallelogram_rule(
    done: &[i32],
    c: usize,
    mesh: &MeshRefs<'_>,
    e: u32,
    corner: u32,
    pred: &mut [i32],
) -> bool {
    let o = mesh.view.opp[corner as usize];
    if o == NIL {
        return false;
    }
    let a = mesh.entry_of(o);
    let b = mesh.entry_of(next(o));
    let far = mesh.entry_of(prev(o));
    if a >= e || b >= e || far >= e {
        return false;
    }
    let (a, b, far) = (a as usize * c, b as usize * c, far as usize * c);
    for (i, p) in pred.iter_mut().enumerate() {
        *p = (i64::from(done[b + i]) + i64::from(done[far + i]) - i64::from(done[a + i])) as i32;
    }
    true
}

/// Parallelogram prediction (ch. 08 §2).
pub(crate) fn parallelogram(
    d: &mut [i32],
    c: usize,
    mesh: &MeshRefs<'_>,
    tf: &Transform,
    trace: &mut Option<Vec<String>>,
) {
    if c == 0 || d.is_empty() {
        return;
    }
    let mut pred = vec![0i32; c];
    tf.apply(&pred, &mut d[..c]);
    for e in 1..d.len() / c {
        let corner = mesh.seq.entry_corner[e];
        if let Some(t) = trace.as_mut() {
            t.push(format!("para entry={e} corner={corner}"));
        }
        let (before, rest) = d.split_at_mut(e * c);
        if parallelogram_rule(before, c, mesh, e as u32, corner, &mut pred) {
            tf.apply(&pred, &mut rest[..c]);
        } else {
            tf.apply(&before[(e - 1) * c..], &mut rest[..c]);
        }
    }
}

/// One crease-flag context of the constrained multi-parallelogram
/// predictor: a counted binary stream decoded on demand.
#[derive(Debug)]
pub(crate) struct FlagStream<'a> {
    decoder: Option<BitDecoder<'a>>,
    left: usize,
}

impl<'a> FlagStream<'a> {
    /// `varint num_flags`, then a binary decoder unless the count is 0.
    pub(crate) fn read(r: &mut Reader<'a>) -> Result<Self> {
        let left = r.count()?;
        let decoder = if left == 0 {
            None
        } else {
            Some(BitDecoder::read(r)?)
        };
        Ok(Self { decoder, left })
    }

    fn next(&mut self) -> Result<bool> {
        match &mut self.decoder {
            Some(d) if self.left > 0 => {
                self.left -= 1;
                Ok(d.bit())
            }
            _ => Err(invalid("draco: crease flags exhausted")),
        }
    }
}

/// Constrained multi-parallelogram prediction (ch. 08 §3).
pub(crate) fn constrained_multi_parallelogram(
    d: &mut [i32],
    c: usize,
    mesh: &MeshRefs<'_>,
    flags: &mut [FlagStream<'_>; 4],
    tf: &Transform,
    trace: &mut Option<Vec<String>>,
) -> Result<()> {
    if c == 0 || d.is_empty() {
        return Ok(());
    }
    let mut candidates = vec![0i32; 4 * c];
    let mut pred = vec![0i32; c];
    tf.apply(&pred, &mut d[..c]);
    let opp = mesh.view.opp;
    for e in 1..d.len() / c {
        let start = mesh.seq.entry_corner[e];
        let (before, rest) = d.split_at_mut(e * c);
        // Collect up to four candidates, swinging left then right.
        let mut n = 0usize;
        let mut corner = start;
        let mut leftwards = true;
        while corner != NIL {
            let slot = &mut candidates[n * c..(n + 1) * c];
            if parallelogram_rule(before, c, mesh, e as u32, corner, slot) {
                n += 1;
                if n == 4 {
                    break;
                }
            }
            corner = if leftwards {
                swing_left(opp, corner)
            } else {
                swing_right(opp, corner)
            };
            if corner == start {
                break;
            }
            if corner == NIL && leftwards {
                leftwards = false;
                corner = swing_right(opp, start);
            }
        }
        if let Some(t) = trace.as_mut() {
            t.push(format!("cmp entry={e} corner={start} candidates={n}"));
        }
        let mut used = 0i32;
        pred.fill(0);
        for k in 0..n {
            if !flags[n - 1].next()? {
                used += 1;
                for (p, &v) in pred.iter_mut().zip(&candidates[k * c..(k + 1) * c]) {
                    *p = p.wrapping_add(v);
                }
            }
        }
        if used == 0 {
            tf.apply(&before[(e - 1) * c..], &mut rest[..c]);
        } else {
            for p in &mut pred {
                *p /= used;
            }
            tf.apply(&pred, &mut rest[..c]);
        }
    }
    Ok(())
}

/// Orientation list of the texture-coordinate predictor (ch. 08 §4): a
/// running value toggled by every 0 bit.
pub(crate) fn read_orientations(r: &mut Reader<'_>, entries: usize) -> Result<Vec<bool>> {
    let count = r.i32()?;
    let mut decoder = BitDecoder::read(r)?;
    // Each entry consumes at most one orientation.
    if count < 0 || count as usize > entries {
        return Err(invalid("draco: orientation count exceeds the entry count"));
    }
    let mut value = true;
    Ok((0..count)
        .map(|_| {
            if !decoder.bit() {
                value = !value;
            }
            value
        })
        .collect())
}

/// Texture-coordinate prediction (ch. 08 §4). `d` holds two integers per
/// entry.
pub(crate) fn texture_coordinates(
    d: &mut [i32],
    mesh: &MeshRefs<'_>,
    positions: &Positions<'_>,
    orientations: &mut Vec<bool>,
    tf: &Transform,
    trace: &mut Option<Vec<String>>,
) -> Result<()> {
    let seq = mesh.seq;
    for e in 0..d.len() / 2 {
        let corner = seq.entry_corner[e];
        let en = mesh.entry_of(next(corner));
        let ep = mesh.entry_of(prev(corner));
        let (before, rest) = d.split_at_mut(e * 2);
        let tuple = |i: u32| {
            let i = i as usize * 2;
            [i64::from(before[i]), i64::from(before[i + 1])]
        };
        let mut pred: Option<[i64; 2]> = None;
        if (en as usize) < e && (ep as usize) < e {
            let n = tuple(en);
            let pv = tuple(ep);
            if n == pv {
                pred = Some(pv);
            } else {
                let pos_n = positions.of_entry(seq, en).ok_or_else(missing_position)?;
                let pos_p = positions.of_entry(seq, ep).ok_or_else(missing_position)?;
                let pos_e = positions
                    .of_entry(seq, e as u32)
                    .ok_or_else(missing_position)?;
                let pn = sub3(pos_p, pos_n);
                let len2 = dot3(pn, pn);
                if len2 != 0 {
                    let cn = sub3(pos_e, pos_n);
                    let dot = dot3(pn, cn);
                    let uv = [pv[0].wrapping_sub(n[0]), pv[1].wrapping_sub(n[1])];
                    let x = [
                        n[0].wrapping_mul(len2)
                            .wrapping_add(uv[0].wrapping_mul(dot)),
                        n[1].wrapping_mul(len2)
                            .wrapping_add(uv[1].wrapping_mul(dot)),
                    ];
                    let foot = [
                        pos_n[0].wrapping_add(pn[0].wrapping_mul(dot).wrapping_div(len2)),
                        pos_n[1].wrapping_add(pn[1].wrapping_mul(dot).wrapping_div(len2)),
                        pos_n[2].wrapping_add(pn[2].wrapping_mul(dot).wrapping_div(len2)),
                    ];
                    let height = sub3(pos_e, foot);
                    let h2 = dot3(height, height);
                    let k = (h2.wrapping_mul(len2) as u64).isqrt() as i64;
                    let w = [uv[1].wrapping_mul(k), uv[0].wrapping_neg().wrapping_mul(k)];
                    let orientation = orientations
                        .pop()
                        .ok_or_else(|| invalid("draco: orientation flags exhausted"))?;
                    pred = Some(if orientation {
                        [
                            x[0].wrapping_add(w[0]).wrapping_div(len2),
                            x[1].wrapping_add(w[1]).wrapping_div(len2),
                        ]
                    } else {
                        [
                            x[0].wrapping_sub(w[0]).wrapping_div(len2),
                            x[1].wrapping_sub(w[1]).wrapping_div(len2),
                        ]
                    });
                }
            }
        }
        let pred = pred.unwrap_or_else(|| {
            if (en as usize) < e {
                tuple(en)
            } else if e > 0 {
                tuple(e as u32 - 1)
            } else {
                [0, 0]
            }
        });
        let pred = [pred[0] as i32, pred[1] as i32];
        if let Some(t) = trace.as_mut() {
            t.push(format!(
                "uv_pred[{e}] corner={corner} = {} {}",
                pred[0], pred[1]
            ));
        }
        tf.apply(&pred, &mut rest[..2]);
    }
    Ok(())
}

#[inline]
fn sub3(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [
        a[0].wrapping_sub(b[0]),
        a[1].wrapping_sub(b[1]),
        a[2].wrapping_sub(b[2]),
    ]
}

#[inline]
fn dot3(a: [i64; 3], b: [i64; 3]) -> i64 {
    a[0].wrapping_mul(b[0])
        .wrapping_add(a[1].wrapping_mul(b[1]))
        .wrapping_add(a[2].wrapping_mul(b[2]))
}

#[inline]
fn cross3(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [
        a[1].wrapping_mul(b[2])
            .wrapping_sub(a[2].wrapping_mul(b[1])),
        a[2].wrapping_mul(b[0])
            .wrapping_sub(a[0].wrapping_mul(b[2])),
        a[0].wrapping_mul(b[1])
            .wrapping_sub(a[1].wrapping_mul(b[0])),
    ]
}

/// Geometric normal prediction (ch. 08 §5). `d` holds two octahedral
/// coordinates per entry; `flips` yields one bit per entry.
pub(crate) fn geometric_normals(
    d: &mut [i32],
    mesh: &MeshRefs<'_>,
    positions: &Positions<'_>,
    oct: &Octahedral,
    flips: &mut BitDecoder<'_>,
    trace: &mut Option<Vec<String>>,
) -> Result<()> {
    const REDUCE_ABOVE: i64 = 1 << 29;
    let seq = mesh.seq;
    let center = i64::from(oct.center);
    let m = oct.m;
    for e in 0..d.len() / 2 {
        let corner = seq.entry_corner[e];
        let pc = positions
            .of_entry(seq, e as u32)
            .ok_or_else(missing_position)?;
        // Step 1: area-weighted normal over the fan of the entry's vertex.
        let mut nrm = [0i64; 3];
        let mut complete = true;
        for_each_corner_of_vertex(mesh.view.opp, corner, |k| {
            let a = positions.of_entry(seq, mesh.entry_of(next(k)));
            let b = positions.of_entry(seq, mesh.entry_of(prev(k)));
            match (a, b) {
                (Some(a), Some(b)) => {
                    let x = cross3(sub3(a, pc), sub3(b, pc));
                    for (acc, v) in nrm.iter_mut().zip(x) {
                        *acc = acc.wrapping_add(v);
                    }
                }
                _ => complete = false,
            }
        });
        if !complete {
            return Err(missing_position());
        }
        // Step 2: range reduction.
        let abs_sum = |v: &[i64; 3]| {
            v[0].wrapping_abs()
                .wrapping_add(v[1].wrapping_abs())
                .wrapping_add(v[2].wrapping_abs())
        };
        let s = abs_sum(&nrm);
        if s > REDUCE_ABOVE {
            let q = s / REDUCE_ABOVE;
            for v in &mut nrm {
                *v = v.wrapping_div(q);
            }
        }
        let raw = [nrm[0] as i32, nrm[1] as i32, nrm[2] as i32];
        if let Some(t) = trace.as_mut() {
            t.push(format!(
                "normal_pred[{e}] corner={corner} raw = {} {} {}",
                raw[0], raw[1], raw[2]
            ));
        }
        // Step 3: projection onto the octahedron.
        let wide = [i64::from(raw[0]), i64::from(raw[1]), i64::from(raw[2])];
        let s = wide[0].abs() + wide[1].abs() + wide[2].abs();
        let (mut x, mut y, mut z) = if s == 0 {
            (center, 0, 0)
        } else {
            let x = wide[0] * center / s;
            let y = wide[1] * center / s;
            let z = center - x.abs() - y.abs();
            (x, y, if wide[2] < 0 { -z } else { z })
        };
        // Step 4: flip.
        if flips.bit() {
            (x, y, z) = (-x, -y, -z);
        }
        // Step 5: octahedral coordinates, canonicalised.
        let big = i64::from(m);
        let (mut s, mut t) = if x >= 0 {
            (y + center, z + center)
        } else {
            (
                if y < 0 { z.abs() } else { big - z.abs() },
                if z < 0 { y.abs() } else { big - y.abs() },
            )
        };
        // First matching rule wins.
        let corner = (s, t) == (0, 0) || (s, t) == (0, big) || (s, t) == (big, 0);
        let mirror_t = (s == 0 && t > center) || (s == big && t < center);
        let mirror_s = (t == big && s < center) || (t == 0 && s > center);
        if corner {
            (s, t) = (big, big);
        } else if mirror_t {
            t = 2 * center - t;
        } else if mirror_s {
            s = 2 * center - s;
        }
        if let Some(tr) = trace.as_mut() {
            tr.push(format!("normal_pred[{e}] oct = {s} {t}"));
        }
        let out = oct.combine([s as i32, t as i32], [d[e * 2], d[e * 2 + 1]]);
        d[e * 2..e * 2 + 2].copy_from_slice(&out);
    }
    Ok(())
}
