//! Output formats for [`parse`](crate::parse) and the `rdb` CLI.

use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use indexmap::IndexMap;

pub use self::json::JSON;
pub use self::nil::Nil;
pub use self::plain::Plain;
pub use self::protocol::Protocol;

use super::types::RdbValue;

/// JSON output.
pub mod json;
/// No output.
pub mod nil;
/// Line-oriented text output.
pub mod plain;
/// RESP command output.
pub mod protocol;

/// Write `data` to `out`, panicking on errors.
#[deprecated(
    since = "0.4.1",
    note = "panics on write errors; use `Write::write_all` and handle the error"
)]
pub fn write_str<W: Write>(out: &mut W, data: &str) {
    #[allow(clippy::unwrap_used)]
    out.write_all(data.as_bytes()).unwrap();
}

/// Buffered output of the built-in formatters.
///
/// Formatter callbacks cannot return errors, so the first write error is kept
/// and reported by [`Formatter::format`] / [`Formatter::flush`]; later writes
/// are dropped.
pub(crate) struct Output {
    inner: BufWriter<Box<dyn Write>>,
    error: Option<io::Error>,
}

impl Output {
    pub(crate) fn new(writer: Box<dyn Write>) -> Self {
        Output {
            inner: BufWriter::new(writer),
            error: None,
        }
    }

    /// Write to the file at `path`, or stdout if it is `None` or cannot be
    /// created (behaviour of the formatters' `new` constructors).
    pub(crate) fn from_path(path: Option<PathBuf>) -> Self {
        let writer: Box<dyn Write> = match path.map(std::fs::File::create) {
            Some(Ok(file)) => Box::new(file),
            _ => Box::new(io::stdout()),
        };
        Output::new(writer)
    }

    pub(crate) fn write(&mut self, data: &[u8]) {
        if self.error.is_none() {
            if let Err(e) = self.inner.write_all(data) {
                self.error = Some(e);
            }
        }
    }

    pub(crate) fn write_str(&mut self, data: &str) {
        self.write(data.as_bytes());
    }

    /// Return the first write error since the last call, if any.
    pub(crate) fn take_error(&mut self) -> io::Result<()> {
        self.error.take().map_or(Ok(()), Err)
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        self.take_error()?;
        self.inner.flush()
    }
}

/// Call the [`Formatter`] method matching `value`.
fn dispatch<F: Formatter + ?Sized>(formatter: &mut F, value: &RdbValue) {
    match value {
        RdbValue::Set {
            key,
            members,
            expiry,
        } => formatter.set(key, members, expiry),
        RdbValue::Hash {
            key,
            values,
            expiry,
        } => formatter.hash(key, values, expiry),
        RdbValue::List {
            key,
            values,
            expiry,
        } => formatter.list(key, values, expiry),
        RdbValue::SortedSet {
            key,
            values,
            expiry,
        } => formatter.sorted_set(key, values, expiry),
        RdbValue::String { key, value, expiry } => formatter.string(key, value, expiry),
        RdbValue::SelectDb(db_number) => formatter.start_database(*db_number),
        RdbValue::ResizeDb {
            db_size,
            expires_size,
        } => formatter.resizedb(*db_size, *expires_size),
        RdbValue::AuxField { key, value } => formatter.aux_field(key, value),
        RdbValue::Checksum(checksum) => formatter.checksum(checksum),
    }
}

/// Renders RDB items. [`format`](Formatter::format) dispatches each
/// [`RdbValue`] to the matching method; all methods default to no-ops.
#[allow(unused_variables)]
pub trait Formatter {
    /// Before the first item.
    fn start_rdb(&mut self) {}
    /// After the last item (the checksum).
    fn end_rdb(&mut self) {}
    /// Trailing checksum; empty for RDB versions before 5.
    fn checksum(&mut self, checksum: &[u8]) {}

    /// Following keys belong to database `db_index`.
    fn start_database(&mut self, db_index: u32) {}
    /// Never called by [`parse`](crate::parse): RDB files mark only the start
    /// of a database.
    fn end_database(&mut self, db_index: u32) {}

    /// Size hints for the current database.
    fn resizedb(&mut self, db_size: u32, expires_size: u32) {}
    /// File metadata such as `redis-ver`.
    fn aux_field(&mut self, key: &[u8], value: &[u8]) {}

    /// A string key. `expiry` is absolute Unix milliseconds.
    fn string(&mut self, key: &[u8], value: &[u8], expiry: &Option<u64>) {}

    /// A hash key.
    fn hash(&mut self, key: &[u8], values: &IndexMap<Vec<u8>, Vec<u8>>, expiry: &Option<u64>) {}

    /// A set key.
    fn set(&mut self, key: &[u8], values: &[Vec<u8>], expiry: &Option<u64>) {}

    /// A list key.
    fn list(&mut self, key: &[u8], values: &[Vec<u8>], expiry: &Option<u64>) {}

    /// A sorted set key with `(score, member)` pairs.
    fn sorted_set(&mut self, key: &[u8], values: &[(f64, Vec<u8>)], expiry: &Option<u64>) {}

    /// Render one item. Returns the first output error, which stops parsing.
    fn format(&mut self, value: &RdbValue) -> io::Result<()> {
        dispatch(self, value);
        Ok(())
    }

    /// Called after [`end_rdb`](Formatter::end_rdb): write out buffered
    /// output and report errors that occurred since the last `format`.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// One of the built-in formatters, chosen at runtime (used by the CLI).
pub enum FormatterType {
    /// See [`JSON`].
    Json(JSON),
    /// See [`Plain`].
    Plain(Plain),
    /// See [`Nil`].
    Nil(Nil),
    /// See [`Protocol`].
    Protocol(Protocol),
}

impl FormatterType {
    fn inner(&mut self) -> &mut dyn Formatter {
        match self {
            Self::Json(f) => f,
            Self::Plain(f) => f,
            Self::Nil(f) => f,
            Self::Protocol(f) => f,
        }
    }
}

impl Formatter for FormatterType {
    fn format(&mut self, value: &RdbValue) -> io::Result<()> {
        self.inner().format(value)
    }

    fn start_rdb(&mut self) {
        self.inner().start_rdb()
    }

    fn end_rdb(&mut self) {
        self.inner().end_rdb()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner().flush()
    }
}
