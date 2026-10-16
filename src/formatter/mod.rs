use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use indexmap::IndexMap;

pub use self::json::JSON;
pub use self::nil::Nil;
pub use self::plain::Plain;
pub use self::protocol::Protocol;

use super::types::RdbValue;

pub mod json;
pub mod nil;
pub mod plain;
pub mod protocol;

#[deprecated(
    since = "0.4.1",
    note = "panics on write errors; use `Write::write_all` and handle the error"
)]
pub fn write_str<W: Write>(out: &mut W, data: &str) {
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
    fn start_rdb(&mut self) {}
    fn end_rdb(&mut self) {}
    fn checksum(&mut self, checksum: &[u8]) {}

    fn start_database(&mut self, db_index: u32) {}
    fn end_database(&mut self, db_index: u32) {}

    fn resizedb(&mut self, db_size: u32, expires_size: u32) {}
    fn aux_field(&mut self, key: &[u8], value: &[u8]) {}

    fn string(&mut self, key: &[u8], value: &[u8], expiry: &Option<u64>) {}

    fn hash(&mut self, key: &[u8], values: &IndexMap<Vec<u8>, Vec<u8>>, expiry: &Option<u64>) {}

    fn set(&mut self, key: &[u8], values: &[Vec<u8>], expiry: &Option<u64>) {}

    fn list(&mut self, key: &[u8], values: &[Vec<u8>], expiry: &Option<u64>) {}

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

pub enum FormatterType {
    Json(JSON),
    Plain(Plain),
    Nil(Nil),
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
