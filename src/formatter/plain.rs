use super::Output;
use crate::formatter::Formatter;
use crate::types::RdbValue;
use indexmap::IndexMap;
use std::io::{self, Write};
use std::path::PathBuf;

/// Writes one line per value: `db=<n> <key> -> <value>` and variants per type.
pub struct Plain {
    out: Output,
    dbnum: u32,
}

impl Plain {
    /// Write to the file at `file_path`, or stdout if `None`.
    ///
    /// Falls back to stdout if the file cannot be created; use
    /// [`Plain::from_writer`] to handle that error.
    pub fn new(file_path: Option<PathBuf>) -> Plain {
        Plain {
            out: Output::from_path(file_path),
            dbnum: 0,
        }
    }

    /// Write to `writer` (buffered internally).
    pub fn from_writer<W: Write + 'static>(writer: W) -> Plain {
        Plain {
            out: Output::new(Box::new(writer)),
            dbnum: 0,
        }
    }

    fn write_line_start(&mut self, key: &[u8]) {
        self.out.write_str(&format!("db={} ", self.dbnum));
        self.out.write(key);
    }
}

impl Formatter for Plain {
    fn string(&mut self, key: &[u8], value: &[u8], _expiry: &Option<u64>) {
        self.write_line_start(key);
        self.out.write_str(" -> ");
        self.out.write(value);
        self.out.write_str("\n");
    }

    fn hash(&mut self, key: &[u8], values: &IndexMap<Vec<u8>, Vec<u8>>, _expiry: &Option<u64>) {
        for (field, value) in values {
            self.write_line_start(key);
            self.out.write_str(" . ");
            self.out.write(field);
            self.out.write_str(" -> ");
            self.out.write(value);
            self.out.write_str("\n");
        }
    }

    fn set(&mut self, key: &[u8], values: &[Vec<u8>], _expiry: &Option<u64>) {
        for member in values {
            self.write_line_start(key);
            self.out.write_str(" { ");
            self.out.write(member);
            self.out.write_str(" } \n");
        }
    }

    fn list(&mut self, key: &[u8], values: &[Vec<u8>], _expiry: &Option<u64>) {
        for (index, value) in values.iter().enumerate() {
            self.write_line_start(key);
            self.out.write_str(&format!("[{}] -> ", index));
            self.out.write(value);
            self.out.write_str("\n");
        }
    }

    fn sorted_set(&mut self, key: &[u8], values: &[(f64, Vec<u8>)], _expiry: &Option<u64>) {
        for (index, (score, member)) in values.iter().enumerate() {
            self.write_line_start(key);
            self.out.write_str(&format!("[{}] -> {{", index));
            self.out.write(member);
            self.out.write_str(&format!(", score={}}}\n", score));
        }
    }

    fn checksum(&mut self, checksum: &[u8]) {
        if !checksum.is_empty() {
            let hex: String = checksum.iter().map(|b| format!("{b:02x}")).collect();
            self.out.write_str(&format!("checksum {}\n", hex));
        }
    }

    fn start_database(&mut self, db_number: u32) {
        self.dbnum = db_number;
    }

    fn aux_field(&mut self, key: &[u8], value: &[u8]) {
        self.out.write_str("aux ");
        self.out.write(key);
        self.out.write_str(" -> ");
        self.out.write(value);
        self.out.write_str("\n");
    }

    fn format(&mut self, value: &RdbValue) -> io::Result<()> {
        super::dispatch(self, value);
        self.out.take_error()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
