use super::Output;
use crate::formatter::Formatter;
use crate::types::RdbValue;
use indexmap::IndexMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::str;

/// Writes a JSON array with one object per database that has matching keys.
pub struct JSON {
    out: Output,
    /// An object for the current database has been opened and not closed.
    db_open: bool,
    /// At least one database object has been written.
    wrote_db: bool,
    is_first_key_in_db: bool,
    element_index: u32,
}

impl JSON {
    /// Write to the file at `file_path`, or stdout if `None`.
    ///
    /// Falls back to stdout if the file cannot be created; use
    /// [`JSON::from_writer`] to handle that error.
    pub fn new(file_path: Option<PathBuf>) -> JSON {
        JSON::with_output(Output::from_path(file_path))
    }

    /// Write to `writer` (buffered internally).
    pub fn from_writer<W: Write + 'static>(writer: W) -> JSON {
        JSON::with_output(Output::new(Box::new(writer)))
    }

    fn with_output(out: Output) -> JSON {
        JSON {
            out,
            db_open: false,
            wrote_db: false,
            is_first_key_in_db: true,
            element_index: 0,
        }
    }

    /// Start a key, opening the current database's object on its first key so
    /// databases without matching keys produce no output.
    fn start_key(&mut self) {
        if !self.db_open {
            if self.wrote_db {
                self.out.write_str(",");
            }
            self.out.write_str("{");
            self.db_open = true;
            self.wrote_db = true;
        }
        if !self.is_first_key_in_db {
            self.out.write_str(",");
        }

        self.is_first_key_in_db = false;
        self.element_index = 0;
    }

    fn write_comma(&mut self) {
        if self.element_index > 0 {
            self.out.write_str(",");
        }
        self.element_index += 1;
    }

    fn write_key(&mut self, key: &[u8]) {
        self.out.write_str(&encode_to_ascii(key));
    }
    fn write_value(&mut self, value: &[u8]) {
        self.out.write_str(&encode_to_ascii(value));
    }
}

/// Encode `value` as a JSON string literal.
///
/// Valid UTF-8 is emitted as-is apart from the mandatory escapes. Bytes of
/// invalid UTF-8 input that are not printable ASCII are emitted as `\u00XX`.
fn encode_to_ascii(value: &[u8]) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    match str::from_utf8(value) {
        Ok(s) => {
            for c in s.chars() {
                push_escaped(&mut out, c);
            }
        }
        Err(_) => {
            for &b in value {
                if (32..127).contains(&b) {
                    push_escaped(&mut out, b as char);
                } else {
                    out.push_str(&format!("\\u{:04x}", b));
                }
            }
        }
    }
    out.push('"');
    out
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{08}' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\u{0c}' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        '\u{00}'..='\u{1f}' | '\u{7f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push(c),
    }
}

impl Formatter for JSON {
    fn start_rdb(&mut self) {
        self.out.write_str("[");
    }

    fn end_rdb(&mut self) {
        if self.db_open {
            self.out.write_str("}");
        }
        self.out.write_str("]\n");
    }

    fn start_database(&mut self, _db_number: u32) {
        if self.db_open {
            self.out.write_str("}");
            self.db_open = false;
        }
        self.is_first_key_in_db = true;
    }

    fn string(&mut self, key: &[u8], value: &[u8], _expiry: &Option<u64>) {
        self.start_key();
        self.write_key(key);
        self.out.write_str(":");
        self.write_value(value);
    }

    fn hash(&mut self, key: &[u8], values: &IndexMap<Vec<u8>, Vec<u8>>, _expiry: &Option<u64>) {
        self.start_key();
        self.write_key(key);
        self.out.write_str(":{");
        for (field, value) in values {
            self.write_comma();
            self.write_key(field);
            self.out.write_str(":");
            self.write_value(value);
        }
        self.out.write_str("}");
    }

    fn set(&mut self, key: &[u8], values: &[Vec<u8>], _expiry: &Option<u64>) {
        self.start_key();
        self.write_key(key);
        self.out.write_str(":[");
        for value in values {
            self.write_comma();
            self.write_value(value);
        }
        self.out.write_str("]");
    }

    fn list(&mut self, key: &[u8], values: &[Vec<u8>], _expiry: &Option<u64>) {
        self.set(key, values, _expiry);
    }

    fn sorted_set(&mut self, key: &[u8], values: &[(f64, Vec<u8>)], _expiry: &Option<u64>) {
        self.start_key();
        self.write_key(key);
        self.out.write_str(":{");
        for (score, member) in values {
            self.write_comma();
            self.write_key(member);
            self.out.write_str(":");
            self.write_value(score.to_string().as_bytes());
        }
        self.out.write_str("}");
    }

    fn format(&mut self, value: &RdbValue) -> io::Result<()> {
        super::dispatch(self, value);
        self.out.take_error()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
