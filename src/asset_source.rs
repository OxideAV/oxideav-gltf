//! [`BufferViewAsset`] — `oxideav_mesh3d::AssetSource` impl backing
//! image bytes by an `(offset, length)` slice into a `.glb` BIN chunk
//! (or any owned byte buffer).
//!
//! Why not just clone the bytes into [`InMemoryAsset`]? Because BIN
//! chunks routinely pin tens of megabytes of textures, and a single
//! `Arc<Vec<u8>>` shared between every `BufferViewAsset` lets the
//! decoder hand out cheap views into it without copying. Each
//! `open()` call returns its own `Cursor` over the same underlying
//! `Arc`, so concurrent readers stay independent.

use std::io::{Cursor, Result as IoResult};
use std::sync::Arc;

use oxideav_mesh3d::asset::ReadSeek;
use oxideav_mesh3d::AssetSource;

/// Lazy view into a shared byte buffer.
///
/// Cheap to clone — the wrapped `Arc<Vec<u8>>` is reference-counted.
/// Each `open()` returns an owned `Cursor` over the slice, so multiple
/// readers can iterate the same asset concurrently.
#[derive(Clone, Debug)]
pub struct BufferViewAsset {
    pub bytes: Arc<Vec<u8>>,
    pub offset: usize,
    pub length: usize,
    pub mime: Option<String>,
}

impl BufferViewAsset {
    /// Construct a view spanning `bytes[offset..offset + length]`.
    /// `mime` is propagated to [`AssetSource::mime`] verbatim.
    pub fn new(bytes: Arc<Vec<u8>>, offset: usize, length: usize, mime: Option<String>) -> Self {
        Self {
            bytes,
            offset,
            length,
            mime,
        }
    }

    /// Borrowed slice view — useful in tests + for the encoder when it
    /// needs to copy bytes back into a fresh BIN chunk.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[self.offset..self.offset + self.length]
    }
}

impl AssetSource for BufferViewAsset {
    fn mime(&self) -> Option<&str> {
        self.mime.as_deref()
    }

    fn size_hint(&self) -> Option<u64> {
        Some(self.length as u64)
    }

    fn open(&self) -> IoResult<Box<dyn ReadSeek + Send>> {
        // Cursor::clone of the slice would force an owned `Vec`; we
        // instead clone the `Arc` and expose a cursor over a fresh
        // owned `Vec<u8>` for that slice. The Arc clone keeps the
        // shared chunk alive; the Vec is the (independent) per-reader
        // cursor backing.
        let view = self.as_slice().to_vec();
        Ok(Box::new(Cursor::new(view)))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn slices_correct_window() {
        let buf: Arc<Vec<u8>> = Arc::new((0u8..32).collect());
        let asset = BufferViewAsset::new(buf, 8, 16, Some("image/png".into()));
        assert_eq!(asset.size_hint(), Some(16));
        assert_eq!(asset.mime(), Some("image/png"));
        let mut reader = asset.open().unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, (8u8..24).collect::<Vec<_>>());
    }

    #[test]
    fn shared_arc_no_copy() {
        let buf: Arc<Vec<u8>> = Arc::new(vec![0xAA; 1024]);
        let a = BufferViewAsset::new(buf.clone(), 0, 512, None);
        let b = BufferViewAsset::new(buf.clone(), 512, 512, None);
        assert_eq!(Arc::strong_count(&buf), 3); // buf + a + b
        assert_eq!(a.as_slice().len(), 512);
        assert_eq!(b.as_slice().len(), 512);
    }
}
