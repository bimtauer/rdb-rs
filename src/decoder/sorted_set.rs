use super::common::utils::{read_blob, read_exact, read_length, vec_for};
use super::common::{read_list_pack, read_ziplist_entry_string, read_ziplist_metadata};
use crate::types::{RdbError, RdbResult, RdbValue};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

pub fn read_sorted_set<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
    is_zset2: bool,
) -> RdbResult<RdbValue> {
    let set_items = read_length(input)?;
    let mut values = vec_for(set_items);

    for _ in 0..set_items {
        let val = read_blob(input)?;
        let score = if is_zset2 {
            input.read_f64::<LittleEndian>()?
        } else {
            read_string_score(input)?
        };
        values.push((score, val));
    }

    Ok(RdbValue::SortedSet {
        key: key.to_vec(),
        values,
        expiry,
    })
}

/// Score of the original zset encoding: a length byte (253/254/255 for
/// NaN/+inf/-inf) followed by the score as ASCII.
pub(crate) fn read_string_score<R: Read>(input: &mut R) -> RdbResult<f64> {
    match input.read_u8()? {
        253 => Ok(f64::NAN),
        254 => Ok(f64::INFINITY),
        255 => Ok(f64::NEG_INFINITY),
        len => parse_score(&read_exact(input, len as u64)?, "read_sorted_set"),
    }
}

fn parse_score(raw: &[u8], context: &'static str) -> RdbResult<f64> {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| RdbError::ParsingError {
            context,
            message: format!("Failed to parse score: {:?}", String::from_utf8_lossy(raw)),
        })
}

pub fn read_sorted_set_ziplist<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let ziplist = read_blob(input)?;
    let mut reader = Cursor::new(ziplist);
    let (_zlbytes, _zltail, zllen) = read_ziplist_metadata(&mut reader)?;

    if !zllen.is_multiple_of(2) {
        return Err(RdbError::ParsingError {
            context: "read_sorted_set_ziplist",
            message: format!("Odd number of ziplist entries: {}", zllen),
        });
    }
    let mut values = Vec::with_capacity(zllen as usize / 2);

    for _ in 0..zllen / 2 {
        let entry = read_ziplist_entry_string(&mut reader)?;
        let score = read_ziplist_entry_string(&mut reader)?;
        values.push((parse_score(&score, "read_sorted_set_ziplist")?, entry));
    }

    let last_byte = reader.read_u8()?;
    if last_byte != 0xFF {
        return Err(RdbError::ParsingError {
            context: "read_sorted_set_ziplist",
            message: format!("Unknown encoding value: {}", last_byte),
        });
    }

    Ok(RdbValue::SortedSet {
        key: key.to_vec(),
        values,
        expiry,
    })
}

pub fn read_sorted_set_listpack<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let entries = read_list_pack(&read_blob(input)?)?;
    if !entries.len().is_multiple_of(2) {
        return Err(RdbError::ParsingError {
            context: "read_sorted_set_listpack",
            message: format!("Odd number of listpack entries: {}", entries.len()),
        });
    }

    let values = entries
        .chunks_exact(2)
        .map(|pair| {
            let score = parse_score(&pair[1], "read_sorted_set_listpack")?;
            Ok((score, pair[0].clone()))
        })
        .collect::<RdbResult<Vec<_>>>()?;

    Ok(RdbValue::SortedSet {
        key: key.to_vec(),
        values,
        expiry,
    })
}
