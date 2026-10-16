//! Parse Redis RDB dump files.
//!
//! An RDB file is the binary snapshot Redis writes with `SAVE`/`BGSAVE`. This
//! crate reads RDB versions 1 to 13 (Redis up to 8.6) and either hands you the
//! decoded items or renders them through a [`Formatter`].
//!
//! ```toml
//! [dependencies]
//! rdb = "0.4"
//! ```
//!
//! # Iterating over a dump
//!
//! [`RdbDecoder`] validates the header and then yields one [`RdbValue`] per
//! database switch, metadata item or key:
//!
//! ```
//! use std::fs::File;
//! use std::io::BufReader;
//! use rdb::{RdbDecoder, RdbValue, filter::Simple};
//!
//! # fn main() -> Result<(), rdb::RdbError> {
//! let file = BufReader::new(File::open("tests/dumps/redis_8_6_all_types.rdb")?);
//! let decoder = RdbDecoder::new(file, Simple::new())?;
//! assert_eq!(decoder.version(), 13);
//!
//! for item in decoder {
//!     match item? {
//!         RdbValue::String { key, value, expiry } => {
//!             println!("{:?} = {:?} (expires at {:?})", key, value, expiry)
//!         }
//!         RdbValue::Hash { key, values, .. } => println!("{:?}: {} fields", key, values.len()),
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Wrap files in a [`BufReader`](std::io::BufReader): the decoder issues many
//! small reads.
//!
//! # Formatting a dump
//!
//! [`parse`] drives a [`Formatter`] over all items. Four formatters are
//! included in [`formatter`]:
//!
//! * [`formatter::JSON`] writes one JSON object per database
//! * [`formatter::Plain`] writes one line per value, for grepping
//! * [`formatter::Protocol`] writes [RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/)
//!   commands that recreate the data (`SET`, `HSET`, ..., `PEXPIREAT`)
//! * [`formatter::Nil`] writes nothing, useful to validate a file
//!
//! Each takes an optional output path and writes to stdout otherwise.
//!
//! ```
//! use std::fs::File;
//! use std::io::BufReader;
//! use rdb::{filter::Simple, formatter::Nil};
//!
//! # fn main() -> Result<(), rdb::RdbError> {
//! let file = BufReader::new(File::open("tests/dumps/redis_8_6_all_types.rdb")?);
//! rdb::parse(file, Nil::new(None), Simple::new())?;
//! # Ok(())
//! # }
//! ```
//!
//! # Filtering
//!
//! [`filter::Simple`] restricts output to databases, value types and keys
//! matching a regular expression. Filtered keys are skipped without being
//! decoded. Implement [`Filter`] for custom rules.
//!
//! ```
//! use rdb::{Type, filter::Simple};
//! use regex::Regex;
//!
//! let mut filter = Simple::new();
//! filter.add_database(0);
//! filter.add_type(Type::Hash);
//! filter.add_keys(Regex::new("^user:").unwrap());
//! ```
//!
//! # What is not decoded
//!
//! * Streams and module values (e.g. RedisJSON) are skipped: the file parses,
//!   but these keys do not appear in the output.
//! * Per-field TTLs of hashes (Redis 7.4+) are read but not reported; the hash
//!   fields and values are.
//! * Lua function libraries, LRU/LFU information and cluster slot info are
//!   skipped.
//!
//! # Command line
//!
//! The crate also builds the `rdb` binary (`cargo install rdb`):
//!
//! ```text
//! $ rdb --format json dump.rdb
//! [{"key":"value"}]
//! $ rdb --format protocol --type hash --keys '^user:' dump.rdb
//! ```

#[cfg(feature = "python")]
use pyo3::exceptions::PyValueError;
#[cfg(feature = "python")]
use pyo3::prelude::*;

use std::io::Read;

pub use types::{RdbError, RdbOk, RdbResult, RdbValue, Type};

pub mod constants;
/// Iterating over the items of an RDB file.
pub mod decoder;
/// Selecting which keys are decoded.
pub mod filter;
pub mod formatter;
/// Decoded items, errors and data types.
pub mod types;

pub use decoder::RdbDecoder;
pub use filter::{Filter, Simple};
pub use formatter::{Formatter, FormatterType};

/// Runs a [`Formatter`] over the items of an RDB file. Usually created
/// through [`parse`]; use [`RdbParser::builder`] to assemble it step by step.
pub struct RdbParser<R: Read, L: Filter, F: Formatter> {
    decoder: RdbDecoder<R, L>,
    formatter: Option<F>,
}

impl<R: Read, L: Filter, F: Formatter> RdbParser<R, L, F> {
    /// Start assembling a parser.
    pub fn builder() -> RdbParserBuilder<R, L, F> {
        RdbParserBuilder {
            reader: None,
            filter: None,
            formatter: None,
        }
    }
}

/// Builder for [`RdbParser`]. A reader is required; the filter defaults to
/// accepting everything.
#[derive(Default)]
pub struct RdbParserBuilder<R: Read, L: Filter, F: Formatter> {
    reader: Option<R>,
    filter: Option<L>,
    formatter: Option<F>,
}

impl<R: Read, L: Filter + Default, F: Formatter> RdbParserBuilder<R, L, F> {
    /// Validate the RDB header and create the parser.
    ///
    /// Fails if no reader was set or the input is not a supported RDB file.
    pub fn build(self) -> RdbResult<RdbParser<R, L, F>> {
        let reader = self
            .reader
            .ok_or(RdbError::MissingValue("RdbParserBuilder::with_reader"))?;
        let filter = self.filter.unwrap_or_default();
        Ok(RdbParser {
            decoder: RdbDecoder::new(reader, filter)?,
            formatter: self.formatter,
        })
    }

    /// Read the RDB file from `reader` (required). Wrap files in a
    /// [`BufReader`](std::io::BufReader).
    pub fn with_reader(mut self, reader: R) -> Self {
        self.reader = Some(reader);
        self
    }

    /// Decode only keys accepted by `filter`.
    pub fn with_filter(mut self, filter: L) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Render items with `formatter`. Without one, [`RdbParser::parse`] does
    /// nothing.
    pub fn with_formatter(mut self, formatter: F) -> Self {
        self.formatter = Some(formatter);
        self
    }
}

impl<R: Read, L: Filter, F: Formatter> RdbParser<R, L, F> {
    /// Feed all items to the formatter. Stops at the first decoding or
    /// output error.
    pub fn parse(self) -> RdbResult<()> {
        if let Some(mut formatter) = self.formatter {
            formatter.start_rdb();
            for value in self.decoder {
                formatter.format(&value?)?;
            }
            formatter.end_rdb();
            formatter.flush()?;
        }
        Ok(())
    }
}

/// Read an RDB file from `reader` and render every item accepted by `filter`
/// through `formatter`.
pub fn parse<R: Read, L: Filter + Default, F: Formatter>(
    reader: R,
    formatter: F,
    filter: L,
) -> RdbResult<()> {
    let parser = RdbParser::builder()
        .with_reader(reader)
        .with_filter(filter)
        .with_formatter(formatter)
        .build()?;
    parser.parse()
}

/// Python iterator over the items of an RDB file (exposed as `rdb.RdbDecoder`).
#[cfg(feature = "python")]
#[pyclass(name = "RdbDecoder")]
pub struct PyRdbDecoder {
    decoder: RdbDecoder<std::io::BufReader<std::fs::File>, Simple>,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyRdbDecoder {
    /// Open `path` and validate the RDB header.
    #[new]
    pub fn new(path: std::path::PathBuf) -> PyResult<Self> {
        // io::Error maps onto the matching OSError subclass
        // (FileNotFoundError, PermissionError, IsADirectoryError, ...).
        let file = std::fs::File::open(path)?;
        let decoder = RdbDecoder::new(std::io::BufReader::new(file), Simple::new())
            .map_err(|e| PyValueError::new_err(format!("Not a valid RDB file: {}", e)))?;

        Ok(PyRdbDecoder { decoder })
    }

    /// RDB format version declared in the file header.
    #[getter]
    fn version(&self) -> u32 {
        self.decoder.version()
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__<'py>(
        mut slf: PyRefMut<'py, Self>,
    ) -> PyResult<Option<Bound<'py, pyo3::types::PyDict>>> {
        let py = slf.py();
        match slf.decoder.next() {
            Some(Ok(value)) => value.into_pyobject(py).map(Some),
            Some(Err(e)) => Err(PyValueError::new_err(format!("Parsing error: {}", e))),
            None => Ok(None),
        }
    }
}

#[cfg(feature = "python")]
#[pymodule(name = "rdb")]
fn rdb_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRdbDecoder>()?;
    Ok(())
}
