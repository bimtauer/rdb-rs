use crate::types::RdbError;
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::Read;

use crate::constants::{constant, encoding, version};

pub use crate::types::{RdbOk, RdbResult};

/// Upper bound for capacity pre-allocation driven by lengths read from the
/// file, so a corrupt length cannot trigger a huge allocation up front.
const MAX_PREALLOC: usize = 4096;

/// `Vec::with_capacity` for an untrusted element count.
pub fn vec_for<T>(len: u64) -> Vec<T> {
    Vec::with_capacity((len as usize).min(MAX_PREALLOC))
}

/// Read a length-encoded integer (`rdbLoadLen`).
///
/// Returns the value and whether it is a special string encoding
/// (`11xxxxxx`, see [`encoding`]) rather than a plain length.
pub fn read_length_with_encoding<R: Read>(input: &mut R) -> RdbResult<(u64, bool)> {
    let enc_type = input.read_u8()?;

    match (enc_type & 0xC0) >> 6 {
        constant::RDB_ENCVAL => Ok(((enc_type & 0x3F) as u64, true)),
        constant::RDB_6BITLEN => Ok(((enc_type & 0x3F) as u64, false)),
        constant::RDB_14BITLEN => {
            let next_byte = input.read_u8()?;
            Ok(((((enc_type & 0x3F) as u64) << 8) | next_byte as u64, false))
        }
        _ => match enc_type {
            constant::RDB_32BITLEN => Ok((input.read_u32::<BigEndian>()? as u64, false)),
            constant::RDB_64BITLEN => Ok((input.read_u64::<BigEndian>()?, false)),
            _ => Err(RdbError::ParsingError {
                context: "read_length",
                message: format!("Unknown length encoding: {:#04x}", enc_type),
            }),
        },
    }
}

pub fn read_length<R: Read>(input: &mut R) -> RdbResult<u64> {
    let (length, is_encoded) = read_length_with_encoding(input)?;
    if is_encoded {
        return Err(RdbError::ParsingError {
            context: "read_length",
            message: format!("Expected a length, found string encoding {}", length),
        });
    }
    Ok(length)
}

/// Read a length that must fit into 32 bits (database ids, sizes reported to formatters).
pub fn read_length_u32<R: Read>(input: &mut R, context: &'static str) -> RdbResult<u32> {
    let length = read_length(input)?;
    u32::try_from(length).map_err(|_| RdbError::ParsingError {
        context,
        message: format!("Value {} does not fit into 32 bits", length),
    })
}

pub fn verify_magic<R: Read>(input: &mut R) -> RdbOk {
    let mut magic = [0; 5];
    input
        .read_exact(&mut magic)
        .map_err(|_| RdbError::InvalidHeader("file too short"))?;

    if magic == constant::RDB_MAGIC.as_bytes() {
        Ok(())
    } else {
        Err(RdbError::InvalidHeader("missing REDIS magic string"))
    }
}

/// Read the 4-digit version following the magic string and check it is supported.
pub fn verify_version<R: Read>(input: &mut R) -> RdbResult<u32> {
    let mut buf = [0u8; 4];
    input
        .read_exact(&mut buf)
        .map_err(|_| RdbError::InvalidHeader("file too short"))?;

    let version = buf.iter().try_fold(0u32, |acc, &byte| {
        if byte.is_ascii_digit() {
            Ok(acc * 10 + (byte - b'0') as u32)
        } else {
            Err(RdbError::InvalidHeader("version is not a number"))
        }
    })?;

    if (version::SUPPORTED_MINIMUM..=version::SUPPORTED_MAXIMUM).contains(&version) {
        Ok(version)
    } else {
        Err(RdbError::UnsupportedVersion(version))
    }
}

/// Read a string object (`rdbGenericLoadStringObject`), decoding integer and
/// LZF encodings.
pub fn read_blob<R: Read>(input: &mut R) -> RdbResult<Vec<u8>> {
    let (length, is_encoded) = read_length_with_encoding(input)?;

    if !is_encoded {
        return read_exact(input, length);
    }

    match length {
        encoding::INT8 => Ok(input.read_i8()?.to_string().into_bytes()),
        encoding::INT16 => Ok(input.read_i16::<LittleEndian>()?.to_string().into_bytes()),
        encoding::INT32 => Ok(input.read_i32::<LittleEndian>()?.to_string().into_bytes()),
        encoding::LZF => {
            let compressed_length = read_length(input)?;
            let real_length = read_length(input)?;
            let data = read_exact(input, compressed_length)?;
            lzf::decompress(&data, real_length as usize).map_err(|e| RdbError::ParsingError {
                context: "read_blob",
                message: format!("LZF decompression failed: {:?}", e),
            })
        }
        _ => Err(RdbError::ParsingError {
            context: "read_blob",
            message: format!("Unknown string encoding: {}", length),
        }),
    }
}

/// Read exactly `len` bytes. Memory grows with the data actually read, so a
/// corrupt length fails with an IO error instead of a huge allocation.
pub fn read_exact<T: Read>(reader: &mut T, len: u64) -> RdbResult<Vec<u8>> {
    let mut buf = vec_for(len);
    reader.take(len).read_to_end(&mut buf)?;
    if (buf.len() as u64) < len {
        return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
    }
    Ok(buf)
}

/// Discard exactly `len` bytes.
pub fn skip_bytes<R: Read>(input: &mut R, len: u64) -> RdbOk {
    let skipped = std::io::copy(&mut input.take(len), &mut std::io::sink())?;
    if skipped < len {
        return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
    }
    Ok(())
}

/// Skip a string object without decoding it.
pub fn skip_blob<R: Read>(input: &mut R) -> RdbOk {
    let (length, is_encoded) = read_length_with_encoding(input)?;

    let skip = if !is_encoded {
        length
    } else {
        match length {
            encoding::INT8 => 1,
            encoding::INT16 => 2,
            encoding::INT32 => 4,
            encoding::LZF => {
                let compressed_length = read_length(input)?;
                read_length(input)?;
                compressed_length
            }
            _ => {
                return Err(RdbError::ParsingError {
                    context: "skip_blob",
                    message: format!("Unknown string encoding: {}", length),
                });
            }
        }
    };

    skip_bytes(input, skip)
}

pub fn read_sequence<R: Read, T, F>(input: &mut R, mut transform: F) -> RdbResult<Vec<T>>
where
    F: FnMut(&mut R) -> RdbResult<T>,
{
    let len = read_length(input)?;
    let mut values = vec_for(len);

    for _ in 0..len {
        values.push(transform(input)?);
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;
    use std::io::Cursor;

    #[rstest]
    #[case(&[0x0], (0, false), 1)]
    #[case(&[0x7f, 0xff], (16383, false), 2)]
    #[case(&[0x80, 0xff, 0xff, 0xff, 0xff], (4294967295, false), 5)]
    #[case(&[0x81, 0x00, 0x00, 0x01, 0x99, 0x5b, 0x8e, 0x4a, 0x10], (1_758_177_675_792, false), 9)]
    #[case(&[0xC0], (0, true), 1)]
    fn test_read_length(
        #[case] input: &[u8],
        #[case] expected: (u64, bool),
        #[case] expected_position: u64,
    ) {
        let mut cursor = Cursor::new(Vec::from(input));
        assert_eq!(expected, read_length_with_encoding(&mut cursor).unwrap());
        assert_eq!(expected_position, cursor.position());
    }

    #[test]
    fn test_read_length_rejects_unknown_prefix() {
        assert!(read_length_with_encoding(&mut Cursor::new(vec![0x82])).is_err());
    }

    #[test]
    fn test_read_blob() {
        assert_eq!(
            vec![0x61, 0x62, 0x63, 0x64],
            read_blob(&mut Cursor::new(vec![4, 0x61, 0x62, 0x63, 0x64])).unwrap()
        );
    }

    #[test]
    fn test_read_blob_truncated_huge_length_errors() {
        let input = vec![0x80, 0xff, 0xff, 0xff, 0xff, 0x61];
        assert!(read_blob(&mut Cursor::new(input)).is_err());
    }

    #[rstest]
    #[case(b"0003", Some(3))]
    #[case(b"0013", Some(13))]
    #[case(b"0014", None)]
    #[case(b"0000", None)]
    #[case(b"000:", None)]
    fn test_verify_version(#[case] input: &[u8], #[case] expected: Option<u32>) {
        assert_eq!(expected, verify_version(&mut Cursor::new(input)).ok());
    }

    #[test]
    fn test_verify_magic() {
        let success = verify_magic(&mut Cursor::new(vec![0x52, 0x45, 0x44, 0x49, 0x53]));
        assert!(success.is_ok(), "Expected success for valid magic bytes");

        let failure = verify_magic(&mut Cursor::new(vec![0x51, 0x0, 0x0, 0x0, 0x0]));
        assert!(failure.is_err(), "Expected error for invalid magic bytes");
    }
}
