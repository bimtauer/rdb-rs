use crate::formatter::Formatter;
use crate::types::RdbValue;
use std::path::PathBuf;

/// Produces no output; useful to validate a file.
pub struct Nil;

impl Nil {
    /// `file_path` is ignored: no file is created or truncated.
    pub fn new(_file_path: Option<PathBuf>) -> Nil {
        Nil
    }
}

impl Formatter for Nil {
    fn format(&mut self, _value: &RdbValue) -> std::io::Result<()> {
        Ok(())
    }
}
