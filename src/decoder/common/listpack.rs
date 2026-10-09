use super::utils::{read_exact, skip_bytes};
use crate::types::{RdbError, RdbResult};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

const LP_EOF: u8 = 0xFF;

/// Decode every entry of a listpack blob as a string.
///
/// Layout: `<total-bytes:u32le> <num-elements:u16le> <entry>* <0xFF>`, where
/// each entry is `<encoding+data> <backlen>`. Integer entries are rendered in
/// decimal, which is how Redis returns them to clients.
pub fn read_list_pack(listpack: &[u8]) -> RdbResult<Vec<Vec<u8>>> {
    let mut reader = Cursor::new(listpack);
    let total_bytes = reader.read_u32::<LittleEndian>()?;
    let num_elements = reader.read_u16::<LittleEndian>()?;

    if total_bytes as usize != listpack.len() {
        return Err(RdbError::ParsingError {
            context: "read_list_pack",
            message: format!(
                "Header announces {} bytes, blob has {}",
                total_bytes,
                listpack.len()
            ),
        });
    }

    let mut entries = Vec::with_capacity(num_elements as usize);
    while let Some(entry) = read_list_pack_entry(&mut reader)? {
        entries.push(entry);
    }

    // 65535 means "count unknown, walk the listpack".
    if num_elements != u16::MAX && entries.len() != num_elements as usize {
        return Err(RdbError::ParsingError {
            context: "read_list_pack",
            message: format!(
                "Header announces {} entries, found {}",
                num_elements,
                entries.len()
            ),
        });
    }

    Ok(entries)
}

/// Read one entry, or `None` at the end-of-listpack marker.
fn read_list_pack_entry<R: Read>(reader: &mut R) -> RdbResult<Option<Vec<u8>>> {
    let header = reader.read_u8()?;

    let (value, encoded_len) = if header & 0x80 == 0 {
        // 0xxxxxxx: 7 bit unsigned integer
        (int_entry((header & 0x7F) as i64), 1)
    } else if header & 0xC0 == 0x80 {
        // 10xxxxxx: string with 6 bit length
        let len = (header & 0x3F) as u64;
        (read_exact(reader, len)?, 1 + len)
    } else if header & 0xE0 == 0xC0 {
        // 110xxxxx yyyyyyyy: 13 bit signed integer
        let raw = (((header & 0x1F) as i64) << 8) | reader.read_u8()? as i64;
        let value = if raw >= 1 << 12 { raw - (1 << 13) } else { raw };
        (int_entry(value), 2)
    } else if header & 0xF0 == 0xE0 {
        // 1110xxxx yyyyyyyy: string with 12 bit length
        let len = (((header & 0x0F) as u64) << 8) | reader.read_u8()? as u64;
        (read_exact(reader, len)?, 2 + len)
    } else {
        match header {
            0xF0 => {
                let len = reader.read_u32::<LittleEndian>()? as u64;
                (read_exact(reader, len)?, 5 + len)
            }
            0xF1 => (int_entry(reader.read_i16::<LittleEndian>()? as i64), 3),
            0xF2 => (int_entry(reader.read_i24::<LittleEndian>()? as i64), 4),
            0xF3 => (int_entry(reader.read_i32::<LittleEndian>()? as i64), 5),
            0xF4 => (int_entry(reader.read_i64::<LittleEndian>()?), 9),
            LP_EOF => return Ok(None),
            _ => {
                return Err(RdbError::ParsingError {
                    context: "read_list_pack_entry",
                    message: format!("Unknown encoding value: {:#04x}", header),
                });
            }
        }
    };

    skip_bytes(reader, backlen_size(encoded_len))?;
    Ok(Some(value))
}

fn int_entry(value: i64) -> Vec<u8> {
    value.to_string().into_bytes()
}

/// Number of bytes used by the back-length field of an entry whose encoding
/// and data take `len` bytes (`lpEncodeBacklen`).
fn backlen_size(len: u64) -> u64 {
    match len {
        0..=127 => 1,
        128..16383 => 2,
        16383..2097151 => 3,
        2097151..268435455 => 4,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a listpack from raw `<encoding+data>` chunks, appending backlens.
    fn listpack(entries: &[&[u8]]) -> Vec<u8> {
        let mut body = Vec::new();
        for entry in entries {
            body.extend_from_slice(entry);
            body.extend(std::iter::repeat_n(
                0u8,
                backlen_size(entry.len() as u64) as usize,
            ));
        }
        let total = (6 + body.len() + 1) as u32;
        let mut lp = total.to_le_bytes().to_vec();
        lp.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        lp.extend_from_slice(&body);
        lp.push(LP_EOF);
        lp
    }

    #[test]
    fn decodes_all_integer_widths_and_signs() {
        let lp = listpack(&[
            &[0x7F],                   // 127, 7 bit
            &[0xDF, 0xFF],             // -1, 13 bit
            &[0xD0, 0x00],             // -4096, 13 bit minimum
            &[0xCF, 0xFF],             // 4095, 13 bit maximum
            &[0xF1, 0x78, 0xEC],       // -5000, int16
            &[0xF2, 0xA0, 0x86, 0x01], // 100000, int24
            &[0xF2, 0x60, 0x79, 0xFE], // -100000, int24
            &[0xF3, 0xFF, 0xFF, 0xFF, 0x7F],
            &[0xF4, 0, 0, 0, 0, 0, 0, 0, 0x80],
        ]);
        let expected: Vec<Vec<u8>> = [
            "127",
            "-1",
            "-4096",
            "4095",
            "-5000",
            "100000",
            "-100000",
            "2147483647",
            "-9223372036854775808",
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
        assert_eq!(expected, read_list_pack(&lp).unwrap());
    }

    #[test]
    fn decodes_strings() {
        let long = vec![b'x'; 300];
        let mut twelve_bit = vec![0xE1, 0x2C];
        twelve_bit.extend_from_slice(&long);
        let lp = listpack(&[&[0x82, b'h', b'i'], &twelve_bit]);
        assert_eq!(vec![b"hi".to_vec(), long], read_list_pack(&lp).unwrap());
    }

    #[test]
    fn rejects_entry_count_mismatch() {
        let mut lp = listpack(&[&[0x01]]);
        lp[4] = 2;
        assert!(read_list_pack(&lp).is_err());
    }

    #[test]
    fn rejects_truncated_listpack() {
        let lp = listpack(&[&[0x82, b'h', b'i']]);
        assert!(read_list_pack(&lp[..lp.len() - 2]).is_err());
    }
}
