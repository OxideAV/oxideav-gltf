//! `KHR_meshopt_compression` bitstream decoder — a pure, self-contained
//! implementation of Appendix A (Bitstream) + Appendix B (Filters) of
//! `docs/3d/gltf/extensions/KHR_meshopt_compression.md`.
//!
//! The extension hangs a compression descriptor off a `bufferView`
//! (§"Specifying compressed views"): the parent bufferView fields
//! describe the *uncompressed* element layout (`byteStride`, `count`)
//! while the descriptor's `buffer` / `byteOffset` / `byteLength` point
//! at the *compressed* source range. [`decode`] turns that compressed
//! range into the `byteStride * count` decompressed bytes, then applies
//! the post-decompression filter from §"Appendix B".
//!
//! Three modes are defined (§"Appendix A"):
//!
//! * Mode 0 ATTRIBUTES — byte-deinterleaved per-channel delta coding,
//!   v0 (`0xa0`, identical to `EXT_meshopt_compression`) and v1
//!   (`0xa1`, with control modes + channel modes).
//! * Mode 1 TRIANGLES — triangle-list index compression driven by an
//!   edge/vertex FIFO and a `codeaux` lookup table.
//! * Mode 2 INDICES — generic index delta compression with two
//!   alternating baselines.
//!
//! Four filters are defined (§"Appendix B"): OCTAHEDRAL, QUATERNION,
//! EXPONENTIAL, COLOR. NONE is a pass-through.
//!
//! All arithmetic follows the spec's stated integer widths and
//! wraparound rules. The decoder never panics on malformed input: a
//! truncated stream, a bad header byte, an out-of-range FIFO read, or
//! leftover tail bytes all surface as `Err`.

use crate::error::{invalid, unsupported, Error, Result};

/// Compression mode (`mode` descriptor property, §"Specifying
/// compressed views").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Attributes,
    Triangles,
    Indices,
}

impl Mode {
    /// Parse the spec's `mode` enum string.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "ATTRIBUTES" => Ok(Mode::Attributes),
            "TRIANGLES" => Ok(Mode::Triangles),
            "INDICES" => Ok(Mode::Indices),
            other => Err(invalid(format!(
                "KHR_meshopt_compression: unknown mode {other:?}"
            ))),
        }
    }
}

/// Post-decompression filter (`filter` descriptor property,
/// §"Appendix B").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None,
    Octahedral,
    Quaternion,
    Exponential,
    Color,
}

impl Filter {
    /// Parse the spec's `filter` enum string (absent → `NONE`).
    pub fn parse(s: Option<&str>) -> Result<Self> {
        match s.unwrap_or("NONE") {
            "NONE" => Ok(Filter::None),
            "OCTAHEDRAL" => Ok(Filter::Octahedral),
            "QUATERNION" => Ok(Filter::Quaternion),
            "EXPONENTIAL" => Ok(Filter::Exponential),
            "COLOR" => Ok(Filter::Color),
            other => Err(invalid(format!(
                "KHR_meshopt_compression: unknown filter {other:?}"
            ))),
        }
    }
}

/// Decode one compressed bufferView payload into `byte_stride * count`
/// decompressed bytes, then apply `filter`.
///
/// `data` is the exact compressed payload (descriptor `byteOffset` ..
/// `byteOffset + byteLength`). `count` and `byte_stride` are the
/// descriptor's element count + element stride.
pub fn decode(
    data: &[u8],
    mode: Mode,
    filter: Filter,
    count: usize,
    byte_stride: usize,
) -> Result<Vec<u8>> {
    let mut out = match mode {
        Mode::Attributes => decode_attributes(data, count, byte_stride)?,
        Mode::Triangles => decode_triangles(data, count, byte_stride)?,
        Mode::Indices => decode_indices(data, count, byte_stride)?,
    };
    apply_filter(&mut out, filter, byte_stride)?;
    Ok(out)
}

/// Encode `byte_stride * count` raw element bytes into a compressed
/// `KHR_meshopt_compression` payload, the inverse of [`decode`].
///
/// `raw` is the uncompressed element data (exactly `byte_stride * count`
/// bytes). The returned payload, fed back through [`decode`] with the
/// same `(mode, NONE, count, byte_stride)`, reproduces `raw`
/// byte-for-byte.
///
/// The encoder targets the spec's `NONE` filter only: the four
/// Appendix B filters (OCTAHEDRAL / QUATERNION / EXPONENTIAL / COLOR)
/// are quantising transforms applied *before* compression by the
/// content author, so they are out of scope for a lossless raw-byte
/// re-compression path. `filter` must therefore be [`Filter::None`].
///
/// * **ATTRIBUTES** (mode 0) emits the v0 stream (`0xa0`) — the same
///   wire shape `EXT_meshopt_compression` uses — with per-byte-position
///   group bit-width selection.
/// * **INDICES** (mode 2) emits the two-baseline varint delta stream.
/// * **TRIANGLES** (mode 1) emits an all-explicit triangle stream that
///   round-trips through the FIFO decoder.
pub fn encode(
    raw: &[u8],
    mode: Mode,
    filter: Filter,
    count: usize,
    byte_stride: usize,
) -> Result<Vec<u8>> {
    if filter != Filter::None {
        return Err(unsupported(
            "KHR_meshopt_compression: encode only supports the NONE filter \
             (Appendix B filters are author-side quantising transforms)",
        ));
    }
    let need = byte_stride
        .checked_mul(count)
        .ok_or_else(|| invalid("KHR_meshopt_compression: byteStride * count overflows"))?;
    if raw.len() != need {
        return Err(invalid(format!(
            "KHR_meshopt_compression: encode input is {} bytes, expected byteStride*count = {need}",
            raw.len()
        )));
    }
    match mode {
        Mode::Attributes => encode_attributes(raw, count, byte_stride),
        Mode::Triangles => encode_triangles(raw, count, byte_stride),
        Mode::Indices => encode_indices(raw, count, byte_stride),
    }
}

fn err_eos() -> Error {
    invalid("KHR_meshopt_compression: unexpected end of compressed stream")
}

// ---------------------------------------------------------------------------
// Mode 0: attributes (§"Mode 0: attributes")
// ---------------------------------------------------------------------------

fn decode_attributes(data: &[u8], count: usize, byte_stride: usize) -> Result<Vec<u8>> {
    if byte_stride == 0 || byte_stride % 4 != 0 {
        // §"Specifying compressed views": ATTRIBUTES byteStride is a
        // multiple of 4 (also enforced by the §3.12 stack validator).
        return Err(invalid(format!(
            "KHR_meshopt_compression: ATTRIBUTES byteStride {byte_stride} must be a positive multiple of 4"
        )));
    }

    // Header byte selects version (§"Mode 0").
    let header = *data.first().ok_or_else(err_eos)?;
    let version = match header {
        0xa1 => 1u8,
        0xa0 => 0u8,
        other => {
            return Err(invalid(format!(
                "KHR_meshopt_compression: ATTRIBUTES header byte 0x{other:02x} not 0xa0/0xa1"
            )))
        }
    };

    // Tail block sits at the very end: baseline element (byteStride
    // bytes) + channel modes (byteStride/4 bytes, v1 only).
    let channels = byte_stride / 4;
    let tail_len = byte_stride + if version == 1 { channels } else { 0 };
    if data.len() < 1 + tail_len {
        return Err(err_eos());
    }
    let tail_start = data.len() - tail_len;
    let baseline = &data[tail_start..tail_start + byte_stride];
    let channel_modes: Vec<u8> = if version == 1 {
        data[tail_start + byte_stride..].to_vec()
    } else {
        // v0 → every channel is mode 0 (byte deltas).
        vec![0u8; channels]
    };

    // Body is [1 .. tail_start). Decode attribute blocks of
    // deinterleaved byte deltas; reconstruct elements per channel mode.
    let body = &data[1..tail_start];
    let mut cur = Cursor::new(body);

    // `prev` holds the previous element's raw bytes (starts at baseline).
    let mut prev: Vec<u8> = baseline.to_vec();
    let mut out: Vec<u8> = Vec::with_capacity(count * byte_stride);

    let max_block_elements = ((8192 / byte_stride) & !15).clamp(1, 256);
    let mut remaining = count;
    while remaining > 0 {
        let block_elements = remaining.min(max_block_elements);
        let group_count = block_elements.div_ceil(16);

        // Per-byte-position control modes (v1 only); v0 → control 0
        // semantics with the v0 delta-mode table.
        let control: Vec<u8> = if version == 1 {
            let cbytes = byte_stride / 4;
            let raw = cur.take(cbytes).ok_or_else(err_eos)?;
            let mut c = Vec::with_capacity(byte_stride);
            for &cb in raw {
                c.push(cb & 0b11);
                c.push((cb >> 2) & 0b11);
                c.push((cb >> 4) & 0b11);
                c.push((cb >> 6) & 0b11);
            }
            c
        } else {
            vec![0u8; byte_stride]
        };

        // Decode each byte position's data block into per-element deltas
        // (zigzag-encoded byte deltas relative to the previous element).
        // `deltas[byte_pos][element]`.
        let mut deltas = vec![vec![0u8; block_elements]; byte_stride];
        for (byte_pos, dslot) in deltas.iter_mut().enumerate() {
            let cmode = if version == 1 { control[byte_pos] } else { 0 };
            decode_byte_channel(&mut cur, cmode, version, group_count, block_elements, dslot)?;
        }

        // Reconstruct each element of the block.
        let mut elem = vec![0u8; byte_stride];
        for e in 0..block_elements {
            for (byte_pos, dslot) in deltas.iter().enumerate() {
                elem[byte_pos] = dslot[e];
            }
            apply_channel_deltas(&mut prev, &elem, &channel_modes, byte_stride)?;
            out.extend_from_slice(&prev);
        }

        remaining -= block_elements;
    }

    if !cur.is_empty() {
        return Err(invalid(
            "KHR_meshopt_compression: ATTRIBUTES stream has leftover bytes before tail",
        ));
    }
    Ok(out)
}

/// Encode raw element bytes into a Mode 0 ATTRIBUTES v0 stream (inverse
/// of [`decode_attributes`] for the `0xa0` header). Channel modes are
/// fixed to v0 byte-delta coding; per-byte-position groups pick the
/// narrowest of the v0 bit-widths {0, 2, 4, 8}.
fn encode_attributes(raw: &[u8], count: usize, byte_stride: usize) -> Result<Vec<u8>> {
    if byte_stride == 0 || byte_stride % 4 != 0 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: ATTRIBUTES byteStride {byte_stride} must be a positive multiple of 4"
        )));
    }

    // Baseline = the first element (so element[0]'s deltas are all zero);
    // an all-zero buffer (count == 0) still needs a baseline of zeros.
    let baseline: Vec<u8> = if count > 0 {
        raw[..byte_stride].to_vec()
    } else {
        vec![0u8; byte_stride]
    };

    // deltas[byte_pos][element] = zigzag(raw[e][p] - raw[e-1][p]),
    // with raw[-1] := baseline (so delta[0] == 0 for every byte pos).
    let mut deltas = vec![vec![0u8; count]; byte_stride];
    for e in 0..count {
        for (p, dslot) in deltas.iter_mut().enumerate() {
            let cur = raw[e * byte_stride + p];
            let prev = if e == 0 {
                baseline[p]
            } else {
                raw[(e - 1) * byte_stride + p]
            };
            dslot[e] = zigzag_encode_u8(cur.wrapping_sub(prev));
        }
    }

    let mut out = vec![0xa0u8]; // v0 header

    let max_block_elements = ((8192 / byte_stride) & !15).clamp(1, 256);
    let mut start = 0usize;
    while start < count {
        let block_elements = (count - start).min(max_block_elements);
        for dslot in &deltas {
            encode_byte_channel(&mut out, &dslot[start..start + block_elements]);
        }
        start += block_elements;
    }

    out.extend_from_slice(&baseline); // tail block (v0: baseline only)
    Ok(out)
}

/// Encode one byte-position "data block" of `block.len()` zigzag delta
/// bytes (inverse of [`decode_byte_channel`] for v0 / cmode 0). Groups
/// of 16 each get a 2-bit header selecting the v0 bit-width.
fn encode_byte_channel(out: &mut Vec<u8>, block: &[u8]) {
    let group_count = block.len().div_ceil(16);

    // Decide each group's header (hb) + serialise its data after the
    // shared header bytes. v0 hb→bits: 0→0, 1→2, 2→4, 3→8.
    let mut headers = vec![0u8; group_count];
    let mut group_payloads: Vec<Vec<u8>> = Vec::with_capacity(group_count);
    for (g, hdr_slot) in headers.iter_mut().enumerate() {
        let g_start = g * 16;
        let g_end = (g_start + 16).min(block.len());
        // Pad the group to 16 with zero deltas (the decoder rounds up).
        let mut grp = [0u8; 16];
        grp[..g_end - g_start].copy_from_slice(&block[g_start..g_end]);

        let (hb, payload) = encode_group_v0(&grp);
        *hdr_slot = hb;
        group_payloads.push(payload);
    }

    // Header bytes: 2 bits per group, 4 groups per byte, LSB-first.
    let header_bytes = group_count.div_ceil(4);
    let mut hdr = vec![0u8; header_bytes];
    for (g, &hb) in headers.iter().enumerate() {
        hdr[g / 4] |= (hb & 0b11) << ((g % 4) * 2);
    }
    out.extend_from_slice(&hdr);
    for payload in group_payloads {
        out.extend_from_slice(&payload);
    }
}

/// Pick the narrowest v0 bit-width {0, 2, 4, 8} for a 16-element group of
/// zigzag delta bytes and serialise it (inverse of [`decode_group`]).
/// Returns `(hb, payload)` where `hb` is the 2-bit group header.
fn encode_group_v0(grp: &[u8; 16]) -> (u8, Vec<u8>) {
    // hb 0 (bits 0): only valid when every delta is zero — no data.
    if grp.iter().all(|&d| d == 0) {
        return (0, Vec::new());
    }

    // Candidate sentinel widths: hb1→2 bits, hb2→4 bits. For each, a
    // delta < sentinel packs inline; otherwise it stores the sentinel +
    // a trailing escape byte. hb3→8 bits stores 16 raw bytes.
    let cost_sentinel = |bits: u32| -> usize {
        let sentinel = (1u32 << bits) - 1;
        let packed = (16 * bits as usize) / 8;
        let escapes = grp.iter().filter(|&&d| d as u32 >= sentinel).count();
        packed + escapes
    };
    let cost2 = cost_sentinel(2);
    let cost4 = cost_sentinel(4);
    let cost8 = 16usize;

    // Prefer the smallest data cost; ties break toward the narrower width
    // (lower hb) for determinism.
    let (hb, bits) = if cost2 <= cost4 && cost2 <= cost8 {
        (1u8, 2u32)
    } else if cost4 <= cost8 {
        (2u8, 4u32)
    } else {
        (3u8, 8u32)
    };

    if bits == 8 {
        return (hb, grp.to_vec());
    }
    (hb, pack_group_sentinel(grp, bits))
}

/// Serialise a 16-element group at `bits` width with sentinel escapes
/// (inverse of the sentinel branch of [`decode_group`] + [`unpack_delta`]).
fn pack_group_sentinel(grp: &[u8; 16], bits: u32) -> Vec<u8> {
    let sentinel = ((1u32 << bits) - 1) as u8;
    let packed_bytes = (16 * bits as usize) / 8;
    let mut packed = vec![0u8; packed_bytes];
    let mut escapes: Vec<u8> = Vec::new();

    for (i, &d) in grp.iter().enumerate() {
        let small = if d >= sentinel { sentinel } else { d };
        pack_delta(&mut packed, bits, i, small);
        if d >= sentinel {
            escapes.push(d);
        }
    }

    packed.extend_from_slice(&escapes);
    packed
}

/// Pack the `bits`-wide value `small` for element `i` into `packed`
/// (inverse of [`unpack_delta`]).
fn pack_delta(packed: &mut [u8], bits: u32, i: usize, small: u8) {
    match bits {
        2 => {
            // MSB-first within byte: element 0 → bits 6-7, etc.
            let within = i % 4;
            let shift = (3 - within) * 2;
            packed[i / 4] |= (small & 0b11) << shift;
        }
        4 => {
            // even i → high nibble, odd → low nibble.
            if i % 2 == 0 {
                packed[i / 2] |= (small & 0x0f) << 4;
            } else {
                packed[i / 2] |= small & 0x0f;
            }
        }
        _ => {}
    }
}

/// Decode one byte-position "data block" into `block_elements`
/// zigzag-encoded byte deltas. `cmode` is the v1 control mode (or 0 for
/// v0). Per §"Mode 0: attributes" each group of 16 elements carries a
/// 2-bit header selecting the per-group encoding.
fn decode_byte_channel(
    cur: &mut Cursor,
    cmode: u8,
    version: u8,
    group_count: usize,
    block_elements: usize,
    out: &mut [u8],
) -> Result<()> {
    if cmode == 2 {
        // Control mode 2: all delta bytes are 0; nothing stored.
        for v in out.iter_mut() {
            *v = 0;
        }
        return Ok(());
    }
    if cmode == 3 {
        // Control mode 3: literal — delta bytes stored uncompressed, no
        // header bits, one byte per element (groups padded to 16).
        let total = group_count * 16;
        let raw = cur.take(total).ok_or_else(err_eos)?;
        out[..block_elements].copy_from_slice(&raw[..block_elements]);
        return Ok(());
    }

    // Control mode 0/1 (v1) or v0: header bits (2 per group) then
    // variable-length delta blocks.
    let header_bytes = group_count.div_ceil(4);
    let header = cur.take(header_bytes).ok_or_else(err_eos)?.to_vec();

    for g in 0..group_count {
        let hb = (header[g / 4] >> ((g % 4) * 2)) & 0b11;
        let g_start = g * 16;
        // bits = how many bits each delta gets in this group (0 means
        // all-zero, 8 means full literal byte).
        let bits = group_bits(cmode, version, hb);
        let group_out = &mut out[g_start.min(block_elements)..(g_start + 16).min(block_elements)];
        decode_group(cur, bits, group_out, g_start, block_elements)?;
    }
    Ok(())
}

/// Map (control-mode, version, 2-bit header) → bit-width for the
/// group's sentinel encoding, per the §"Mode 0" delta-encoding-mode
/// tables. Returns the bit width: 0 (all zero), 1/2/4 (sentinel), or 8
/// (full byte literal).
fn group_bits(cmode: u8, version: u8, hb: u8) -> u8 {
    if version == 0 {
        // v0 table.
        match hb {
            0 => 0,
            1 => 2,
            2 => 4,
            _ => 8,
        }
    } else if cmode == 0 {
        // v1 control mode 0: {0, 1, 2, 4}.
        match hb {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        }
    } else {
        // v1 control mode 1: {1, 2, 4, 8}.
        match hb {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => 8,
        }
    }
}

/// Decode a single 16-element group with `bits`-wide deltas. Sentinel
/// values (all-ones in `bits`) escape to a trailing full byte.
fn decode_group(
    cur: &mut Cursor,
    bits: u8,
    group_out: &mut [u8],
    g_start: usize,
    block_elements: usize,
) -> Result<()> {
    // Number of *real* output slots in this (possibly truncated tail)
    // group; the spec rounds up to 16 and ignores the surplus.
    let valid = group_out.len();

    if bits == 0 {
        for v in group_out.iter_mut() {
            *v = 0;
        }
        return Ok(());
    }
    if bits == 8 {
        // Full literal: 16 bytes (one per element of the group).
        let raw = cur.take(16).ok_or_else(err_eos)?;
        for (i, v) in group_out.iter_mut().enumerate() {
            *v = raw[i];
        }
        return Ok(());
    }

    // Sentinel encoding. First the packed bit-deltas for all 16
    // elements, then a trailing full byte per sentinel.
    let packed_bytes = (16 * bits as usize) / 8;
    let packed = cur.take(packed_bytes).ok_or_else(err_eos)?.to_vec();
    let sentinel: u8 = (1u16 << bits).wrapping_sub(1) as u8;

    let mut small = [0u8; 16];
    for (i, slot) in small.iter_mut().enumerate() {
        *slot = unpack_delta(&packed, bits, i);
    }

    // Sentinels are replaced by trailing explicit bytes, in element
    // order, for ALL 16 group positions (the encoder emits them for the
    // rounded-up group). We must consume escapes for every sentinel
    // among the 16, even the ones past `valid`, so the cursor stays
    // aligned.
    for (i, &s) in small.iter().enumerate() {
        let escaped = if s == sentinel {
            Some(*cur.take(1).ok_or_else(err_eos)?.first().unwrap())
        } else {
            None
        };
        if i < valid {
            group_out[i] = escaped.unwrap_or(s);
        }
        let _ = g_start;
        let _ = block_elements;
    }
    Ok(())
}

/// Extract the `bits`-wide delta for element `i` from a packed group
/// byte buffer, per §"Mode 0" packing conventions.
fn unpack_delta(packed: &[u8], bits: u8, i: usize) -> u8 {
    match bits {
        1 => {
            // LSB-first, 8 per byte.
            let byte = packed[i / 8];
            (byte >> (i % 8)) & 1
        }
        2 => {
            // MSB-first within byte: (d3<<0)|(d2<<2)|(d1<<4)|(d0<<6).
            let byte = packed[i / 4];
            let within = i % 4; // 0..3
            let shift = (3 - within) * 2;
            (byte >> shift) & 0b11
        }
        4 => {
            // (d1<<0)|(d0<<4): even index → high nibble.
            let byte = packed[i / 2];
            if i % 2 == 0 {
                (byte >> 4) & 0x0f
            } else {
                byte & 0x0f
            }
        }
        _ => 0,
    }
}

/// Reconstruct the current element bytes from the previous element plus
/// the freshly-decoded zigzag byte deltas, honouring per-channel modes
/// (§"Mode 0", channel modes 0/1/2). `prev` is updated in place to hold
/// the new element.
fn apply_channel_deltas(
    prev: &mut [u8],
    delta_bytes: &[u8],
    channel_modes: &[u8],
    byte_stride: usize,
) -> Result<()> {
    let channels = byte_stride / 4;
    for (ch, &mode_byte) in channel_modes.iter().enumerate().take(channels) {
        let base = ch * 4;
        let low = mode_byte & 0x0f;
        let high = (mode_byte >> 4) & 0x0f;
        match low {
            0 => {
                if high != 0 {
                    return Err(invalid(
                        "KHR_meshopt_compression: channel mode 0 with non-zero high nibble",
                    ));
                }
                // Byte deltas: per-byte zigzag diff vs previous.
                for b in 0..4 {
                    let d = zigzag_decode_u8(delta_bytes[base + b]);
                    prev[base + b] = prev[base + b].wrapping_add(d);
                }
            }
            1 => {
                if high != 0 {
                    return Err(invalid(
                        "KHR_meshopt_compression: channel mode 1 with non-zero high nibble",
                    ));
                }
                // 2-byte deltas: zigzag diff vs previous 16-bit values,
                // little-endian. Two 16-bit lanes per 4-byte channel.
                for half in 0..2 {
                    let off = base + half * 2;
                    let dz = u16::from_le_bytes([delta_bytes[off], delta_bytes[off + 1]]);
                    let d = zigzag_decode_u16(dz);
                    let p = u16::from_le_bytes([prev[off], prev[off + 1]]);
                    let v = p.wrapping_add(d);
                    let vb = v.to_le_bytes();
                    prev[off] = vb[0];
                    prev[off + 1] = vb[1];
                }
            }
            2 => {
                // 4-byte XOR deltas with rotation `r` (high nibble),
                // little-endian.
                let r = high as u32;
                let d = u32::from_le_bytes([
                    delta_bytes[base],
                    delta_bytes[base + 1],
                    delta_bytes[base + 2],
                    delta_bytes[base + 3],
                ]);
                let rot = rotate_left(d, r);
                let p = u32::from_le_bytes([
                    prev[base],
                    prev[base + 1],
                    prev[base + 2],
                    prev[base + 3],
                ]);
                let v = p ^ rot;
                let vb = v.to_le_bytes();
                prev[base..base + 4].copy_from_slice(&vb);
            }
            other => {
                return Err(invalid(format!(
                    "KHR_meshopt_compression: invalid channel mode {other}"
                )));
            }
        }
    }
    Ok(())
}

fn rotate_left(v: u32, r: u32) -> u32 {
    // (v << r) | (v >> ((32 - r) & 31)), matching the spec's `rotate`.
    let r = r & 31;
    if r == 0 {
        v
    } else {
        (v << r) | (v >> ((32 - r) & 31))
    }
}

fn zigzag_decode_u8(v: u8) -> u8 {
    if v & 1 != 0 {
        !(v >> 1)
    } else {
        v >> 1
    }
}

/// Inverse of [`zigzag_decode_u8`]: map a signed byte delta (as a `u8`
/// two's-complement value) to its zigzag encoding. The decoder applies
/// `d = (v & 1) ? !(v >> 1) : (v >> 1)`; this picks `v` so that holds.
fn zigzag_encode_u8(d: u8) -> u8 {
    if d & 0x80 != 0 {
        // Negative: decoder needs !(v>>1) == d → v>>1 == !d, odd.
        ((!d) << 1) | 1
    } else {
        d << 1
    }
}

fn zigzag_decode_u16(v: u16) -> u16 {
    if v & 1 != 0 {
        !(v >> 1)
    } else {
        v >> 1
    }
}

// ---------------------------------------------------------------------------
// Mode 1: triangles (§"Mode 1: triangles")
// ---------------------------------------------------------------------------

fn decode_triangles(data: &[u8], count: usize, byte_stride: usize) -> Result<Vec<u8>> {
    if byte_stride != 2 && byte_stride != 4 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: TRIANGLES byteStride {byte_stride} must be 2 or 4"
        )));
    }
    if count % 3 != 0 {
        return Err(invalid(
            "KHR_meshopt_compression: TRIANGLES count must be divisible by 3",
        ));
    }
    let header = *data.first().ok_or_else(err_eos)?;
    if header != 0xe1 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: TRIANGLES header byte 0x{header:02x} not 0xe1"
        )));
    }
    let triangle_count = count / 3;

    // Tail block: 16-byte codeaux table at the very end.
    if data.len() < 1 + 16 {
        return Err(err_eos());
    }
    let codeaux = &data[data.len() - 16..];
    // §: last two bytes must be 0; no nibble may be 0xf.
    if codeaux[14] != 0 || codeaux[15] != 0 {
        return Err(invalid(
            "KHR_meshopt_compression: TRIANGLES codeaux last two bytes must be 0",
        ));
    }
    for &b in &codeaux[..14] {
        if (b >> 4) == 0x0f || (b & 0x0f) == 0x0f {
            return Err(invalid(
                "KHR_meshopt_compression: TRIANGLES codeaux nibble equals 0xf",
            ));
        }
    }

    // `code` bytes: one per triangle, right after the header.
    let code_start = 1;
    let code_end = code_start + triangle_count;
    if code_end > data.len() - 16 {
        return Err(err_eos());
    }
    let codes = &data[code_start..code_end];
    // `data` section: between codes and the codeaux tail.
    let mut dcur = Cursor::new(&data[code_end..data.len() - 16]);

    let mut next: u32 = 0;
    let mut last: u32 = 0;
    let mut edge_fifo = Fifo2::new();
    let mut vertex_fifo = Fifo1::new();

    let mut indices: Vec<u32> = Vec::with_capacity(count);

    for &code in codes {
        let x = (code >> 4) & 0x0f;
        let y = code & 0x0f;
        let (a, b, c);

        if x < 0x0f {
            // Edge-based encodings (read edge at FIFO index X).
            let (ea, eb) = edge_fifo.get(x as usize)?;
            a = ea;
            b = eb;
            match y {
                0x0 => {
                    c = next;
                    next = next.wrapping_add(1);
                    edge_fifo.push(c, b);
                    edge_fifo.push(a, c);
                    vertex_fifo.push(c);
                }
                0x1..=0x0c => {
                    c = vertex_fifo.get(y as usize)?;
                    edge_fifo.push(c, b);
                    edge_fifo.push(a, c);
                    // §0xXY does NOT push vertex c.
                }
                0x0d => {
                    c = last.wrapping_sub(1);
                    last = c;
                    edge_fifo.push(c, b);
                    edge_fifo.push(a, c);
                    vertex_fifo.push(c);
                }
                0x0e => {
                    c = last.wrapping_add(1);
                    last = c;
                    edge_fifo.push(c, b);
                    edge_fifo.push(a, c);
                    vertex_fifo.push(c);
                }
                _ => {
                    // 0xXf
                    c = decode_index(&mut dcur, &mut last)?;
                    edge_fifo.push(c, b);
                    edge_fifo.push(a, c);
                    vertex_fifo.push(c);
                }
            }
        } else {
            // 0xfY family.
            if y < 0x0e {
                let zw = *codeaux.get(y as usize).ok_or_else(|| {
                    invalid("KHR_meshopt_compression: codeaux index out of range")
                })?;
                let z = (zw >> 4) & 0x0f;
                let w = zw & 0x0f;

                a = next;
                next = next.wrapping_add(1);

                if z == 0 {
                    b = next;
                    next = next.wrapping_add(1);
                } else {
                    b = vertex_fifo.get((z - 1) as usize)?;
                }
                if w == 0 {
                    c = next;
                    next = next.wrapping_add(1);
                } else {
                    c = vertex_fifo.get((w - 1) as usize)?;
                }

                edge_fifo.push(b, a);
                edge_fifo.push(c, b);
                edge_fifo.push(a, c);
                vertex_fifo.push(a);
                if z == 0 {
                    vertex_fifo.push(b);
                }
                if w == 0 {
                    vertex_fifo.push(c);
                }
            } else {
                // 0xfe or 0xff: three indices explicitly.
                let zw = *dcur.take(1).ok_or_else(err_eos)?.first().unwrap();
                if zw == 0x00 {
                    next = 0;
                }
                let z = (zw >> 4) & 0x0f;
                let w = zw & 0x0f;

                if y == 0x0e {
                    a = next;
                    next = next.wrapping_add(1);
                } else {
                    a = decode_index(&mut dcur, &mut last)?;
                }

                if z == 0 {
                    b = next;
                    next = next.wrapping_add(1);
                } else if z < 0x0f {
                    b = vertex_fifo.get((z - 1) as usize)?;
                } else {
                    b = decode_index(&mut dcur, &mut last)?;
                }

                if w == 0 {
                    c = next;
                    next = next.wrapping_add(1);
                } else if w < 0x0f {
                    c = vertex_fifo.get((w - 1) as usize)?;
                } else {
                    c = decode_index(&mut dcur, &mut last)?;
                }

                edge_fifo.push(b, a);
                edge_fifo.push(c, b);
                edge_fifo.push(a, c);
                vertex_fifo.push(a);
                if z == 0 || z == 0x0f {
                    vertex_fifo.push(b);
                }
                if w == 0 || w == 0x0f {
                    vertex_fifo.push(c);
                }
            }
        }

        indices.push(a);
        indices.push(b);
        indices.push(c);
    }

    if !dcur.is_empty() {
        return Err(invalid(
            "KHR_meshopt_compression: TRIANGLES data section has leftover bytes",
        ));
    }

    emit_indices(&indices, byte_stride)
}

/// Encode raw index bytes into a Mode 1 TRIANGLES stream (inverse of
/// [`decode_triangles`]). Uses the all-explicit `0xff` / `zw = 0xff`
/// escape for every triangle: each of the three corner indices is
/// emitted as an explicit `decode_index` delta against the shared
/// `last`. This is the simplest fully-general encoding the FIFO decoder
/// accepts — it does not exploit edge/vertex reuse, so it trades
/// compactness for a clean lossless round-trip.
fn encode_triangles(raw: &[u8], count: usize, byte_stride: usize) -> Result<Vec<u8>> {
    if byte_stride != 2 && byte_stride != 4 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: TRIANGLES byteStride {byte_stride} must be 2 or 4"
        )));
    }
    if count % 3 != 0 {
        return Err(invalid(
            "KHR_meshopt_compression: TRIANGLES count must be divisible by 3",
        ));
    }
    let triangle_count = count / 3;

    let read_idx = |i: usize| -> u32 {
        let base = i * byte_stride;
        if byte_stride == 2 {
            u16::from_le_bytes([raw[base], raw[base + 1]]) as u32
        } else {
            u32::from_le_bytes([raw[base], raw[base + 1], raw[base + 2], raw[base + 3]])
        }
    };

    // `code` bytes (one per triangle, all 0xff) come first, then the
    // `data` section (the per-corner zw + explicit index varints), then
    // the 16-byte codeaux tail. All-explicit corners never index codeaux,
    // so an all-zero codeaux table satisfies the decoder's validity
    // checks (last two bytes 0, no 0xf nibble).
    let mut out = vec![0xe1u8]; // header
    out.resize(out.len() + triangle_count, 0xffu8); // codes

    let mut last: u32 = 0;
    for t in 0..triangle_count {
        // zw = 0xff → a, b, c all explicit. zw must not be 0x00 (which
        // would reset `next`); 0xff is safe.
        out.push(0xff);
        for corner in 0..3 {
            let idx = read_idx(t * 3 + corner);
            encode_index(&mut out, idx, &mut last);
        }
    }

    out.extend_from_slice(&[0u8; 16]); // codeaux tail (all zero)
    Ok(out)
}

/// Encode one explicit index as a zigzag varint delta vs `last`
/// (inverse of [`decode_index`]). The decoder reads
/// `delta = (v & 1) ? !(v >> 1) : (v >> 1)`.
fn encode_index(out: &mut Vec<u8>, idx: u32, last: &mut u32) {
    let delta = idx.wrapping_sub(*last);
    *last = idx;
    let (sign, w) = zigzag_split_u32(delta);
    let v = (w << 1) | (sign as u32);
    write_varint(out, v);
}

// ---------------------------------------------------------------------------
// Mode 2: indices (§"Mode 2: indices")
// ---------------------------------------------------------------------------

fn decode_indices(data: &[u8], count: usize, byte_stride: usize) -> Result<Vec<u8>> {
    if byte_stride != 2 && byte_stride != 4 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: INDICES byteStride {byte_stride} must be 2 or 4"
        )));
    }
    let header = *data.first().ok_or_else(err_eos)?;
    if header != 0xd1 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: INDICES header byte 0x{header:02x} not 0xd1"
        )));
    }
    // Tail block: 4 padding bytes.
    if data.len() < 1 + 4 {
        return Err(err_eos());
    }
    let mut cur = Cursor::new(&data[1..data.len() - 4]);

    let mut last = [0u32; 2];
    let mut indices: Vec<u32> = Vec::with_capacity(count);
    for _ in 0..count {
        let v = read_varint(&mut cur)?;
        let baseline = (v & 1) as usize;
        let delta = if v & 2 != 0 { !(v >> 2) } else { v >> 2 };
        last[baseline] = last[baseline].wrapping_add(delta);
        indices.push(last[baseline]);
    }

    if !cur.is_empty() {
        return Err(invalid(
            "KHR_meshopt_compression: INDICES stream has leftover bytes before tail",
        ));
    }
    emit_indices(&indices, byte_stride)
}

/// Emit decoded 32-bit indices as the descriptor's element width
/// (UNSIGNED_SHORT with wraparound when `byte_stride == 2`, else
/// UNSIGNED_INT).
fn emit_indices(indices: &[u32], byte_stride: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(indices.len() * byte_stride);
    if byte_stride == 2 {
        for &i in indices {
            out.extend_from_slice(&(i as u16).to_le_bytes());
        }
    } else {
        for &i in indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    Ok(out)
}

/// Encode raw index bytes into a Mode 2 INDICES stream (inverse of
/// [`decode_indices`]). Each index is delta-coded against baseline 0
/// only — a valid, fully-general encoding the spec's decoder accepts
/// (baseline 1 is an optional compression aid the encoder need not use).
fn encode_indices(raw: &[u8], count: usize, byte_stride: usize) -> Result<Vec<u8>> {
    if byte_stride != 2 && byte_stride != 4 {
        return Err(invalid(format!(
            "KHR_meshopt_compression: INDICES byteStride {byte_stride} must be 2 or 4"
        )));
    }
    let mut out = vec![0xd1u8]; // header
    let mut last0: u32 = 0;
    for i in 0..count {
        let base = i * byte_stride;
        let idx = if byte_stride == 2 {
            u16::from_le_bytes([raw[base], raw[base + 1]]) as u32
        } else {
            u32::from_le_bytes([raw[base], raw[base + 1], raw[base + 2], raw[base + 3]])
        };
        let delta = idx.wrapping_sub(last0);
        last0 = idx;
        // Layout (per decode_indices): v = (w << 2) | (sign << 1) | base.
        // base = 0 (baseline 0), sign + w chosen so decode's
        // `delta = sign ? !w : w` reproduces `delta`.
        let (sign, w) = zigzag_split_u32(delta);
        let v = (w << 2) | ((sign as u32) << 1);
        write_varint(&mut out, v);
    }
    out.extend_from_slice(&[0, 0, 0, 0]); // 4-byte tail padding
    Ok(out)
}

/// Choose the `(sign, magnitude)` pair such that the decoder's
/// `delta = sign ? !magnitude : magnitude` (32-bit bitwise-not)
/// reproduces `delta`, minimising the stored magnitude so the varint
/// stays short. Picks `sign = (delta >> 31)`: positives store `delta`
/// directly, negatives store `!delta` (small for near-zero negatives).
fn zigzag_split_u32(delta: u32) -> (bool, u32) {
    if delta & 0x8000_0000 != 0 {
        (true, !delta)
    } else {
        (false, delta)
    }
}

/// varint-7 / unsigned LEB128 writer (inverse of [`read_varint`]).
fn write_varint(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
}

/// §"Mode 1"/"Mode 2": decode a zigzag-encoded signed index delta vs
/// `last`, updating `last` with 32-bit wraparound.
fn decode_index(cur: &mut Cursor, last: &mut u32) -> Result<u32> {
    let v = read_varint(cur)?;
    let delta = if v & 1 != 0 { !(v >> 1) } else { v >> 1 };
    *last = last.wrapping_add(delta);
    Ok(*last)
}

/// varint-7 / unsigned LEB128 reader (§"Mode 1"/"Mode 2"). 1–5 bytes;
/// MSB of the final byte is 0.
fn read_varint(cur: &mut Cursor) -> Result<u32> {
    let mut result: u32 = 0;
    for i in 0..5 {
        let byte = *cur.take(1).ok_or_else(err_eos)?.first().unwrap();
        result |= ((byte & 0x7f) as u32) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(invalid("KHR_meshopt_compression: varint-7 exceeds 5 bytes"))
}

// ---------------------------------------------------------------------------
// Appendix B: filters
// ---------------------------------------------------------------------------

fn apply_filter(buf: &mut [u8], filter: Filter, byte_stride: usize) -> Result<()> {
    match filter {
        Filter::None => Ok(()),
        Filter::Octahedral => filter_octahedral(buf, byte_stride),
        Filter::Quaternion => filter_quaternion(buf, byte_stride),
        Filter::Exponential => filter_exponential(buf, byte_stride),
        Filter::Color => filter_color(buf, byte_stride),
    }
}

fn round_away(x: f32) -> f32 {
    // Round half away from zero, matching the spec's `round`.
    if x >= 0.0 {
        (x + 0.5).floor()
    } else {
        (x - 0.5).ceil()
    }
}

/// §"Filter 1: octahedral".
fn filter_octahedral(buf: &mut [u8], byte_stride: usize) -> Result<()> {
    if byte_stride != 4 && byte_stride != 8 {
        return Err(invalid(
            "KHR_meshopt_compression: OCTAHEDRAL filter requires byteStride 4 or 8",
        ));
    }
    let comp16 = byte_stride == 8;
    let int_max: f32 = if comp16 { 32767.0 } else { 127.0 };
    let n = buf.len() / byte_stride;
    for e in 0..n {
        let base = e * byte_stride;
        let (i0, i1, i2, i3);
        if comp16 {
            i0 = i16::from_le_bytes([buf[base], buf[base + 1]]) as f32;
            i1 = i16::from_le_bytes([buf[base + 2], buf[base + 3]]) as f32;
            i2 = i16::from_le_bytes([buf[base + 4], buf[base + 5]]) as f32;
            i3 = [buf[base + 6], buf[base + 7]];
        } else {
            i0 = (buf[base] as i8) as f32;
            i1 = (buf[base + 1] as i8) as f32;
            i2 = (buf[base + 2] as i8) as f32;
            i3 = [buf[base + 3], 0];
        }
        let one = i2;
        let mut x = i0 / one;
        let mut y = i1 / one;
        let mut z = 1.0 - x.abs() - y.abs();
        let t = z.min(0.0);
        x -= t.copysign(x);
        y -= t.copysign(y);
        let len = (x * x + y * y + z * z).sqrt();
        x /= len;
        y /= len;
        z /= len;
        let o0 = round_away(x * int_max);
        let o1 = round_away(y * int_max);
        let o2 = round_away(z * int_max);
        if comp16 {
            buf[base..base + 2].copy_from_slice(&(o0 as i16).to_le_bytes());
            buf[base + 2..base + 4].copy_from_slice(&(o1 as i16).to_le_bytes());
            buf[base + 4..base + 6].copy_from_slice(&(o2 as i16).to_le_bytes());
            // i3 passed through verbatim (already in place).
            let _ = i3;
        } else {
            buf[base] = (o0 as i16 as i8) as u8;
            buf[base + 1] = (o1 as i16 as i8) as u8;
            buf[base + 2] = (o2 as i16 as i8) as u8;
            // i3[0] (the 4th input byte) already in place.
        }
    }
    Ok(())
}

/// §"Filter 2: quaternion".
fn filter_quaternion(buf: &mut [u8], byte_stride: usize) -> Result<()> {
    if byte_stride != 8 {
        return Err(invalid(
            "KHR_meshopt_compression: QUATERNION filter requires byteStride 8",
        ));
    }
    let range = 1.0f32 / 2.0f32.sqrt();
    let n = buf.len() / 8;
    for e in 0..n {
        let base = e * 8;
        let i0 = i16::from_le_bytes([buf[base], buf[base + 1]]) as f32;
        let i1 = i16::from_le_bytes([buf[base + 2], buf[base + 3]]) as f32;
        let i2 = i16::from_le_bytes([buf[base + 4], buf[base + 5]]) as f32;
        let i3 = i16::from_le_bytes([buf[base + 6], buf[base + 7]]);
        let one = (i3 | 3) as f32;
        let x = i0 / one * range;
        let y = i1 / one * range;
        let z = i2 / one * range;
        let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
        let maxcomp = (i3 & 3) as usize;
        let mut out = [0i16; 4];
        out[(maxcomp + 1) % 4] = round_away(x * 32767.0) as i16;
        out[(maxcomp + 2) % 4] = round_away(y * 32767.0) as i16;
        out[(maxcomp + 3) % 4] = round_away(z * 32767.0) as i16;
        out[maxcomp % 4] = round_away(w * 32767.0) as i16;
        for (k, &o) in out.iter().enumerate() {
            buf[base + k * 2..base + k * 2 + 2].copy_from_slice(&o.to_le_bytes());
        }
    }
    Ok(())
}

/// §"Filter 3: exponential".
fn filter_exponential(buf: &mut [u8], byte_stride: usize) -> Result<()> {
    if byte_stride == 0 || byte_stride % 4 != 0 {
        return Err(invalid(
            "KHR_meshopt_compression: EXPONENTIAL filter requires byteStride a multiple of 4",
        ));
    }
    let n = buf.len() / 4;
    for e in 0..n {
        let base = e * 4;
        let input = i32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
        let exp = input >> 24; // arithmetic shift → signed exponent
        let mant = (input << 8) >> 8; // sign-extend the 24-bit mantissa
        let value = 2.0f32.powi(exp) * (mant as f32);
        buf[base..base + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

/// §"Filter 4: color".
fn filter_color(buf: &mut [u8], byte_stride: usize) -> Result<()> {
    if byte_stride != 4 && byte_stride != 8 {
        return Err(invalid(
            "KHR_meshopt_compression: COLOR filter requires byteStride 4 or 8",
        ));
    }
    let comp16 = byte_stride == 8;
    let uint_max: f32 = if comp16 { 65535.0 } else { 255.0 };
    let n = buf.len() / byte_stride;
    for e in 0..n {
        let base = e * byte_stride;
        let (in0, in1, in2, in3): (i32, i32, i32, i32);
        if comp16 {
            in0 = u16::from_le_bytes([buf[base], buf[base + 1]]) as i32;
            in1 = i16::from_le_bytes([buf[base + 2], buf[base + 3]]) as i32;
            in2 = i16::from_le_bytes([buf[base + 4], buf[base + 5]]) as i32;
            in3 = u16::from_le_bytes([buf[base + 6], buf[base + 7]]) as i32;
        } else {
            in0 = buf[base] as i32;
            in1 = (buf[base + 1] as i8) as i32;
            in2 = (buf[base + 2] as i8) as i32;
            in3 = buf[base + 3] as i32;
        }
        // recover scale from alpha high bit: as = (1 << (findMSB+1)) - 1
        let msb = find_msb(in3 as u32);
        if msb < 0 {
            return Err(invalid(
                "KHR_meshopt_compression: COLOR filter alpha component has no set bit",
            ));
        }
        let as_ = (1i32 << (msb + 1)) - 1;
        let y = in0;
        let co = in1;
        let cg = in2;
        let r = y + co - cg;
        let g = y + cg;
        let b = y - co - cg;
        // expand alpha by one bit, replicating LSB.
        let mut a = in3 & (as_ >> 1);
        a = (a << 1) | (a & 1);
        let ss = uint_max / (as_ as f32);
        let o0 = round_away(r as f32 * ss);
        let o1 = round_away(g as f32 * ss);
        let o2 = round_away(b as f32 * ss);
        let o3 = round_away(a as f32 * ss);
        if comp16 {
            buf[base..base + 2].copy_from_slice(&(o0 as u16).to_le_bytes());
            buf[base + 2..base + 4].copy_from_slice(&(o1 as u16).to_le_bytes());
            buf[base + 4..base + 6].copy_from_slice(&(o2 as u16).to_le_bytes());
            buf[base + 6..base + 8].copy_from_slice(&(o3 as u16).to_le_bytes());
        } else {
            buf[base] = o0 as u8;
            buf[base + 1] = o1 as u8;
            buf[base + 2] = o2 as u8;
            buf[base + 3] = o3 as u8;
        }
    }
    Ok(())
}

/// Position of the most significant set bit (0-based), or -1 if none.
fn find_msb(v: u32) -> i32 {
    if v == 0 {
        -1
    } else {
        31 - v.leading_zeros() as i32
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// A forward byte cursor that never panics on over-read.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }
    /// Take `n` bytes, advancing the cursor; `None` if fewer remain.
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.data.len() {
            return None;
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Some(s)
    }
    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// 16-entry FIFO of single vertex indices (§"Mode 1"). Push wraps; read
/// of an entry never written (within the first 16 pushes) is an error.
struct Fifo1 {
    buf: [u32; 16],
    len: usize,
    head: usize,
}

impl Fifo1 {
    fn new() -> Self {
        Fifo1 {
            buf: [0; 16],
            len: 0,
            head: 0,
        }
    }
    fn push(&mut self, v: u32) {
        self.head = (self.head + 15) % 16; // step back one slot
        self.buf[self.head] = v;
        if self.len < 16 {
            self.len += 1;
        }
    }
    /// Index 0 = most recently added.
    fn get(&self, i: usize) -> Result<u32> {
        if i >= self.len {
            return Err(invalid(
                "KHR_meshopt_compression: vertex FIFO read of unwritten entry",
            ));
        }
        Ok(self.buf[(self.head + i) % 16])
    }
}

/// 16-entry FIFO of edge (a, b) index pairs (§"Mode 1").
struct Fifo2 {
    buf: [(u32, u32); 16],
    len: usize,
    head: usize,
}

impl Fifo2 {
    fn new() -> Self {
        Fifo2 {
            buf: [(0, 0); 16],
            len: 0,
            head: 0,
        }
    }
    fn push(&mut self, a: u32, b: u32) {
        self.head = (self.head + 15) % 16;
        self.buf[self.head] = (a, b);
        if self.len < 16 {
            self.len += 1;
        }
    }
    /// Index 0 = most recently added edge.
    fn get(&self, i: usize) -> Result<(u32, u32)> {
        if i >= self.len {
            return Err(invalid(
                "KHR_meshopt_compression: edge FIFO read of unwritten entry",
            ));
        }
        Ok(self.buf[(self.head + i) % 16])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint7_examples() {
        // §"Mode 1"/"Mode 2" normatively define the encoding as
        // "unsigned LEB128" — i.e. the low 7 bits of the FIRST byte are
        // the least-significant 7 bits of the value (LSB-first). The
        // spec's first two worked examples agree with that:
        //   0x7f       => 0x7f
        //   0x81 0x04  => 0x201   (0x01 | (0x04 << 7))
        let mut c = Cursor::new(&[0x7f]);
        assert_eq!(read_varint(&mut c).unwrap(), 0x7f);
        let mut c = Cursor::new(&[0x81, 0x04]);
        assert_eq!(read_varint(&mut c).unwrap(), 0x201);
        // NOTE — the spec's THIRD worked example (`0xff 0xa0 0x05 =>
        // 0x1fd005`) is only reproducible under an MSB-first reading,
        // which contradicts both the normative "unsigned LEB128" wording
        // and the second example. We follow the normative LEB128
        // definition (LSB-first), so this byte sequence decodes to
        //   0x7f | (0x20 << 7) | (0x05 << 14) = 0x1507f.
        // The example is reported as a docs erratum.
        let mut c = Cursor::new(&[0xff, 0xa0, 0x05]);
        assert_eq!(read_varint(&mut c).unwrap(), 0x1507f);
    }

    #[test]
    fn zigzag_roundtrip_u8() {
        // encode(v) = ((v&0x80)!=0) ? ~(v<<1) : (v<<1)
        for v in 0u8..=255 {
            let enc = if v & 0x80 != 0 {
                !(v.wrapping_shl(1))
            } else {
                v.wrapping_shl(1)
            };
            assert_eq!(zigzag_decode_u8(enc), v, "v={v}");
        }
    }

    #[test]
    fn unpack_4bit_example() {
        // §"Mode 0": 4-bit sentinel example.
        // 0x17 0x5f 0xf0 0xbc 0x77 0xa9 0x21 0x00 0x34 0xb5
        // → de-zigzagged deltas:
        // -1 -4 -3 26 -91 0 -6 6 -4 -4 5 -5 1 -1 0 0
        let packed = [0x17u8, 0x5f, 0xf0, 0xbc, 0x77, 0xa9, 0x21, 0x00];
        let escapes = [0x34u8, 0xb5];
        let sentinel = 0x0f;
        let mut raw = [0u8; 16];
        for (i, slot) in raw.iter_mut().enumerate() {
            *slot = unpack_delta(&packed, 4, i);
        }
        // The two sentinels in the example are at element positions 3
        // and 4 (per the spec's prose), replaced by escapes.
        let mut esc_iter = escapes.iter();
        let mut decoded = [0i8; 16];
        for (i, &s) in raw.iter().enumerate() {
            let byte = if s == sentinel {
                *esc_iter.next().unwrap()
            } else {
                s
            };
            decoded[i] = zigzag_decode_u8(byte) as i8;
        }
        let expected: [i8; 16] = [-1, -4, -3, 26, -91, 0, -6, 6, -4, -4, 5, -5, 1, -1, 0, 0];
        assert_eq!(decoded, expected);
    }

    #[test]
    fn rotate_left_matches_spec() {
        assert_eq!(rotate_left(0x0000_0001, 4), 0x0000_0010);
        assert_eq!(rotate_left(0x8000_0000, 1), 0x0000_0001);
        assert_eq!(rotate_left(0x1234_5678, 0), 0x1234_5678);
    }

    #[test]
    fn find_msb_basic() {
        assert_eq!(find_msb(0), -1);
        assert_eq!(find_msb(1), 0);
        assert_eq!(find_msb(0x80), 7);
        assert_eq!(find_msb(0xffff), 15);
    }

    #[test]
    fn indices_mode_simple_delta() {
        // Hand-build a Mode 2 INDICES stream for indices [0,1,2,3]
        // using baseline 0 only: each delta = +1, baseline bit 0.
        // encode: v = (delta << 2) | (baseline << ... ) per decode():
        //   baseline = v&1, delta_zigzag = v>>2 (with v&2 sign).
        // For delta=+1 (positive): zigzag(+1) = 2 → bits above baseline
        //   shifted: v = (2 << 1)? Let's derive from decode():
        //   delta = (v&2)? ~(v>>2) : (v>>2); want delta=1, baseline=0.
        //   pick v=4: v&1=0 (baseline 0), v&2=0 → delta=v>>2=1. ✓
        // First index is delta vs last[0]=0 → 0? We want 0,1,2,3.
        //   index0: delta +0 → v=0 (baseline0, delta0). last[0]=0.
        //   index1..3: v=4 each → +1.
        let mut stream = vec![0xd1u8]; // header
        stream.push(0x00); // v=0 → index 0
        stream.push(0x04); // +1 → 1
        stream.push(0x04); // +1 → 2
        stream.push(0x04); // +1 → 3
        stream.extend_from_slice(&[0, 0, 0, 0]); // 4-byte tail padding
        let out = decode(&stream, Mode::Indices, Filter::None, 4, 4).unwrap();
        let got: Vec<u32> = out
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(got, vec![0, 1, 2, 3]);
    }

    #[test]
    fn indices_mode_u16_wraparound() {
        let mut stream = vec![0xd1u8];
        stream.push(0x00); // 0
        stream.push(0x04); // 1
        stream.extend_from_slice(&[0, 0, 0, 0]);
        let out = decode(&stream, Mode::Indices, Filter::None, 2, 2).unwrap();
        let got: Vec<u16> = out
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(got, vec![0, 1]);
    }

    #[test]
    fn attributes_v0_all_zero_deltas() {
        // v0 stream, byteStride 4, count 3. One block, one group.
        // All delta-encoding-mode 0 (all 16 deltas zero) for every byte
        // position → every element equals the baseline.
        let byte_stride = 4usize;
        let count = 3usize;
        let baseline = [0x11u8, 0x22, 0x33, 0x44];
        let mut stream = vec![0xa0u8]; // v0 header
                                       // For each of 4 byte positions: header bits for 1 group.
                                       // groupCount=1 → 1 header byte, bits hb=0 → mode 0 (all zero,
                                       // 0 data bytes). 0x00 selects hb=0 for group 0.
        stream.resize(1 + byte_stride, 0x00);
        // Tail padding to 32 bytes minimum for v0, then tail block
        // (baseline only for v0). Compute current body length.
        // body = stream[1..], tail = baseline (4 bytes).
        // The spec pads the tail block region; for our decoder we only
        // require the tail to be findable. Append baseline at the very
        // end, padding the gap with zeros.
        // Decoder reads tail from the END, body is [1..tail_start].
        // No extra padding needed for correctness of our reader since
        // it computes tail_start = len - tail_len. Keep body tight.
        stream.extend_from_slice(&baseline);
        let out = decode(&stream, Mode::Attributes, Filter::None, count, byte_stride).unwrap();
        assert_eq!(out.len(), count * byte_stride);
        for e in 0..count {
            assert_eq!(&out[e * 4..e * 4 + 4], &baseline);
        }
    }

    #[test]
    fn octahedral_decodes_unit_vector() {
        // byteStride 4: input four i8 [x,y,one,w]; one encodes 1.0 as
        // (1<<(K-1))-1 for K=8 → 127. Pick x=127,y=0 → decodes to a unit
        // vector roughly (+X). z = 1 - 1 - 0 = 0.
        let mut buf = vec![127i8 as u8, 0, 127, 0];
        filter_octahedral(&mut buf, 4).unwrap();
        let x = buf[0] as i8;
        let y = buf[1] as i8;
        let z = buf[2] as i8;
        // Expect approx (127, 0, 0).
        assert!((x as i32 - 127).abs() <= 1, "x={x}");
        assert_eq!(y, 0);
        assert!(z.abs() <= 1, "z={z}");
    }

    #[test]
    fn exponential_decodes_power_of_two() {
        // input: e=0, m=1 → 2^0 * 1 = 1.0. Pack as i32 little-endian
        // with e in high byte: (0 << 24) | 1 = 1.
        let input: i32 = 1;
        let mut buf = input.to_le_bytes().to_vec();
        filter_exponential(&mut buf, 4).unwrap();
        let v = f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        assert_eq!(v, 1.0);
        // e=2, m=3 → 12.0.
        let input: i32 = (2 << 24) | 3;
        let mut buf = input.to_le_bytes().to_vec();
        filter_exponential(&mut buf, 4).unwrap();
        let v = f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        assert_eq!(v, 12.0);
    }

    #[test]
    fn bad_header_rejected() {
        assert!(decode(&[0x00, 0, 0, 0, 0], Mode::Indices, Filter::None, 0, 4).is_err());
        assert!(decode(&[0x00; 8], Mode::Triangles, Filter::None, 3, 4).is_err());
        assert!(decode(&[0x00; 8], Mode::Attributes, Filter::None, 1, 4).is_err());
    }

    #[test]
    fn truncated_stream_rejected() {
        assert!(decode(&[0xd1], Mode::Indices, Filter::None, 1, 4).is_err());
    }

    // -- encode round-trip coverage ---------------------------------------

    fn rt(mode: Mode, raw: &[u8], count: usize, byte_stride: usize) {
        let enc = encode(raw, mode, Filter::None, count, byte_stride).unwrap();
        let dec = decode(&enc, mode, Filter::None, count, byte_stride).unwrap();
        assert_eq!(dec, raw, "{mode:?} round-trip mismatch");
    }

    #[test]
    fn zigzag_encode_u8_inverts_decode() {
        for v in 0u8..=255 {
            assert_eq!(zigzag_encode_u8(zigzag_decode_u8(v)), {
                // canonical encoding is unique, so re-encoding the decoded
                // value must reproduce the original byte for every v.
                v
            });
        }
    }

    #[test]
    fn write_varint_inverts_read() {
        for &v in &[0u32, 1, 0x7f, 0x80, 0x201, 0x1507f, 0xffff_ffff] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let mut c = Cursor::new(&buf);
            assert_eq!(read_varint(&mut c).unwrap(), v);
            assert!(c.is_empty());
        }
    }

    #[test]
    fn encode_indices_roundtrip_u32() {
        let idx: [u32; 6] = [0, 1, 2, 2, 3, 0];
        let mut raw = Vec::new();
        for &i in &idx {
            raw.extend_from_slice(&i.to_le_bytes());
        }
        rt(Mode::Indices, &raw, idx.len(), 4);
    }

    #[test]
    fn encode_indices_roundtrip_u16_and_backwards_deltas() {
        // Includes a large jump then a backwards delta to exercise both
        // zigzag sign paths.
        let idx: [u16; 8] = [0, 100, 50, 51, 200, 199, 0, 65535];
        let mut raw = Vec::new();
        for &i in &idx {
            raw.extend_from_slice(&i.to_le_bytes());
        }
        rt(Mode::Indices, &raw, idx.len(), 2);
    }

    #[test]
    fn encode_triangles_roundtrip() {
        // Two triangles sharing an edge (0,1,2) + (2,1,3).
        let idx: [u32; 6] = [0, 1, 2, 2, 1, 3];
        let mut raw = Vec::new();
        for &i in &idx {
            raw.extend_from_slice(&i.to_le_bytes());
        }
        rt(Mode::Triangles, &raw, idx.len(), 4);
    }

    #[test]
    fn encode_triangles_roundtrip_u16() {
        let idx: [u16; 9] = [0, 1, 2, 3, 4, 5, 5, 4, 0];
        let mut raw = Vec::new();
        for &i in &idx {
            raw.extend_from_slice(&i.to_le_bytes());
        }
        rt(Mode::Triangles, &raw, idx.len(), 2);
    }

    #[test]
    fn encode_attributes_roundtrip_small() {
        // 3 elements, stride 4 — picks varied group widths per byte pos.
        let raw: Vec<u8> = vec![
            0x11, 0x22, 0x33, 0x44, // e0
            0x12, 0x22, 0x40, 0x44, // e1: +1, 0, +13, 0
            0x10, 0x52, 0x33, 0xc4, // e2: -2, +0x30, -13, +0x80
        ];
        rt(Mode::Attributes, &raw, 3, 4);
    }

    #[test]
    fn encode_attributes_roundtrip_large_multi_block_multi_group() {
        // stride 8, 40 elements → multiple groups (3 groups) in one block.
        let count = 40;
        let stride = 8;
        let mut raw = vec![0u8; count * stride];
        // Deterministic pseudo-random-ish fill exercising all delta sizes.
        let mut s: u32 = 0x1234_5678;
        for b in raw.iter_mut() {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *b = (s >> 24) as u8;
        }
        rt(Mode::Attributes, &raw, count, stride);
    }

    #[test]
    fn encode_attributes_roundtrip_all_zero() {
        // Every byte identical → all-zero deltas → hb 0 groups.
        let raw = vec![0x7eu8; 5 * 4];
        rt(Mode::Attributes, &raw, 5, 4);
    }

    #[test]
    fn encode_attributes_roundtrip_zero_count() {
        rt(Mode::Attributes, &[], 0, 4);
        rt(Mode::Indices, &[], 0, 4);
        rt(Mode::Triangles, &[], 0, 4);
    }

    #[test]
    fn encode_rejects_non_none_filter() {
        assert!(encode(&[0u8; 8], Mode::Attributes, Filter::Octahedral, 2, 4).is_err());
    }

    #[test]
    fn encode_rejects_size_mismatch() {
        assert!(encode(&[0u8; 7], Mode::Attributes, Filter::None, 2, 4).is_err());
    }

    #[test]
    fn attributes_encode_shrinks_smooth_data() {
        // 256 VEC3 f32 positions on a smooth ramp → small per-element
        // byte deltas → the v0 group bit-width selection should compress
        // well below the raw 256*12 = 3072 bytes.
        let count = 256;
        let stride = 12;
        let mut raw = Vec::with_capacity(count * stride);
        for i in 0..count {
            let x = i as f32 * 0.01;
            for c in [x, x * 0.5, -x] {
                raw.extend_from_slice(&c.to_le_bytes());
            }
        }
        let enc = encode(&raw, Mode::Attributes, Filter::None, count, stride).unwrap();
        assert!(
            enc.len() < raw.len(),
            "smooth attributes should shrink: {} vs raw {}",
            enc.len(),
            raw.len()
        );
        // And still round-trip exactly.
        let dec = decode(&enc, Mode::Attributes, Filter::None, count, stride).unwrap();
        assert_eq!(dec, raw);
    }

    #[test]
    fn indices_encode_shrinks_sequential_data() {
        // A sequential triangle strip-ish index list compresses to ~1
        // byte per index (delta +1 → varint single byte) vs 4 raw.
        let count = 300;
        let stride = 4;
        let mut raw = Vec::with_capacity(count * stride);
        for i in 0..count as u32 {
            raw.extend_from_slice(&i.to_le_bytes());
        }
        let enc = encode(&raw, Mode::Indices, Filter::None, count, stride).unwrap();
        assert!(
            enc.len() < raw.len(),
            "sequential indices should shrink: {} vs raw {}",
            enc.len(),
            raw.len()
        );
        let dec = decode(&enc, Mode::Indices, Filter::None, count, stride).unwrap();
        assert_eq!(dec, raw);
    }
}
