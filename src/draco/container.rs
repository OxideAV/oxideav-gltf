//! Payload header and metadata block
//! (`docs/3d/gltf/draco/spec/01-container.md`).

use super::reader::Reader;
use super::{EncodingMethod, Metadata, MetadataEntry, MetadataRecord};
use crate::error::{Result, invalid, unsupported};

/// Header flag: a metadata block follows the header.
const FLAG_METADATA: u16 = 0x8000;

/// Deepest nesting of metadata child records accepted. The corpus has no
/// nested record at all; the cap only bounds recursion on hostile input.
const MAX_METADATA_DEPTH: u32 = 32;

/// Parsed 11-byte header.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Header {
    pub(crate) method: EncodingMethod,
    pub(crate) has_metadata: bool,
}

pub(crate) fn read_header(r: &mut Reader<'_>) -> Result<Header> {
    let magic = r
        .take(5)
        .map_err(|_| invalid("draco: payload shorter than its header"))?;
    if magic != b"DRACO" {
        return Err(invalid("draco: bad magic"));
    }
    let major = r.u8()?;
    let minor = r.u8()?;
    let geometry_type = r.u8()?;
    let encoding_method = r.u8()?;
    let flags = r.u16()?;
    if (major, minor) != (2, 2) {
        return Err(unsupported(format!(
            "draco: bitstream version {major}.{minor} (only 2.2 is specified)"
        )));
    }
    match geometry_type {
        1 => {}
        0 => return Err(unsupported("draco: point-cloud payloads are not supported")),
        _ => return Err(invalid("draco: unknown geometry type")),
    }
    let method = match encoding_method {
        0 => EncodingMethod::Sequential,
        1 => EncodingMethod::Edgebreaker,
        _ => return Err(invalid("draco: unknown mesh encoding method")),
    };
    Ok(Header {
        method,
        has_metadata: flags & FLAG_METADATA != 0,
    })
}

fn read_name(r: &mut Reader<'_>) -> Result<Vec<u8>> {
    let len = usize::from(r.u8()?);
    Ok(r.take(len)?.to_vec())
}

fn read_record(r: &mut Reader<'_>, depth: u32) -> Result<MetadataRecord> {
    if depth > MAX_METADATA_DEPTH {
        return Err(invalid("draco: metadata nested too deeply"));
    }
    let mut record = MetadataRecord::default();
    let num_entries = r.count()?;
    // An entry is at least a name length, a value size and one value byte.
    if num_entries > r.remaining() / 3 {
        return Err(invalid("draco: metadata entry count exceeds the payload"));
    }
    for _ in 0..num_entries {
        let name = read_name(r)?;
        let size = r.count()?;
        if size == 0 {
            return Err(invalid("draco: empty metadata value"));
        }
        let value = r.take(size)?.to_vec();
        record.entries.push(MetadataEntry { name, value });
    }
    let num_children = r.count()?;
    // A child is at least a name length and two counts.
    if num_children > r.remaining() / 3 {
        return Err(invalid("draco: metadata child count exceeds the payload"));
    }
    for _ in 0..num_children {
        let name = read_name(r)?;
        let child = read_record(r, depth + 1)?;
        record.children.push((name, child));
    }
    Ok(record)
}

/// Metadata block (§2): per-attribute records, then the geometry record.
pub(crate) fn read_metadata(r: &mut Reader<'_>) -> Result<Metadata> {
    let num_attribute_records = r.count()?;
    if num_attribute_records > r.remaining() / 3 {
        return Err(invalid("draco: metadata record count exceeds the payload"));
    }
    let mut attributes = Vec::new();
    for _ in 0..num_attribute_records {
        let unique_id = r.varint32()?;
        attributes.push((unique_id, read_record(r, 0)?));
    }
    let geometry = read_record(r, 0)?;
    Ok(Metadata {
        attributes,
        geometry,
    })
}
