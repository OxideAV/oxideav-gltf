//! Decoder for the Draco compressed-mesh bitstream (version 2.2, triangle
//! meshes) — the payload format of the glTF extension
//! `KHR_draco_mesh_compression`.
//!
//! Built from the observer specification under `docs/3d/gltf/draco/spec/`
//! and verified against its fixture corpus. Module layout follows the
//! specification's chapters:
//!
//! | Module | Chapter |
//! |---|---|
//! | `container` | 01 header, metadata block |
//! | `reader` | 02 byte order, varint, bit runs, signed mapping |
//! | `entropy` | 03 rANS symbol decoder, symbol sequences, binary decoder |
//! | `sequential` | 04 mesh encoding method 0 |
//! | `attributes` | 05 groups, value blocks, dequantisation, octahedral normals |
//! | `edgebreaker` | 06 mesh encoding method 1, seam sets, point numbering |
//! | `sequencing` | 07 attribute entry order |
//! | `predict` | 05 §6.4–§6.5 and 08 predictors and prediction transforms |
//!
//! ```no_run
//! let bytes = std::fs::read("mesh.drc").unwrap();
//! let mesh = oxideav_gltf::draco::decode(&bytes).unwrap();
//! for face in &mesh.faces {
//!     println!("{face:?}");
//! }
//! let position = mesh.attribute_by_unique_id(0).unwrap();
//! println!("{:?}", position.point_f32(0));
//! ```
//!
//! What the specification leaves out is rejected with
//! [`Error::Unsupported`](crate::Error): point clouds, bitstream versions
//! before 2.2, prediction methods 2 and 3, and integer-coded attributes
//! with 64-bit or boolean data types.

mod attributes;
mod container;
mod edgebreaker;
mod entropy;
mod predict;
mod reader;
mod sequencing;
mod sequential;

use crate::error::{Result, invalid};

/// Mesh encoding method of a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodingMethod {
    /// Method 0: stored point indices, attribute values in point order.
    Sequential,
    /// Method 1: triangle adjacency stored as a symbol string.
    Edgebreaker,
}

/// Attribute semantic as stored in the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Semantic {
    Position,
    Normal,
    Color,
    TexCoord,
    Generic,
}

impl Semantic {
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Position,
            1 => Self::Normal,
            2 => Self::Color,
            3 => Self::TexCoord,
            4 => Self::Generic,
            _ => return None,
        })
    }

    /// The integer code stored in the bitstream.
    pub fn code(self) -> u8 {
        match self {
            Self::Position => 0,
            Self::Normal => 1,
            Self::Color => 2,
            Self::TexCoord => 3,
            Self::Generic => 4,
        }
    }
}

/// Component data type of an attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DataType {
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Float32,
    Float64,
    Bool,
}

impl DataType {
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Int8,
            2 => Self::Uint8,
            3 => Self::Int16,
            4 => Self::Uint16,
            5 => Self::Int32,
            6 => Self::Uint32,
            7 => Self::Int64,
            8 => Self::Uint64,
            9 => Self::Float32,
            10 => Self::Float64,
            11 => Self::Bool,
            _ => return None,
        })
    }

    /// The integer code stored in the bitstream.
    pub fn code(self) -> u8 {
        match self {
            Self::Int8 => 1,
            Self::Uint8 => 2,
            Self::Int16 => 3,
            Self::Uint16 => 4,
            Self::Int32 => 5,
            Self::Uint32 => 6,
            Self::Int64 => 7,
            Self::Uint64 => 8,
            Self::Float32 => 9,
            Self::Float64 => 10,
            Self::Bool => 11,
        }
    }

    /// Size of one component in bytes.
    pub fn size(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 | Self::Bool => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Int64 | Self::Uint64 | Self::Float64 => 8,
        }
    }
}

/// One decoded attribute: a table of values and a map from point index to
/// value index.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DracoAttribute {
    pub semantic: Semantic,
    pub data_type: DataType,
    pub num_components: u8,
    pub normalized: bool,
    /// Application-level identifier; glTF's extension `attributes` object
    /// refers to attributes by this id, not by stream position.
    pub unique_id: u32,
    /// Value table: `value_count()` entries of `num_components` little-endian
    /// components each.
    pub values: Vec<u8>,
    /// `point_to_value[p]` is the index of the value point `p` uses.
    pub point_to_value: Vec<u32>,
}

impl DracoAttribute {
    /// Size of one value (all components) in bytes.
    pub fn value_size(&self) -> usize {
        usize::from(self.num_components) * self.data_type.size()
    }

    /// Number of distinct values stored.
    pub fn value_count(&self) -> usize {
        self.values.len() / self.value_size()
    }

    /// Raw little-endian bytes of value `index`.
    pub fn value_bytes(&self, index: usize) -> &[u8] {
        let size = self.value_size();
        &self.values[index * size..(index + 1) * size]
    }

    /// Raw little-endian bytes of the value used by point `point`.
    pub fn point_bytes(&self, point: usize) -> &[u8] {
        self.value_bytes(self.point_to_value[point] as usize)
    }

    /// The value used by point `point` as `f32` components, when the
    /// attribute's data type is [`DataType::Float32`].
    pub fn point_f32(&self, point: usize) -> Option<Vec<f32>> {
        if self.data_type != DataType::Float32 {
            return None;
        }
        Some(
            self.point_bytes(point)
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_le_bytes(b))
                .collect(),
        )
    }
}

/// One key/value pair of a metadata record. The bitstream does not tag the
/// value type; text is stored as its bytes, integers as 4 little-endian
/// bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MetadataEntry {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

/// A metadata record: entries plus named child records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MetadataRecord {
    pub entries: Vec<MetadataEntry>,
    pub children: Vec<(Vec<u8>, MetadataRecord)>,
}

impl MetadataRecord {
    /// Value of the first entry named `name`.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.name == name.as_bytes())
            .map(|e| e.value.as_slice())
    }
}

/// The optional metadata block of a payload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metadata {
    /// Per-attribute records, keyed by the attribute's unique id.
    pub attributes: Vec<(u32, MetadataRecord)>,
    /// The geometry-level record.
    pub geometry: MetadataRecord,
}

/// A decoded triangle mesh.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DracoMesh {
    pub method: EncodingMethod,
    /// Number of points; face indices and every attribute's
    /// `point_to_value` are in `0..num_points`.
    pub num_points: u32,
    /// Three point indices per face, in bitstream order.
    pub faces: Vec<[u32; 3]>,
    /// Attributes in order of appearance in the bitstream.
    pub attributes: Vec<DracoAttribute>,
    pub metadata: Option<Metadata>,
    /// Number of payload bytes the decoder consumed; a buffer may carry
    /// trailing bytes after the payload.
    pub bytes_consumed: usize,
}

impl DracoMesh {
    /// The attribute carrying `unique_id`.
    pub fn attribute_by_unique_id(&self, unique_id: u32) -> Option<&DracoAttribute> {
        self.attributes.iter().find(|a| a.unique_id == unique_id)
    }
}

/// Resource limits of a decode call.
///
/// The bitstream's entropy coding can describe a symbol in zero bytes, so a
/// payload's declared counts are not bounded by its size; these limits are
/// what bounds the memory a hostile payload can make the decoder allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Largest accepted point count.
    pub max_points: u32,
    /// Largest accepted face count.
    pub max_faces: u32,
    /// Largest accepted number of entropy-coded integers per point, summed
    /// over all attributes (an attribute stores one integer per component
    /// and entry, two for an octahedral normal). Together with
    /// `max_points` this bounds the attribute section's working memory.
    pub max_values_per_point: u32,
}

impl Limits {
    /// The default limits: 2^24 points, 2^24 faces and 32 coded integers
    /// per point.
    pub fn new() -> Self {
        Self {
            max_points: 1 << 24,
            max_faces: 1 << 24,
            max_values_per_point: 32,
        }
    }

    pub fn with_max_points(mut self, max_points: u32) -> Self {
        self.max_points = max_points;
        self
    }

    pub fn with_max_faces(mut self, max_faces: u32) -> Self {
        self.max_faces = max_faces;
        self
    }

    pub fn with_max_values_per_point(mut self, max_values_per_point: u32) -> Self {
        self.max_values_per_point = max_values_per_point;
        self
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode a payload with the default [`Limits`]. Bytes after the payload
/// are ignored.
pub fn decode(data: &[u8]) -> Result<DracoMesh> {
    decode_with_limits(data, &Limits::new())
}

/// Decode a payload under explicit [`Limits`].
pub fn decode_with_limits(data: &[u8], limits: &Limits) -> Result<DracoMesh> {
    decode_inner(data, limits, &mut None)
}

/// Decode and also return the derived-value events of the decode, in the
/// vocabulary of `docs/3d/gltf/draco/draco-fixtures-and-traces.md` §4.3
/// (`eb_symbol`, `start_face_interior`, `seam set`, `conn face`, `seq`,
/// `residual`, `para`, `cmp`, `uv_pred`, `normal_pred`, `integer`). Test
/// support for replaying the staged traces.
#[doc(hidden)]
pub fn decode_traced(data: &[u8], limits: &Limits) -> (Result<DracoMesh>, Vec<String>) {
    let mut trace = Some(Vec::new());
    let mesh = decode_inner(data, limits, &mut trace);
    (mesh, trace.unwrap_or_default())
}

fn decode_inner(
    data: &[u8],
    limits: &Limits,
    trace: &mut Option<Vec<String>>,
) -> Result<DracoMesh> {
    let mut r = reader::Reader::new(data);
    let header = container::read_header(&mut r)?;
    let metadata = if header.has_metadata {
        Some(container::read_metadata(&mut r)?)
    } else {
        None
    };
    let mut scratch = entropy::EntropyScratch::default();
    let (num_points, faces, attributes) = match header.method {
        EncodingMethod::Sequential => {
            let conn = sequential::read_connectivity(&mut r, limits, &mut scratch)?;
            let topology = attributes::Topology::Sequential {
                num_points: conn.num_points,
            };
            let attrs =
                attributes::read_attributes(&mut r, &topology, limits, &mut scratch, trace)?;
            (conn.num_points, conn.faces, attrs)
        }
        EncodingMethod::Edgebreaker => {
            let conn = edgebreaker::read_connectivity(&mut r, limits, &mut scratch, trace)?;
            if conn.num_points > limits.max_points {
                return Err(invalid("draco: point count above the decoder limit"));
            }
            let topology = attributes::Topology::Edgebreaker(&conn);
            let attrs =
                attributes::read_attributes(&mut r, &topology, limits, &mut scratch, trace)?;
            let faces = conn.points().as_chunks::<3>().0.to_vec();
            debug_assert_eq!(conn.num_faces * 3, conn.points().len());
            (conn.num_points, faces, attrs)
        }
    };
    Ok(DracoMesh {
        method: header.method,
        num_points,
        faces,
        attributes,
        metadata,
        bytes_consumed: r.pos(),
    })
}
