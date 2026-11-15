//! Byte-level primitives of the compressed-mesh bitstream: little-endian
//! fixed-width fields, varints and bit runs
//! (`docs/3d/gltf/draco/spec/02-primitives.md`).

use crate::error::{Result, invalid};

/// Forward cursor over the payload.
#[derive(Clone, Debug)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// The unread tail of the payload (does not advance).
    pub(crate) fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// Take the next `n` bytes.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            return Err(invalid("draco: payload truncated"));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Varint of at most `max_bytes` 7-bit groups (ch. 02 §2).
    fn varint(&mut self, max_bytes: u32) -> Result<u64> {
        let mut value = 0u64;
        for k in 0..max_bytes {
            let b = self.u8()?;
            let group = u64::from(b & 0x7f);
            let shift = 7 * k;
            if shift == 63 && group > 1 {
                return Err(invalid("draco: varint exceeds 64 bits"));
            }
            value |= group << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(invalid("draco: varint too long"))
    }

    pub(crate) fn varint32(&mut self) -> Result<u32> {
        u32::try_from(self.varint(5)?).map_err(|_| invalid("draco: varint exceeds 32 bits"))
    }

    pub(crate) fn varint64(&mut self) -> Result<u64> {
        self.varint(10)
    }

    /// A varint used as an element count, converted to `usize`.
    pub(crate) fn count(&mut self) -> Result<usize> {
        Ok(self.varint32()? as usize)
    }
}

/// Reader of a bit run (ch. 02 §3): least-significant bit first inside each
/// byte, first bit consumed is the least-significant bit of the field.
#[derive(Clone, Debug)]
pub(crate) struct BitRun<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitRun<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    /// Number of bytes the run has touched so far.
    pub(crate) fn bytes_used(&self) -> usize {
        self.bit.div_ceil(8)
    }

    /// Read an `n`-bit field, `n <= 32`. Fails when the field reaches past
    /// the end of the backing bytes.
    #[inline(always)]
    pub(crate) fn bits(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        debug_assert!(n <= 32);
        let end = self.bit + n as usize;
        if end > self.data.len() * 8 {
            return Err(invalid("draco: bit run truncated"));
        }
        let byte = self.bit >> 3;
        let shift = (self.bit & 7) as u32;
        let window = if byte + 8 <= self.data.len() {
            u64::from_le_bytes(self.data[byte..byte + 8].try_into().unwrap())
        } else {
            let mut tmp = [0u8; 8];
            let tail = &self.data[byte..];
            tmp[..tail.len()].copy_from_slice(tail);
            u64::from_le_bytes(tmp)
        };
        self.bit = end;
        Ok(((window >> shift) & ((1u64 << n) - 1)) as u32)
    }
}

/// Inverse of the signed-to-unsigned fold (ch. 02 §4).
#[inline]
pub(crate) fn unfold(u: u32) -> i32 {
    let half = (u >> 1) as i32;
    if u & 1 == 0 { half } else { -half - 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_examples() {
        let mut r = Reader::new(&[0x02, 0xe0, 0x03, 0x90, 0x02]);
        assert_eq!(r.varint32().unwrap(), 2);
        assert_eq!(r.varint32().unwrap(), 480);
        assert_eq!(r.varint32().unwrap(), 272);
        assert!(r.varint32().is_err());
    }

    #[test]
    fn varint_bounds() {
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x0f]);
        assert_eq!(r.varint32().unwrap(), u32::MAX);
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x1f]);
        assert!(r.varint32().is_err());
        let mut r = Reader::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]);
        assert!(r.varint32().is_err());
        let bytes: Vec<u8> = [0xff; 9].iter().copied().chain([0x01]).collect();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.varint64().unwrap(), u64::MAX);
    }

    #[test]
    fn bit_run_example() {
        // ch. 02 §3 worked example.
        let mut b = BitRun::new(&[0xb4, 0x14]);
        let got: Vec<u32> = [1, 2, 2, 3, 3, 2]
            .iter()
            .map(|&n| b.bits(n).unwrap())
            .collect();
        assert_eq!(got, [0, 2, 2, 5, 4, 2]);
        assert_eq!(b.bytes_used(), 2);
        assert!(b.bits(4).is_err());
    }

    #[test]
    fn unfold_examples() {
        let got: Vec<i32> = (0..5).map(unfold).collect();
        assert_eq!(got, [0, -1, 1, -2, 2]);
        assert_eq!(unfold(u32::MAX), i32::MIN);
    }
}
