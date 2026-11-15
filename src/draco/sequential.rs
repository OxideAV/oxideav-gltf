//! Mesh encoding method 0: stored point indices
//! (`docs/3d/gltf/draco/spec/04-sequential-connectivity.md`).

use super::Limits;
use super::entropy::{EntropyScratch, read_symbols};
use super::reader::Reader;
use crate::error::{Result, invalid};

/// Result of the sequential connectivity section.
#[derive(Debug)]
pub(crate) struct SequentialConnectivity {
    pub(crate) num_points: u32,
    pub(crate) faces: Vec<[u32; 3]>,
}

pub(crate) fn read_connectivity(
    r: &mut Reader<'_>,
    limits: &Limits,
    scratch: &mut EntropyScratch,
) -> Result<SequentialConnectivity> {
    let num_faces = r.varint32()?;
    let num_points = r.varint32()?;
    if num_faces > u32::MAX / 3 || num_faces as usize > r.remaining() / 3 {
        return Err(invalid("draco: face count exceeds the payload"));
    }
    if num_faces > limits.max_faces {
        return Err(invalid("draco: face count above the decoder limit"));
    }
    if num_points > limits.max_points {
        return Err(invalid("draco: point count above the decoder limit"));
    }
    let n = num_faces as usize * 3;
    let mut indices: Vec<u32> = Vec::new();
    match r.u8()? {
        1 => {
            // §2: storage width chosen by the point count.
            indices.reserve(n);
            if num_points < 256 {
                indices.extend(r.take(n)?.iter().map(|&b| u32::from(b)));
            } else if num_points < 65536 {
                let raw = r.take(n.saturating_mul(2))?;
                indices.extend(
                    raw.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&b| u32::from(u16::from_le_bytes(b))),
                );
            } else if num_points < (1 << 21) {
                for _ in 0..n {
                    indices.push(r.varint32()?);
                }
            } else {
                let raw = r.take(n.saturating_mul(4))?;
                indices.extend(
                    raw.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|&b| u32::from_le_bytes(b)),
                );
            }
        }
        0 => {
            // §3: sign-magnitude differences to the previous index.
            read_symbols(r, n, 1, scratch, &mut indices)?;
            let mut previous = 0u32;
            for v in &mut indices {
                let magnitude = *v >> 1;
                previous = if *v & 1 == 1 {
                    previous.wrapping_sub(magnitude)
                } else {
                    previous.wrapping_add(magnitude)
                };
                *v = previous;
            }
        }
        _ => return Err(invalid("draco: unknown index coding")),
    }
    if indices.iter().any(|&i| i >= num_points) {
        return Err(invalid("draco: face index outside the point range"));
    }
    let faces = indices.as_chunks::<3>().0.to_vec();
    Ok(SequentialConnectivity { num_points, faces })
}
