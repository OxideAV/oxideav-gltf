//! Mesh encoding method 1: triangle adjacency stored as a symbol string
//! and replayed face by face
//! (`docs/3d/gltf/draco/spec/06-edgebreaker-connectivity.md`).

use super::Limits;
use super::entropy::{BitDecoder, EntropyScratch, read_symbols};
use super::reader::{BitRun, Reader};
use crate::error::{Result, invalid};

/// "No corner" / "no vertex" marker (the specification's −1).
pub(crate) const NIL: u32 = u32::MAX;

/// Following corner of the same face.
#[inline]
pub(crate) fn next(c: u32) -> u32 {
    if c % 3 == 2 { c - 2 } else { c + 1 }
}

/// Preceding corner of the same face.
#[inline]
pub(crate) fn prev(c: u32) -> u32 {
    if c.is_multiple_of(3) { c + 2 } else { c - 1 }
}

#[inline]
fn cross(opp: &[u32], c: u32) -> u32 {
    if c == NIL { NIL } else { opp[c as usize] }
}

/// `next(opp(next(c)))`, or `NIL`.
#[inline]
pub(crate) fn swing_left(opp: &[u32], c: u32) -> u32 {
    if c == NIL {
        return NIL;
    }
    let o = opp[next(c) as usize];
    if o == NIL { NIL } else { next(o) }
}

/// `prev(opp(prev(c)))`, or `NIL`.
#[inline]
pub(crate) fn swing_right(opp: &[u32], c: u32) -> u32 {
    if c == NIL {
        return NIL;
    }
    let o = opp[prev(c) as usize];
    if o == NIL { NIL } else { prev(o) }
}

/// `opp(next(c))`, or `NIL`.
#[inline]
pub(crate) fn right_corner(opp: &[u32], c: u32) -> u32 {
    if c == NIL { NIL } else { cross(opp, next(c)) }
}

/// `opp(prev(c))`, or `NIL`.
#[inline]
pub(crate) fn left_corner(opp: &[u32], c: u32) -> u32 {
    if c == NIL { NIL } else { cross(opp, prev(c)) }
}

/// Every corner of the vertex at `c0` (§8): `c0`, then leftwards until the
/// fan closes or ends, then — for an open fan — rightwards from `c0`.
pub(crate) fn for_each_corner_of_vertex(opp: &[u32], c0: u32, mut f: impl FnMut(u32)) {
    if c0 == NIL {
        return;
    }
    f(c0);
    let mut k = swing_left(opp, c0);
    while k != NIL && k != c0 {
        f(k);
        k = swing_left(opp, k);
    }
    if k == NIL {
        let mut k = swing_right(opp, c0);
        while k != NIL && k != c0 {
            f(k);
            k = swing_right(opp, k);
        }
    }
}

/// Corner table of one seam set (§7.2): the mesh table with seam edges cut.
#[derive(Debug)]
pub(crate) struct SeamSet {
    /// `seam_opp(c)`.
    pub(crate) opp: Vec<u32>,
    /// `seam_vertex(c)`.
    pub(crate) vertex: Vec<u32>,
    /// `seam_first_corner(w)`.
    pub(crate) first_corner: Vec<u32>,
}

/// Decoded connectivity.
#[derive(Debug)]
pub(crate) struct Connectivity {
    pub(crate) num_faces: usize,
    /// `vertex(c)` per corner.
    pub(crate) vertex: Vec<u32>,
    /// `opp(c)` per corner.
    pub(crate) opp: Vec<u32>,
    /// `first_corner(v)` per mesh vertex.
    pub(crate) first_corner: Vec<u32>,
    /// `point(c)` per corner (§9) when seam sets split vertices into
    /// points; `None` when points are the vertices.
    split_points: Option<Vec<u32>>,
    pub(crate) num_points: u32,
    pub(crate) seams: Vec<SeamSet>,
}

impl Connectivity {
    /// `point(c)` per corner (§9).
    pub(crate) fn points(&self) -> &[u32] {
        self.split_points.as_deref().unwrap_or(&self.vertex)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Symbol {
    C,
    S,
    L,
    R,
    E,
}

impl Symbol {
    /// Code used by the trace vocabulary (`eb_symbol[i] = v`).
    fn trace_code(self) -> u32 {
        match self {
            Symbol::C => 0,
            Symbol::S => 1,
            Symbol::L => 3,
            Symbol::R => 5,
            Symbol::E => 7,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SplitEvent {
    source: u32,
    split: u32,
    right: bool,
}

/// Where the replay takes its symbols from.
enum SymbolSource<'a> {
    /// §4: prefix-coded symbols in a bit run.
    Standard(BitRun<'a>),
    /// §5: six contexts selected by vertex valence, each consumed from its
    /// last value toward its first.
    Valence {
        contexts: [Vec<u32>; 6],
        active: Option<usize>,
    },
}

impl SymbolSource<'_> {
    fn next_symbol(&mut self) -> Result<Symbol> {
        match self {
            SymbolSource::Standard(run) => {
                if run.bits(1)? == 0 {
                    return Ok(Symbol::C);
                }
                Ok(match run.bits(2)? {
                    0 => Symbol::S,
                    1 => Symbol::L,
                    2 => Symbol::R,
                    _ => Symbol::E,
                })
            }
            SymbolSource::Valence { contexts, active } => {
                let Some(k) = *active else {
                    return Ok(Symbol::E);
                };
                match contexts[k].pop() {
                    Some(0) => Ok(Symbol::C),
                    Some(1) => Ok(Symbol::S),
                    Some(2) => Ok(Symbol::L),
                    Some(3) => Ok(Symbol::R),
                    Some(4) => Ok(Symbol::E),
                    Some(_) => Err(invalid("draco: unknown connectivity symbol")),
                    None => Err(invalid("draco: valence context exhausted")),
                }
            }
        }
    }
}

/// Corner table under construction.
struct Table {
    opp: Vec<u32>,
    vertex: Vec<u32>,
    first_corner: Vec<u32>,
    boundary: Vec<bool>,
    /// Per-vertex valence counters; maintained only for valence coding.
    valence: Vec<u32>,
    max_vertices: u64,
}

impl Table {
    fn new_vertex(&mut self, corner: u32) -> Result<u32> {
        if self.first_corner.len() as u64 >= self.max_vertices {
            return Err(invalid("draco: connectivity creates too many vertices"));
        }
        let v = self.first_corner.len() as u32;
        self.first_corner.push(corner);
        self.boundary.push(true);
        self.valence.push(0);
        Ok(v)
    }

    fn link(&mut self, a: u32, b: u32) -> Result<()> {
        if a == NIL || b == NIL || a == b {
            return Err(invalid("draco: connectivity links a missing corner"));
        }
        if self.opp[a as usize] != NIL || self.opp[b as usize] != NIL {
            return Err(invalid("draco: connectivity links an edge twice"));
        }
        self.opp[a as usize] = b;
        self.opp[b as usize] = a;
        Ok(())
    }

    /// `next(first_corner(v))`.
    fn after_first_corner(&self, v: u32) -> Result<u32> {
        match self.first_corner[v as usize] {
            NIL => Err(invalid("draco: connectivity uses a vertex without corners")),
            c => Ok(next(c)),
        }
    }

    fn bump(&mut self, c: u32, by: u32) {
        let v = self.vertex[c as usize] as usize;
        self.valence[v] = self.valence[v].wrapping_add(by);
    }
}

fn top(stack: &[u32]) -> Result<u32> {
    stack
        .last()
        .copied()
        .ok_or_else(|| invalid("draco: connectivity symbol needs an active corner"))
}

/// Read and replay the edgebreaker connectivity section.
pub(crate) fn read_connectivity(
    r: &mut Reader<'_>,
    limits: &Limits,
    scratch: &mut EntropyScratch,
    trace: &mut Option<Vec<String>>,
) -> Result<Connectivity> {
    // §2 layout.
    let symbol_coding = r.u8()?;
    if symbol_coding != 0 && symbol_coding != 2 {
        return Err(invalid("draco: unknown connectivity symbol coding"));
    }
    let num_vertices = r.varint32()?;
    let num_faces = r.varint32()?;
    let num_seam_sets = usize::from(r.u8()?);
    let num_symbols = r.varint32()?;
    let num_split_symbols = r.varint32()?;
    if num_symbols > num_faces
        || u64::from(num_faces) > u64::from(num_symbols) + u64::from(num_symbols / 3)
        || num_split_symbols > num_symbols
        || u64::from(num_vertices) > 3 * u64::from(num_faces)
    {
        return Err(invalid("draco: inconsistent connectivity counts"));
    }
    if num_faces > limits.max_faces {
        return Err(invalid("draco: face count above the decoder limit"));
    }

    // §3 split events.
    let num_events = r.count()?;
    if num_events > r.remaining() / 2 {
        return Err(invalid("draco: split event count exceeds the payload"));
    }
    let mut events = Vec::with_capacity(num_events);
    let mut source = 0u32;
    for _ in 0..num_events {
        let source_delta = r.varint32()?;
        let split_delta = r.varint32()?;
        source = source
            .checked_add(source_delta)
            .ok_or_else(|| invalid("draco: split event id overflow"))?;
        let split = source
            .checked_sub(split_delta)
            .ok_or_else(|| invalid("draco: split event id underflow"))?;
        if source >= num_symbols {
            return Err(invalid("draco: split event outside the symbol range"));
        }
        events.push(SplitEvent {
            source,
            split,
            right: false,
        });
    }
    if num_events > 0 {
        let mut run = BitRun::new(r.rest());
        for ev in &mut events {
            ev.right = run.bits(1)? == 1;
        }
        r.take(run.bytes_used())?;
    }

    let s = num_symbols as usize;
    let mut symbols = if symbol_coding == 0 {
        let size = r.count()?;
        let bytes = r.take(size)?;
        if s > bytes.len().saturating_mul(8) {
            return Err(invalid("draco: symbol count exceeds the symbol stream"));
        }
        SymbolSource::Standard(BitRun::new(bytes))
    } else {
        SymbolSource::Valence {
            contexts: Default::default(),
            active: None,
        }
    };
    let mut start_faces = BitDecoder::read(r)?;
    let mut seam_decoders = Vec::with_capacity(num_seam_sets);
    for _ in 0..num_seam_sets {
        seam_decoders.push(BitDecoder::read(r)?);
    }
    if let SymbolSource::Valence { contexts, .. } = &mut symbols {
        let mut total = 0usize;
        for ctx in contexts.iter_mut() {
            let count = r.count()?;
            if count > s {
                return Err(invalid(
                    "draco: valence context larger than the symbol count",
                ));
            }
            read_symbols(r, count, 1, scratch, ctx)?;
            total += count;
        }
        if total + 1 < s {
            return Err(invalid("draco: valence contexts hold too few symbols"));
        }
    }

    // §6 replay.
    let f = num_faces as usize;
    let mut t = Table {
        opp: vec![NIL; 3 * f],
        vertex: vec![NIL; 3 * f],
        first_corner: Vec::new(),
        boundary: Vec::new(),
        valence: Vec::new(),
        max_vertices: u64::from(num_vertices) + u64::from(num_split_symbols),
    };
    let valence_coding = symbol_coding == 2;
    let mut stack: Vec<u32> = Vec::new();
    let mut pending: Vec<u32> = if events.is_empty() {
        Vec::new()
    } else {
        vec![NIL; s]
    };
    let mut merged_away: Vec<u32> = Vec::new();

    for i in 0..s {
        let symbol = symbols.next_symbol()?;
        if let Some(t) = trace.as_mut() {
            t.push(format!("eb_symbol[{i}] = {}", symbol.trace_code()));
        }
        let c = 3 * i as u32;
        match symbol {
            Symbol::C => {
                let a = top(&stack)?;
                let x = t.vertex[next(a) as usize];
                let b = t.after_first_corner(x)?;
                t.link(a, c + 1)?;
                t.link(b, c + 2)?;
                let far = t.vertex[prev(a) as usize];
                t.vertex[c as usize] = x;
                t.vertex[c as usize + 1] = t.vertex[next(b) as usize];
                t.vertex[c as usize + 2] = far;
                t.first_corner[far as usize] = c + 2;
                t.boundary[x as usize] = false;
                *stack.last_mut().unwrap() = c;
            }
            Symbol::R => {
                let a = top(&stack)?;
                t.link(c + 2, a)?;
                let n = t.new_vertex(c + 2)?;
                t.vertex[c as usize + 2] = n;
                let rv = t.vertex[prev(a) as usize];
                t.vertex[c as usize] = rv;
                t.first_corner[rv as usize] = c;
                t.vertex[c as usize + 1] = t.vertex[next(a) as usize];
                *stack.last_mut().unwrap() = c;
            }
            Symbol::L => {
                let a = top(&stack)?;
                t.link(c + 1, a)?;
                let n = t.new_vertex(c + 1)?;
                t.vertex[c as usize + 1] = n;
                let lv = t.vertex[prev(a) as usize];
                t.vertex[c as usize + 2] = lv;
                t.first_corner[lv as usize] = c + 2;
                t.vertex[c as usize] = t.vertex[next(a) as usize];
                *stack.last_mut().unwrap() = c;
            }
            Symbol::S => {
                let b = stack
                    .pop()
                    .ok_or_else(|| invalid("draco: split symbol without an active corner"))?;
                if let Some(&k) = pending.get(i)
                    && k != NIL
                {
                    stack.push(k);
                }
                let a = top(&stack)?;
                if a == b {
                    return Err(invalid("draco: split symbol joins a corner with itself"));
                }
                t.link(a, c + 2)?;
                t.link(b, c + 1)?;
                let p = t.vertex[prev(a) as usize];
                t.vertex[c as usize] = p;
                t.vertex[c as usize + 1] = t.vertex[next(a) as usize];
                let q = t.vertex[prev(b) as usize];
                t.vertex[c as usize + 2] = q;
                t.first_corner[q as usize] = c + 2;
                // Merge the vertex at next(b) into p.
                let m = next(b);
                let n = t.vertex[m as usize];
                if valence_coding {
                    t.valence[p as usize] =
                        t.valence[p as usize].wrapping_add(t.valence[n as usize]);
                }
                t.first_corner[p as usize] = t.first_corner[n as usize];
                let mut k = m;
                loop {
                    t.vertex[k as usize] = p;
                    k = swing_left(&t.opp, k);
                    if k == NIL {
                        break;
                    }
                    if k == m {
                        return Err(invalid("draco: split symbol merges a closed fan"));
                    }
                }
                t.first_corner[n as usize] = NIL;
                if num_seam_sets == 0 {
                    merged_away.push(n);
                }
                *stack.last_mut().unwrap() = c;
            }
            Symbol::E => {
                for j in 0..3 {
                    let n = t.new_vertex(c + j)?;
                    t.vertex[(c + j) as usize] = n;
                }
                stack.push(c);
            }
        }

        // §6.6 step 1 / §5 steps 2–3.
        let a = top(&stack)?;
        if let SymbolSource::Valence { active, .. } = &mut symbols {
            let (an, ap) = (next(a), prev(a));
            match symbol {
                Symbol::C | Symbol::S => {
                    t.bump(an, 1);
                    t.bump(ap, 1);
                }
                Symbol::R => {
                    t.bump(a, 1);
                    t.bump(an, 1);
                    t.bump(ap, 2);
                }
                Symbol::L => {
                    t.bump(a, 1);
                    t.bump(an, 2);
                    t.bump(ap, 1);
                }
                Symbol::E => {
                    t.bump(a, 2);
                    t.bump(an, 2);
                    t.bump(ap, 2);
                }
            }
            let v = t.valence[t.vertex[an as usize] as usize];
            *active = Some(v.clamp(2, 7) as usize - 2);
        }
        // §6.6 step 2.
        if matches!(symbol, Symbol::R | Symbol::L | Symbol::E) {
            let id = (s - 1 - i) as u32;
            while let Some(ev) = events.last().copied() {
                if ev.source > id {
                    return Err(invalid("draco: split event was never reached"));
                }
                if ev.source < id {
                    break;
                }
                events.pop();
                let k = if ev.right { next(a) } else { prev(a) };
                pending[s - 1 - ev.split as usize] = k;
            }
        }
    }

    // §6.7 start faces.
    let mut created = s;
    while let Some(a) = stack.pop() {
        let interior = start_faces.bit();
        if let Some(t) = trace.as_mut() {
            t.push(format!(
                "start_face_interior corner={a} = {}",
                u8::from(interior)
            ));
        }
        if !interior {
            continue;
        }
        if created >= f {
            return Err(invalid("draco: more faces than declared"));
        }
        let c = 3 * created as u32;
        created += 1;
        let n = t.vertex[next(a) as usize];
        let b = t.after_first_corner(n)?;
        let x = t.vertex[next(b) as usize];
        let d = t.after_first_corner(x)?;
        let p = t.vertex[next(d) as usize];
        t.link(c, a)?;
        t.link(c + 1, b)?;
        t.link(c + 2, d)?;
        t.vertex[c as usize] = x;
        t.vertex[c as usize + 1] = p;
        t.vertex[c as usize + 2] = n;
        for v in [x, p, n] {
            t.boundary[v as usize] = false;
        }
    }
    if created != f {
        return Err(invalid("draco: fewer faces than declared"));
    }

    let Table {
        opp,
        mut vertex,
        mut first_corner,
        mut boundary,
        ..
    } = t;

    let (point, num_points, seams) = if num_seam_sets == 0 {
        // §6.8 vertex renumbering, §9 points are the vertices.
        let mut nv = first_corner.len();
        for &r in &merged_away {
            while nv > 0 && first_corner[nv - 1] == NIL {
                nv -= 1;
            }
            if nv == 0 {
                break;
            }
            let last = nv - 1;
            if last < r as usize {
                continue;
            }
            for_each_corner_of_vertex(&opp, first_corner[last], |k| vertex[k as usize] = r);
            first_corner[r as usize] = first_corner[last];
            first_corner[last] = NIL;
            boundary[r as usize] = boundary[last];
            nv -= 1;
        }
        if vertex.iter().any(|&v| v as usize >= nv) {
            return Err(invalid("draco: face refers to a removed vertex"));
        }
        first_corner.truncate(nv);
        (None, nv as u32, Vec::new())
    } else {
        let seams = build_seam_sets(&opp, &vertex, &first_corner, &mut seam_decoders, trace)?;
        let (point, num_points) = number_points(&opp, &first_corner, &boundary, &seams)?;
        (Some(point), num_points, seams)
    };

    if let Some(t) = trace.as_mut() {
        let point = point.as_deref().unwrap_or(&vertex);
        let show = |v: u32| if v == NIL { -1 } else { i64::from(v) };
        for face in 0..f {
            let c = 3 * face;
            t.push(format!(
                "conn face {face}: vertices {} {} {} opposite {} {} {} points {} {} {}",
                vertex[c],
                vertex[c + 1],
                vertex[c + 2],
                show(opp[c]),
                show(opp[c + 1]),
                show(opp[c + 2]),
                point[c],
                point[c + 1],
                point[c + 2]
            ));
        }
    }

    Ok(Connectivity {
        num_faces: f,
        vertex,
        opp,
        first_corner,
        split_points: point,
        num_points,
        seams,
    })
}

/// §7: decode the seam flags and build one cut corner table per seam set.
fn build_seam_sets(
    opp: &[u32],
    vertex: &[u32],
    first_corner: &[u32],
    decoders: &mut [BitDecoder<'_>],
    trace: &mut Option<Vec<String>>,
) -> Result<Vec<SeamSet>> {
    let corners = opp.len();
    let sets = decoders.len();
    let mut seam_edge = vec![false; corners * sets];
    // §7.1 seam flags, interleaved across the seam sets.
    for c in 0..corners {
        let o = opp[c];
        if o == NIL {
            for j in 0..sets {
                seam_edge[j * corners + c] = true;
            }
        } else if (o as usize) / 3 >= c / 3 {
            for (j, dec) in decoders.iter_mut().enumerate() {
                let bit = dec.bit();
                if let Some(t) = trace.as_mut() {
                    t.push(format!("seam set={j} corner={c} = {}", u8::from(bit)));
                }
                if bit {
                    seam_edge[j * corners + c] = true;
                    seam_edge[j * corners + o as usize] = true;
                }
            }
        }
    }

    // §7.2 tables.
    let mut out = Vec::with_capacity(sets);
    let mut on_seam = vec![false; first_corner.len()];
    for j in 0..sets {
        let edges = &seam_edge[j * corners..(j + 1) * corners];
        on_seam.fill(false);
        let mut seam_opp = vec![NIL; corners];
        for c in 0..corners {
            if edges[c] {
                on_seam[vertex[next(c as u32) as usize] as usize] = true;
                on_seam[vertex[prev(c as u32) as usize] as usize] = true;
            } else {
                seam_opp[c] = opp[c];
            }
        }
        let mut seam_vertex = vec![NIL; corners];
        let mut seam_first_corner: Vec<u32> = Vec::new();
        for (v, &first) in first_corner.iter().enumerate() {
            if first == NIL {
                continue;
            }
            let mut c0 = first;
            if on_seam[v] {
                loop {
                    let left = swing_left(&seam_opp, c0);
                    if left == NIL {
                        break;
                    }
                    if left == first {
                        return Err(invalid("draco: seam vertex without a seam edge"));
                    }
                    c0 = left;
                }
            }
            let mut current = seam_first_corner.len() as u32;
            seam_first_corner.push(c0);
            seam_vertex[c0 as usize] = current;
            let mut k = swing_right(opp, c0);
            while k != NIL && k != c0 {
                if edges[next(k) as usize] {
                    current = seam_first_corner.len() as u32;
                    seam_first_corner.push(k);
                }
                seam_vertex[k as usize] = current;
                k = swing_right(opp, k);
            }
        }
        if seam_vertex.contains(&NIL) {
            return Err(invalid("draco: corner outside every seam vertex"));
        }
        out.push(SeamSet {
            opp: seam_opp,
            vertex: seam_vertex,
            first_corner: seam_first_corner,
        });
    }
    Ok(out)
}

/// §9 with at least one seam set: split vertices into points.
fn number_points(
    opp: &[u32],
    first_corner: &[u32],
    boundary: &[bool],
    seams: &[SeamSet],
) -> Result<(Vec<u32>, u32)> {
    let mut point = vec![NIL; opp.len()];
    let mut count = 0u32;
    let differs = |a: u32, b: u32| {
        seams
            .iter()
            .any(|s| s.vertex[a as usize] != s.vertex[b as usize])
    };
    for (v, &first) in first_corner.iter().enumerate() {
        if first == NIL {
            continue;
        }
        let mut c0 = first;
        if !boundary[v] {
            // Move the start onto a seam so that no fan is cut in two.
            'sets: for s in seams {
                let mut k = swing_right(opp, c0);
                while k != NIL && k != c0 {
                    if s.vertex[k as usize] != s.vertex[c0 as usize] {
                        c0 = k;
                        break 'sets;
                    }
                    k = swing_right(opp, k);
                }
            }
        }
        point[c0 as usize] = count;
        count += 1;
        let mut before = c0;
        let mut k = swing_right(opp, c0);
        while k != NIL && k != c0 {
            if differs(k, before) {
                point[k as usize] = count;
                count += 1;
            } else {
                point[k as usize] = point[before as usize];
            }
            before = k;
            k = swing_right(opp, k);
        }
    }
    if point.contains(&NIL) {
        return Err(invalid("draco: corner outside every point"));
    }
    Ok((point, count))
}
