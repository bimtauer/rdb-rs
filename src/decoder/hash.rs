use super::common::utils::{read_blob, read_exact, read_length};
use super::common::{read_list_pack, read_ziplist_entry_string, read_ziplist_metadata};
use crate::types::{RdbError, RdbResult, RdbValue};
use byteorder::{LittleEndian, ReadBytesExt};
use indexmap::IndexMap;
use std::io::{Cursor, Read};

pub fn read_hash<R: Read>(input: &mut R, key: &[u8], expiry: Option<u64>) -> RdbResult<RdbValue> {
    let hash_items = read_length(input)?;
    let mut values = IndexMap::new();

    for _ in 0..hash_items {
        let field = read_blob(input)?;
        let val = read_blob(input)?;
        values.insert(field, val);
    }

    Ok(RdbValue::Hash {
        key: key.to_vec(),
        values,
        expiry,
    })
}

/// Hash with per-field TTLs (Redis >= 7.4), hashtable encoding.
///
/// `[min-expire:ms]` (type 24 only), then `len` triplets of
/// `<ttl:length> <field> <value>`. Field TTLs are consumed but not reported.
pub fn read_hash_metadata<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
    has_min_expire: bool,
) -> RdbResult<RdbValue> {
    if has_min_expire {
        input.read_u64::<LittleEndian>()?;
    }

    let hash_items = read_length(input)?;
    let mut values = IndexMap::new();

    for _ in 0..hash_items {
        read_length(input)?;
        let field = read_blob(input)?;
        let val = read_blob(input)?;
        values.insert(field, val);
    }

    Ok(RdbValue::Hash {
        key: key.to_vec(),
        values,
        expiry,
    })
}

pub fn read_hash_ziplist<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let ziplist = read_blob(input)?;
    let mut reader = Cursor::new(ziplist);
    let (_zlbytes, _zltail, zllen) = read_ziplist_metadata(&mut reader)?;

    if !zllen.is_multiple_of(2) {
        return Err(RdbError::ParsingError {
            context: "read_hash_ziplist",
            message: format!("Odd number of ziplist entries: {}", zllen),
        });
    }

    let mut values = IndexMap::new();

    for _ in 0..zllen / 2 {
        let field = read_ziplist_entry_string(&mut reader)?;
        let value = read_ziplist_entry_string(&mut reader)?;
        values.insert(field, value);
    }

    let last_byte = reader.read_u8()?;
    if last_byte != 0xFF {
        return Err(RdbError::ParsingError {
            context: "read_hash_ziplist",
            message: format!("Unknown encoding value: {}", last_byte),
        });
    }

    Ok(RdbValue::Hash {
        key: key.to_vec(),
        values,
        expiry,
    })
}

pub fn read_hash_zipmap<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let zipmap = read_blob(input)?;
    let mut reader = Cursor::new(zipmap);

    let zmlen = reader.read_u8()?;

    let mut length: i32;
    if zmlen <= 254 {
        length = zmlen as i32;
    } else {
        length = -1;
    }

    let mut values = IndexMap::new();

    loop {
        let next_byte = reader.read_u8()?;

        if next_byte == 0xFF {
            break; // End of list.
        }

        let field = read_zipmap_entry(next_byte, &mut reader)?;

        let next_byte = reader.read_u8()?;
        let _free = reader.read_u8()?;
        let value = read_zipmap_entry(next_byte, &mut reader)?;

        values.insert(field, value);

        if length > 0 {
            length -= 1;
        }

        if length == 0 {
            let last_byte = reader.read_u8()?;

            if last_byte != 0xFF {
                return Err(RdbError::ParsingError {
                    context: "read_hash_zipmap",
                    message: format!("Unknown encoding value: {}", last_byte),
                });
            }
            break;
        }
    }

    Ok(RdbValue::Hash {
        key: key.to_vec(),
        values,
        expiry,
    })
}

fn read_zipmap_entry<T: Read>(next_byte: u8, zipmap: &mut T) -> RdbResult<Vec<u8>> {
    let elem_len = match next_byte {
        253 => zipmap.read_u32::<LittleEndian>()?,
        254 | 255 => {
            return Err(RdbError::ParsingError {
                context: "read_zipmap_entry",
                message: format!("Unknown encoding value: {}", next_byte),
            });
        }
        _ => next_byte as u32,
    };

    read_exact(zipmap, elem_len as u64)
}

pub fn read_hash_list_pack<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
) -> RdbResult<RdbValue> {
    let values = list_pack_fields(input, 2, "read_hash_list_pack")?;
    Ok(RdbValue::Hash {
        key: key.to_vec(),
        values,
        expiry,
    })
}

/// Hash with per-field TTLs (Redis >= 7.4), listpack encoding.
///
/// `[min-expire:ms]` (type 25 only), then a listpack of
/// `<field> <value> <ttl>` triplets. Field TTLs are consumed but not reported.
pub fn read_hash_list_pack_ex<R: Read>(
    input: &mut R,
    key: &[u8],
    expiry: Option<u64>,
    has_min_expire: bool,
) -> RdbResult<RdbValue> {
    if has_min_expire {
        input.read_u64::<LittleEndian>()?;
    }
    let values = list_pack_fields(input, 3, "read_hash_list_pack_ex")?;
    Ok(RdbValue::Hash {
        key: key.to_vec(),
        values,
        expiry,
    })
}

/// Read a listpack blob of `tuple_len`-sized tuples starting with field, value.
fn list_pack_fields<R: Read>(
    input: &mut R,
    tuple_len: usize,
    context: &'static str,
) -> RdbResult<IndexMap<Vec<u8>, Vec<u8>>> {
    let entries = read_list_pack(&read_blob(input)?)?;
    if !entries.len().is_multiple_of(tuple_len) {
        return Err(RdbError::ParsingError {
            context,
            message: format!(
                "{} listpack entries is not a multiple of {}",
                entries.len(),
                tuple_len
            ),
        });
    }

    let mut values = IndexMap::with_capacity(entries.len() / tuple_len);
    let mut entries = entries.into_iter();
    while let (Some(field), Some(value)) = (entries.next(), entries.next()) {
        values.insert(field, value);
        for _ in 2..tuple_len {
            entries.next();
        }
    }
    Ok(values)
}
