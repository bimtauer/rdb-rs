use super::common::read_list_pack;
use super::common::utils::{read_blob, read_sequence};
use crate::types::{RdbError, RdbResult, RdbValue};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

pub fn read_set<R: Read>(input: &mut R, key: &[u8], expiry: Option<u64>) -> RdbResult<RdbValue> {
    let members = read_sequence(input, |input| read_blob(input))?;

    Ok(RdbValue::Set {
        key: key.to_vec(),
        members,
        expiry,
    })
}

pub fn read_set_intset<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let intset = read_blob(input)?;

    let mut reader = Cursor::new(intset);
    let byte_size = reader.read_u32::<LittleEndian>()?;
    let intset_length = reader.read_u32::<LittleEndian>()?;

    let mut members = Vec::with_capacity((intset_length as usize).min(4096));

    for _ in 0..intset_length {
        let val = match byte_size {
            2 => reader.read_i16::<LittleEndian>()? as i64,
            4 => reader.read_i32::<LittleEndian>()? as i64,
            8 => reader.read_i64::<LittleEndian>()?,
            _ => {
                return Err(RdbError::ParsingError {
                    context: "read_set_intset",
                    message: format!("Unhandled byte size in intset: {}", byte_size),
                });
            }
        };

        members.push(val.to_string().into_bytes());
    }

    Ok(RdbValue::Set {
        key: key.to_vec(),
        members,
        expiry,
    })
}

pub fn read_set_list_pack<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let members = read_list_pack(&read_blob(input)?)?;

    Ok(RdbValue::Set {
        key: key.to_vec(),
        members,
        expiry,
    })
}
