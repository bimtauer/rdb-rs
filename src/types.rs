use thiserror::Error;

use indexmap::IndexMap;

use crate::constants::encoding_type;

#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::PyDict;

/// Errors returned while reading an RDB file.
#[derive(Error, Debug)]
pub enum RdbError {
    /// Reading from the underlying reader failed, including unexpected end of input.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No value found after {0}")]
    MissingValue(&'static str),
    /// A value type byte this crate does not know.
    #[error("Unknown encoding type: {0}")]
    UnknownEncoding(u8),
    /// The input does not start with a valid `REDIS<version>` header.
    #[error("Not an RDB file: {0}")]
    InvalidHeader(&'static str),
    /// The header declares an RDB version newer than this crate supports.
    #[error("Unsupported RDB version {0} (supported: 1-13)")]
    UnsupportedVersion(u32),
    /// A known but unsupported format, such as pre-release Redis formats.
    #[error("Unsupported format: {0}")]
    Unsupported(&'static str),
    /// The data is malformed.
    #[error("Parsing error in {context}: {message}")]
    ParsingError {
        context: &'static str,
        message: String,
    },
}
pub type RdbResult<T> = Result<T, RdbError>;

pub type RdbOk = RdbResult<()>;

/// Logical Redis data type, independent of its on-disk encoding.
#[derive(Debug, PartialEq)]
pub enum Type {
    String,
    List,
    Set,
    SortedSet,
    Hash,
    /// Walked but not decoded.
    Stream,
    /// Walked but not decoded.
    Module,
}

impl Type {
    /// Map an on-disk value type byte to its logical type.
    pub fn from_encoding(enc_type: u8) -> RdbResult<Type> {
        match enc_type {
            encoding_type::STRING => Ok(Type::String),
            encoding_type::HASH
            | encoding_type::HASH_ZIPMAP
            | encoding_type::HASH_ZIPLIST
            | encoding_type::HASH_LIST_PACK
            | encoding_type::HASH_METADATA_PRE_GA
            | encoding_type::HASH_LIST_PACK_EX_PRE_GA
            | encoding_type::HASH_METADATA
            | encoding_type::HASH_LIST_PACK_EX => Ok(Type::Hash),
            encoding_type::LIST
            | encoding_type::LIST_ZIPLIST
            | encoding_type::LIST_QUICKLIST
            | encoding_type::LIST_QUICKLIST_2 => Ok(Type::List),
            encoding_type::SET | encoding_type::SET_INTSET | encoding_type::SET_LIST_PACK => {
                Ok(Type::Set)
            }
            encoding_type::ZSET
            | encoding_type::ZSET_ZIPLIST
            | encoding_type::ZSET_2
            | encoding_type::ZSET_LIST_PACK => Ok(Type::SortedSet),
            encoding_type::STREAM_LIST_PACKS
            | encoding_type::STREAM_LIST_PACKS_2
            | encoding_type::STREAM_LIST_PACKS_3
            | encoding_type::STREAM_LIST_PACKS_4 => Ok(Type::Stream),
            encoding_type::MODULE | encoding_type::MODULE_2 => Ok(Type::Module),
            _ => Err(RdbError::UnknownEncoding(enc_type)),
        }
    }
}

/// One item of an RDB file.
///
/// Keys, values and members are raw bytes, as stored by Redis. `expiry` is an
/// absolute Unix timestamp in milliseconds.
#[derive(Debug)]
pub enum RdbValue {
    /// Following keys belong to this database.
    SelectDb(u32),
    /// Size hints for the current database.
    ResizeDb { db_size: u32, expires_size: u32 },
    /// Metadata such as `redis-ver` or `ctime`.
    AuxField { key: Vec<u8>, value: Vec<u8> },
    /// Trailing CRC64 checksum (8 bytes, little-endian; empty before RDB 5).
    /// Always the last item.
    Checksum(Vec<u8>),
    String {
        key: Vec<u8>,
        value: Vec<u8>,
        expiry: Option<u64>,
    },
    /// Fields in file order. Per-field TTLs (Redis 7.4+) are not included.
    Hash {
        key: Vec<u8>,
        values: IndexMap<Vec<u8>, Vec<u8>>,
        expiry: Option<u64>,
    },
    Set {
        key: Vec<u8>,
        members: Vec<Vec<u8>>,
        expiry: Option<u64>,
    },
    List {
        key: Vec<u8>,
        values: Vec<Vec<u8>>,
        expiry: Option<u64>,
    },
    SortedSet {
        key: Vec<u8>,
        /// `(score, member)` pairs.
        values: Vec<(f64, Vec<u8>)>,
        expiry: Option<u64>,
    },
}

#[cfg(feature = "python")]
impl<'py> IntoPyObject<'py> for RdbValue {
    type Target = PyDict;
    type Output = Bound<'py, PyDict>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        match self {
            RdbValue::Hash {
                key,
                values,
                expiry,
            } => {
                let dict = PyDict::new(py);
                let values_dict = PyDict::new(py);
                for (k, v) in values {
                    values_dict.set_item(k, v)?;
                }
                dict.set_item("type", "hash")?;
                dict.set_item("key", key)?;
                dict.set_item("values", values_dict)?;
                dict.set_item("expiry", expiry)?;
                Ok(dict)
            }
            RdbValue::List {
                key,
                values,
                expiry,
            } => {
                let dict = PyDict::new(py);
                dict.set_item("type", "list")?;
                dict.set_item("key", key)?;
                dict.set_item("values", values)?;
                dict.set_item("expiry", expiry)?;
                Ok(dict)
            }
            RdbValue::Set {
                key,
                members,
                expiry,
            } => {
                let dict = PyDict::new(py);
                dict.set_item("type", "set")?;
                dict.set_item("key", key)?;
                dict.set_item("members", members)?;
                dict.set_item("expiry", expiry)?;
                Ok(dict)
            }
            RdbValue::SortedSet {
                key,
                values,
                expiry,
            } => {
                let dict = PyDict::new(py);
                dict.set_item("type", "sorted_set")?;
                dict.set_item("key", key)?;
                dict.set_item("values", values)?;
                dict.set_item("expiry", expiry)?;
                Ok(dict)
            }
            RdbValue::String { key, value, expiry } => {
                let dict = PyDict::new(py);
                dict.set_item("type", "string")?;
                dict.set_item("key", key)?;
                dict.set_item("value", value)?;
                dict.set_item("expiry", expiry)?;
                Ok(dict)
            }
            RdbValue::SelectDb(db) => {
                let dict = PyDict::new(py);
                dict.set_item("type", "select_db")?;
                dict.set_item("db", db)?;
                Ok(dict)
            }
            RdbValue::ResizeDb {
                db_size,
                expires_size,
            } => {
                let dict = PyDict::new(py);
                dict.set_item("type", "resize_db")?;
                dict.set_item("db_size", db_size)?;
                dict.set_item("expires_size", expires_size)?;
                Ok(dict)
            }
            RdbValue::AuxField { key, value } => {
                let dict = PyDict::new(py);
                dict.set_item("type", "aux_field")?;
                dict.set_item("key", key)?;
                dict.set_item("value", value)?;
                Ok(dict)
            }
            RdbValue::Checksum(checksum) => {
                let dict = PyDict::new(py);
                dict.set_item("type", "checksum")?;
                dict.set_item("checksum", checksum)?;
                Ok(dict)
            }
        }
    }
}
