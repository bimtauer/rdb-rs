//! Decoding arbitrary input must return values or errors, never panic or
//! exhaust memory. Every item is also rendered, to cover the formatters.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rdb::filter::Simple;
use rdb::formatter::{Formatter, JSON, Plain, Protocol};

fuzz_target!(|data: &[u8]| {
    let Ok(decoder) = rdb::RdbDecoder::new(data, Simple::new()) else {
        return;
    };
    let mut formatters = [
        Box::new(JSON::from_writer(std::io::sink())) as Box<dyn Formatter>,
        Box::new(Plain::from_writer(std::io::sink())),
        Box::new(Protocol::from_writer(std::io::sink())),
    ];
    for item in decoder {
        let Ok(item) = item else { return };
        for formatter in &mut formatters {
            let _ = formatter.format(&item);
        }
    }
});
