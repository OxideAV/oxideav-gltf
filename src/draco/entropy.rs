//! Entropy decoders of the compressed-mesh bitstream
//! (`docs/3d/gltf/draco/spec/03-entropy-coding.md`): the multi-symbol rANS
//! decoder with its serialised frequency table, the two symbol-sequence
//! schemes built on it, and the fixed-probability binary rANS decoder.
//!
//! Both decoders consume their byte stream from its last byte toward its
//! first.

use super::reader::{BitRun, Reader};
use crate::error::{Result, invalid};

/// Precision of the tag decoder of the tagged scheme (§2).
const TAG_PRECISION: u32 = 12;

/// Reusable buffers for frequency tables, so decoding a symbol sequence
/// allocates nothing once the buffers have grown to their working size.
#[derive(Debug, Default)]
pub(crate) struct EntropyScratch {
    /// `freq[s]` of the current table.
    freq: Vec<u32>,
    /// `cum[s]` of the current table (same length as `freq`).
    cum: Vec<u32>,
    /// Direct map from the low `precision_bits` of the state to the symbol;
    /// empty when the sequence is too short to amortise building it.
    slot: Vec<u32>,
}

impl EntropyScratch {
    /// Read a frequency table (§1.1) for `precision_bits`; `values` is the
    /// number of symbols that will be decoded with it and only steers the
    /// choice between the direct map and a binary search.
    fn read_table(&mut self, r: &mut Reader<'_>, precision_bits: u32, values: usize) -> Result<()> {
        let total = 1u32 << precision_bits;
        let n = r.count()?;
        // Every token byte assigns at most 64 symbols.
        if n > r.remaining().saturating_mul(64) {
            return Err(invalid("draco: frequency table larger than the payload"));
        }
        self.freq.clear();
        self.freq.resize(n, 0);
        let mut i = 0usize;
        while i < n {
            let b = r.u8()?;
            let low = b & 3;
            if low == 3 {
                let run = usize::from(b >> 2) + 1;
                if run > n - i {
                    return Err(invalid("draco: zero-frequency run past the alphabet"));
                }
                i += run;
            } else {
                let mut f = u32::from(b >> 2);
                if low >= 1 {
                    f |= u32::from(r.u8()?) << 6;
                }
                if low == 2 {
                    f |= u32::from(r.u8()?) << 14;
                }
                self.freq[i] = f;
                i += 1;
            }
        }
        self.cum.clear();
        self.cum.reserve(n);
        let mut acc = 0u64;
        for &f in &self.freq {
            self.cum.push(acc as u32);
            acc += u64::from(f);
            if acc > u64::from(total) {
                return Err(invalid("draco: frequencies exceed the rANS total"));
            }
        }
        if acc != u64::from(total) {
            return Err(invalid("draco: frequencies do not sum to the rANS total"));
        }
        self.slot.clear();
        if values.saturating_mul(16) >= total as usize {
            self.slot.resize(total as usize, 0);
            for (s, (&f, &c)) in self.freq.iter().zip(&self.cum).enumerate() {
                if f != 0 {
                    self.slot[c as usize..(c + f) as usize].fill(s as u32);
                }
            }
        }
        Ok(())
    }

    /// The symbol the residue `r` belongs to.
    #[inline]
    fn symbol_of(&self, r: u32) -> usize {
        if self.slot.is_empty() {
            // Last symbol whose cumulative frequency is <= r; zero-frequency
            // symbols share their successor's value and are skipped by taking
            // the last match.
            self.cum.partition_point(|&c| c <= r) - 1
        } else {
            self.slot[r as usize] as usize
        }
    }
}

/// State of a multi-symbol rANS decoder (§1.2, §1.3).
#[derive(Debug)]
struct SymbolDecoder<'a> {
    buf: &'a [u8],
    rem: usize,
    state: u32,
    precision_bits: u32,
}

/// Initial state from the tail of `buf` with lower bound `low` (§1.2).
/// Returns `(state, rem)`.
fn initial_state(buf: &[u8], low: u32, max_extra: usize) -> Result<(u32, usize)> {
    let m = buf.len();
    if m == 0 {
        return Err(invalid("draco: empty rANS stream"));
    }
    let k = usize::from(buf[m - 1] >> 6);
    if k > max_extra || m < k + 1 {
        return Err(invalid("draco: bad rANS state header"));
    }
    let mut raw = 0u32;
    for (j, &b) in buf[m - 1 - k..].iter().enumerate() {
        raw |= u32::from(b) << (8 * j);
    }
    raw &= (1u32 << (8 * (k as u32 + 1) - 2)) - 1;
    let state = raw + low;
    if u64::from(state) >= 256 * u64::from(low) {
        return Err(invalid("draco: rANS initial state out of range"));
    }
    Ok((state, m - 1 - k))
}

impl<'a> SymbolDecoder<'a> {
    /// Read `varint stream_size` + the stream bytes and initialise.
    fn read(r: &mut Reader<'a>, precision_bits: u32) -> Result<Self> {
        let m = r.varint64()?;
        if m > r.remaining() as u64 {
            return Err(invalid("draco: rANS stream larger than the payload"));
        }
        let buf = r.take(m as usize)?;
        let (state, rem) = initial_state(buf, 4u32 << precision_bits, 3)?;
        Ok(Self {
            buf,
            rem,
            state,
            precision_bits,
        })
    }

    #[inline]
    fn symbol(&mut self, table: &EntropyScratch) -> u32 {
        let low = 4u32 << self.precision_bits;
        while self.state < low && self.rem > 0 {
            self.rem -= 1;
            self.state = (self.state << 8) | u32::from(self.buf[self.rem]);
        }
        let q = self.state >> self.precision_bits;
        let r = self.state & ((1u32 << self.precision_bits) - 1);
        let s = table.symbol_of(r);
        // q * freq + r - cum <= state for a table summing to the total, so
        // none of this can overflow; wrapping keeps a corrupted state from
        // panicking in debug builds.
        self.state = q
            .wrapping_mul(table.freq[s])
            .wrapping_add(r)
            .wrapping_sub(table.cum[s]);
        s as u32
    }
}

/// Decode a symbol sequence (§3) of `n` values grouped in tuples of `c`
/// components into `out` (cleared first). Nothing is read when `n == 0`.
pub(crate) fn read_symbols(
    r: &mut Reader<'_>,
    n: usize,
    c: usize,
    scratch: &mut EntropyScratch,
    out: &mut Vec<u32>,
) -> Result<()> {
    out.clear();
    if n == 0 {
        return Ok(());
    }
    debug_assert!(c > 0 && n.is_multiple_of(c));
    // Grow as values arrive rather than trusting `n` for one big reservation:
    // a symbol can cost zero stream bytes, so `n` is not bounded by the
    // payload size (callers bound it by their own limits).
    out.reserve(n.min(1 << 16));
    match r.u8()? {
        0 => {
            scratch.read_table(r, TAG_PRECISION, n / c)?;
            let mut dec = SymbolDecoder::read(r, TAG_PRECISION)?;
            let mut run = BitRun::new(r.rest());
            for _ in 0..n / c {
                let t = dec.symbol(scratch);
                if t > 32 {
                    return Err(invalid("draco: tagged bit length above 32"));
                }
                for _ in 0..c {
                    out.push(run.bits(t)?);
                }
            }
            r.take(run.bytes_used())?;
        }
        1 => {
            let b = u32::from(r.u8()?);
            if !(1..=18).contains(&b) {
                return Err(invalid("draco: raw-scheme bit length out of range"));
            }
            let precision_bits = (3 * b / 2).clamp(12, 20);
            scratch.read_table(r, precision_bits, n)?;
            let mut dec = SymbolDecoder::read(r, precision_bits)?;
            for _ in 0..n {
                out.push(dec.symbol(scratch));
            }
        }
        _ => return Err(invalid("draco: unknown symbol-sequence scheme")),
    }
    Ok(())
}

/// Binary rANS decoder with one fixed probability (§4).
#[derive(Clone, Debug)]
pub(crate) struct BitDecoder<'a> {
    buf: &'a [u8],
    rem: usize,
    state: u32,
    /// Probability of a 1 bit in 1/256 units (1..=256).
    p1: u32,
}

impl<'a> BitDecoder<'a> {
    /// Read `u8 prob_zero`, `varint stream_size`, the stream bytes, and
    /// initialise.
    pub(crate) fn read(r: &mut Reader<'a>) -> Result<Self> {
        let prob_zero = u32::from(r.u8()?);
        let m = r.count()?;
        if m > r.remaining() {
            return Err(invalid("draco: bit stream larger than the payload"));
        }
        let buf = r.take(m)?;
        let (state, rem) = initial_state(buf, 4096, 2)?;
        Ok(Self {
            buf,
            rem,
            state,
            p1: 256 - prob_zero,
        })
    }

    /// Decode one bit.
    #[inline]
    pub(crate) fn bit(&mut self) -> bool {
        if self.state < 4096 && self.rem > 0 {
            self.rem -= 1;
            self.state = (self.state << 8) | u32::from(self.buf[self.rem]);
        }
        let q = self.state >> 8;
        let r = self.state & 0xff;
        if r < self.p1 {
            self.state = q * self.p1 + r;
            true
        } else {
            self.state = self.state - q * self.p1 - self.p1;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `seq-quad-idxdiff@14..28`: ch. 03 §1–§3.1 worked example.
    const TAGGED: [u8; 15] = [
        0x00, 0x04, 0x03, 0xad, 0x0a, 0x01, 0x20, 0x55, 0x15, 0x03, 0xcd, 0x05, 0x70, 0xb4, 0x14,
    ];

    #[test]
    fn frequency_table_example() {
        let mut s = EntropyScratch::default();
        let mut r = Reader::new(&TAGGED[1..]);
        s.read_table(&mut r, 12, 6).unwrap();
        assert_eq!(s.freq, [0, 683, 2048, 1365]);
        assert_eq!(s.cum, [0, 0, 683, 2731]);
        let mut d = SymbolDecoder::read(&mut r, 12).unwrap();
        assert_eq!((d.state, d.rem), (28677, 1));
        assert_eq!(d.symbol(&s), 1);
        assert_eq!(d.state, 4786);
        assert_eq!(d.symbol(&s), 2);
        assert_eq!(d.state, 612386);
        assert_eq!(d.symbol(&s), 2);
        assert_eq!(d.state, 306551);
    }

    #[test]
    fn direct_map_and_search_agree() {
        for values in [1usize, 100_000] {
            let mut s = EntropyScratch::default();
            let mut r = Reader::new(&TAGGED[1..]);
            s.read_table(&mut r, 12, values).unwrap();
            assert_eq!(s.slot.is_empty(), values == 1);
            for res in 0..4096u32 {
                let want = if res < 683 {
                    1
                } else if res < 2731 {
                    2
                } else {
                    3
                };
                assert_eq!(s.symbol_of(res), want);
            }
        }
    }

    #[test]
    fn tagged_sequence_example() {
        let mut s = EntropyScratch::default();
        let mut out = Vec::new();
        let mut r = Reader::new(&TAGGED);
        read_symbols(&mut r, 6, 1, &mut s, &mut out).unwrap();
        assert_eq!(out, [0, 2, 2, 5, 4, 2]);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn empty_sequence_reads_nothing() {
        let mut s = EntropyScratch::default();
        let mut out = vec![7];
        let mut r = Reader::new(&[0xff]);
        read_symbols(&mut r, 0, 1, &mut s, &mut out).unwrap();
        assert!(out.is_empty());
        assert_eq!(r.remaining(), 1);
    }

    #[test]
    fn binary_decoder_example() {
        // `eb-quad@20..22`.
        let mut r = Reader::new(&[0xff, 0x01, 0x11]);
        let mut d = BitDecoder::read(&mut r).unwrap();
        assert_eq!(d.state, 4113);
        assert!(!d.bit());
        assert_eq!(d.state, 4096);
    }

    #[test]
    fn malformed_tables_are_rejected() {
        let mut s = EntropyScratch::default();
        // Sum below the total.
        assert!(
            s.read_table(&mut Reader::new(&[0x01, 0x04]), 12, 1)
                .is_err()
        );
        // Zero run past the alphabet.
        assert!(
            s.read_table(&mut Reader::new(&[0x02, 0x0b]), 12, 1)
                .is_err()
        );
        // Alphabet far larger than the payload.
        assert!(
            s.read_table(&mut Reader::new(&[0xff, 0xff, 0x03, 0x03]), 12, 1)
                .is_err()
        );
        // Empty streams and a 4-byte state for the binary decoder.
        assert!(BitDecoder::read(&mut Reader::new(&[0x80, 0x00])).is_err());
        assert!(BitDecoder::read(&mut Reader::new(&[0x80, 0x04, 0, 0, 0, 0xc0])).is_err());
    }
}
