use super::Output;
use crate::formatter::Formatter;
use crate::types::RdbValue;
use indexmap::IndexMap;
use std::io::{self, Write};
use std::path::PathBuf;

/// Writes RESP commands (`SELECT`, `SET`, `HSET`, `SADD`, `RPUSH`, `ZADD`,
/// `PEXPIREAT`) that recreate the data, e.g. for `redis-cli --pipe`.
pub struct Protocol {
    out: Output,
}

impl Protocol {
    /// Write to the file at `file_path`, or stdout if `None`.
    ///
    /// Falls back to stdout if the file cannot be created; use
    /// [`Protocol::from_writer`] to handle that error.
    pub fn new(file_path: Option<PathBuf>) -> Protocol {
        Protocol {
            out: Output::from_path(file_path),
        }
    }

    /// Write to `writer` (buffered internally).
    pub fn from_writer<W: Write + 'static>(writer: W) -> Protocol {
        Protocol {
            out: Output::new(Box::new(writer)),
        }
    }

    fn emit(&mut self, args: &[&[u8]]) {
        self.out.write_str(&format!("*{}\r\n", args.len()));
        for arg in args {
            self.out.write_str(&format!("${}\r\n", arg.len()));
            self.out.write(arg);
            self.out.write_str("\r\n");
        }
    }

    fn expire(&mut self, key: &[u8], expiry: &Option<u64>) {
        if let Some(expire) = expiry {
            let expire = expire.to_string();
            self.emit(&[b"PEXPIREAT", key, expire.as_bytes()]);
        }
    }
}

impl Formatter for Protocol {
    fn string(&mut self, key: &[u8], value: &[u8], expiry: &Option<u64>) {
        self.emit(&[b"SET", key, value]);
        self.expire(key, expiry);
    }

    fn hash(&mut self, key: &[u8], values: &IndexMap<Vec<u8>, Vec<u8>>, expiry: &Option<u64>) {
        for (field, value) in values {
            self.emit(&[b"HSET", key, field, value]);
        }
        self.expire(key, expiry);
    }

    fn set(&mut self, key: &[u8], values: &[Vec<u8>], expiry: &Option<u64>) {
        for member in values {
            self.emit(&[b"SADD", key, member]);
        }
        self.expire(key, expiry);
    }

    fn list(&mut self, key: &[u8], values: &[Vec<u8>], expiry: &Option<u64>) {
        for value in values {
            self.emit(&[b"RPUSH", key, value]);
        }
        self.expire(key, expiry);
    }

    fn sorted_set(&mut self, key: &[u8], values: &[(f64, Vec<u8>)], expiry: &Option<u64>) {
        for (score, member) in values {
            let score = score.to_string();
            self.emit(&[b"ZADD", key, score.as_bytes(), member]);
        }
        self.expire(key, expiry);
    }

    fn start_database(&mut self, db_number: u32) {
        let db = db_number.to_string();
        self.emit(&[b"SELECT", db.as_bytes()]);
    }

    fn format(&mut self, value: &RdbValue) -> io::Result<()> {
        super::dispatch(self, value);
        self.out.take_error()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
