//! Method 1 attribute sequencing: the mesh walk that orders the entries of
//! an attribute group (`docs/3d/gltf/draco/spec/07-attribute-sequencing.md`).

use super::edgebreaker::{NIL, left_corner, next, prev, right_corner, swing_left};
use crate::error::{Result, invalid};

/// The view of the mesh an attribute group walks (§1): the mesh corner
/// table, or the cut table of one seam set.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View<'a> {
    /// `V(c)` per corner.
    pub(crate) vertex: &'a [u32],
    /// `OPP(c)` per corner.
    pub(crate) opp: &'a [u32],
    /// One designated corner per view vertex (`NIL` for a vertex without
    /// faces).
    pub(crate) first_corner: &'a [u32],
}

impl View<'_> {
    fn on_boundary(&self, v: u32) -> bool {
        swing_left(self.opp, self.first_corner[v as usize]) == NIL
    }
}

/// Outputs of a group's walk (§2).
#[derive(Debug, Default)]
pub(crate) struct Sequence {
    /// `entry_corner[e]`.
    pub(crate) entry_corner: Vec<u32>,
    /// `vertex_entry[v]`, `NIL` for a vertex the walk never reached.
    pub(crate) vertex_entry: Vec<u32>,
    /// `entry_point[e]`.
    pub(crate) entry_point: Vec<u32>,
}

impl Sequence {
    pub(crate) fn len(&self) -> usize {
        self.entry_corner.len()
    }

    #[inline]
    fn visited(&self, v: u32) -> bool {
        self.vertex_entry[v as usize] != NIL
    }

    fn visit(&mut self, v: u32, c: u32, point: &[u32], trace: &mut Option<Vec<String>>) {
        let e = self.entry_corner.len() as u32;
        self.vertex_entry[v as usize] = e;
        self.entry_corner.push(c);
        self.entry_point.push(point[c as usize]);
        if let Some(t) = trace.as_mut() {
            t.push(format!(
                "seq[{e}]: vertex={v} corner={c} point={}",
                point[c as usize]
            ));
        }
    }
}

#[inline]
fn done(face_visited: &[bool], c: u32) -> bool {
    c == NIL || face_visited[c as usize / 3]
}

/// Walk the view in the given traversal order (0 depth-first,
/// 1 prediction-degree) and return the entry sequence.
pub(crate) fn sequence(
    view: View<'_>,
    point: &[u32],
    traversal_order: u8,
    trace: &mut Option<Vec<String>>,
) -> Result<Sequence> {
    let corners = view.opp.len();
    if view
        .vertex
        .iter()
        .any(|&v| v as usize >= view.first_corner.len())
    {
        return Err(invalid("draco: corner refers to a missing vertex"));
    }
    let mut seq = Sequence {
        entry_corner: Vec::new(),
        vertex_entry: vec![NIL; view.first_corner.len()],
        entry_point: Vec::new(),
    };
    let mut face_visited = vec![false; corners / 3];
    let vertex = |c: u32| view.vertex[c as usize];

    if traversal_order == 0 {
        // §3.1 depth-first.
        let mut stack: Vec<u32> = Vec::new();
        for start in (0..corners as u32).step_by(3) {
            if face_visited[start as usize / 3] {
                continue;
            }
            stack.clear();
            stack.push(start);
            for c in [next(start), prev(start)] {
                if !seq.visited(vertex(c)) {
                    seq.visit(vertex(c), c, point, trace);
                }
            }
            while let Some(&at) = stack.last() {
                let mut k = at;
                if done(&face_visited, k) {
                    stack.pop();
                    continue;
                }
                loop {
                    if k == NIL {
                        stack.pop();
                        break;
                    }
                    face_visited[k as usize / 3] = true;
                    let v = vertex(k);
                    if !seq.visited(v) {
                        let boundary = view.on_boundary(v);
                        seq.visit(v, k, point, trace);
                        if !boundary {
                            k = right_corner(view.opp, k);
                            continue;
                        }
                    }
                    let r = right_corner(view.opp, k);
                    let l = left_corner(view.opp, k);
                    match (done(&face_visited, r), done(&face_visited, l)) {
                        (true, true) => {
                            stack.pop();
                            break;
                        }
                        (true, false) => k = l,
                        (false, true) => k = r,
                        (false, false) => {
                            *stack.last_mut().unwrap() = l;
                            stack.push(r);
                            break;
                        }
                    }
                }
            }
        }
    } else {
        // §3.2 prediction-degree.
        let mut degree = vec![0u32; view.first_corner.len()];
        let mut stacks: [Vec<u32>; 3] = Default::default();
        for start in (0..corners as u32).step_by(3) {
            stacks[0].push(start);
            let mut best = 0usize;
            for c in [next(start), prev(start), start] {
                if !seq.visited(vertex(c)) {
                    seq.visit(vertex(c), c, point, trace);
                }
            }
            loop {
                // take()
                let mut taken = NIL;
                for (i, stack) in stacks.iter_mut().enumerate().skip(best) {
                    if let Some(c) = stack.pop() {
                        best = i;
                        taken = c;
                        break;
                    }
                }
                if taken == NIL {
                    break;
                }
                let mut k = taken;
                if face_visited[k as usize / 3] {
                    continue;
                }
                loop {
                    face_visited[k as usize / 3] = true;
                    let v = vertex(k);
                    if !seq.visited(v) {
                        seq.visit(v, k, point, trace);
                    }
                    let r = right_corner(view.opp, k);
                    let l = left_corner(view.opp, k);
                    let right_done = done(&face_visited, r);
                    let mut priority = |c: u32, seq: &Sequence| -> usize {
                        let v = vertex(c);
                        if seq.visited(v) {
                            return 0;
                        }
                        degree[v as usize] += 1;
                        if degree[v as usize] > 1 { 1 } else { 2 }
                    };
                    if !done(&face_visited, l) {
                        let pr = priority(l, &seq);
                        if right_done && pr <= best {
                            k = l;
                            continue;
                        }
                        stacks[pr].push(l);
                        best = best.min(pr);
                    }
                    if !right_done {
                        let pr = priority(r, &seq);
                        if pr <= best {
                            k = r;
                            continue;
                        }
                        stacks[pr].push(r);
                        best = best.min(pr);
                    }
                    break;
                }
            }
        }
    }
    Ok(seq)
}
