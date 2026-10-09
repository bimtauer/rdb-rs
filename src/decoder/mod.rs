mod common;
mod hash;
mod list;
mod module;
mod rdb;
mod set;
mod sorted_set;
mod stream;

use std::io::Read;

use self::rdb::DecoderState;
use crate::filter::Filter;
use crate::types::{RdbResult, RdbValue};

/// Iterator over the items of an RDB file.
///
/// Created via [`RdbDecoder::new`], which validates the file header. Each call
/// to `next` yields the next database switch, metadata item or key that
/// passes the filter. Iteration ends after the trailing checksum or after the
/// first error.
pub struct RdbDecoder<R: Read, F: Filter> {
    reader: R,
    filter: F,
    version: u32,
    state: DecoderState,
}

impl<R: Read, F: Filter> RdbDecoder<R, F> {
    /// Read and validate the RDB header (magic string and version).
    pub fn new(mut reader: R, filter: F) -> RdbResult<Self> {
        let version = rdb::verify_header(&mut reader)?;
        Ok(Self {
            reader,
            filter,
            version,
            state: DecoderState::default(),
        })
    }

    /// RDB format version declared in the file header.
    pub fn version(&self) -> u32 {
        self.version
    }
}

impl<R: Read, F: Filter> Iterator for RdbDecoder<R, F> {
    type Item = RdbResult<RdbValue>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.state.finished {
            return None;
        }
        let result = rdb::process_next_operation(&mut self.reader, &self.filter, &mut self.state);
        if result.is_err() {
            self.state.finished = true;
        }
        Some(result)
    }
}
