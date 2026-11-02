//! Streams are not decoded; their structure is walked so the rest of the file
//! can still be parsed. Layout per `rdbLoadObject` in Redis 8.6 `rdb.c`.

use super::common::utils::{read_length, skip_blob, skip_bytes};
use crate::constants::encoding_type;
use crate::types::RdbOk;
use std::io::Read;

/// Size of a raw stream ID (`<ms:u64be><seq:u64be>`) and of a millisecond timestamp.
const STREAM_ID_SIZE: u64 = 16;
const MS_TIME_SIZE: u64 = 8;

pub fn skip_stream<R: Read>(input: &mut R, enc_type: u8) -> RdbOk {
    let v2 = enc_type >= encoding_type::STREAM_LIST_PACKS_2;
    let v3 = enc_type >= encoding_type::STREAM_LIST_PACKS_3;
    let v4 = enc_type >= encoding_type::STREAM_LIST_PACKS_4;

    // Radix tree nodes: master ID + listpack of entries.
    for _ in 0..read_length(input)? {
        skip_blob(input)?;
        skip_blob(input)?;
    }

    // length, last_id.ms, last_id.seq
    skip_lengths(input, 3)?;
    if v2 {
        // first_id (2), max_deleted_entry_id (2), entries_added
        skip_lengths(input, 5)?;
    }

    for _ in 0..read_length(input)? {
        skip_blob(input)?; // group name
        skip_lengths(input, 2)?; // last_id
        if v2 {
            skip_lengths(input, 1)?; // entries_read
        }

        // Group PEL: raw ID, delivery time, delivery count.
        for _ in 0..read_length(input)? {
            skip_bytes(input, STREAM_ID_SIZE + MS_TIME_SIZE)?;
            skip_lengths(input, 1)?;
        }

        for _ in 0..read_length(input)? {
            skip_blob(input)?; // consumer name
            skip_bytes(input, MS_TIME_SIZE)?; // seen time
            if v3 {
                skip_bytes(input, MS_TIME_SIZE)?; // active time
            }
            let consumer_pel = read_length(input)?;
            // Saturate: a corrupt count then fails as a short read.
            skip_bytes(input, consumer_pel.saturating_mul(STREAM_ID_SIZE))?;
        }
    }

    if v4 {
        // IDMP: duration, max entries, producers, iids_added, iids_duplicates
        skip_lengths(input, 2)?;
        for _ in 0..read_length(input)? {
            skip_blob(input)?; // producer id
            for _ in 0..read_length(input)? {
                skip_blob(input)?; // idempotency id
                skip_lengths(input, 2)?; // stream id
            }
        }
        skip_lengths(input, 2)?;
    }

    Ok(())
}

fn skip_lengths<R: Read>(input: &mut R, count: usize) -> RdbOk {
    for _ in 0..count {
        read_length(input)?;
    }
    Ok(())
}
