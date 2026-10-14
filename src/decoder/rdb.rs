use super::common::utils::{
    read_blob, read_length, read_length_u32, skip_blob, skip_bytes, verify_magic, verify_version,
};
use super::module::{skip_key_meta, skip_module_2, skip_module_aux};
use super::sorted_set::read_string_score;
use super::stream::skip_stream;
use super::{hash, list, set, sorted_set};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Read;

use crate::constants::{encoding_type, op_code};
use crate::filter::Filter;
use crate::types::{RdbError, RdbOk, RdbResult, RdbValue};

#[derive(Default)]
pub(crate) struct DecoderState {
    /// Expiry set by an `EXPIRETIME*` opcode for the next key only.
    pub pending_expiry: Option<u64>,
    pub current_database: u32,
    /// Set once the EOF marker was read or an error occurred.
    pub finished: bool,
}

pub(crate) fn verify_header<R: Read>(input: &mut R) -> RdbResult<u32> {
    verify_magic(input)?;
    verify_version(input)
}

/// Decode a value of type `value_type`. Returns `None` for types that are
/// walked but not decoded (streams, module values).
pub(crate) fn read_type<R: Read>(
    input: &mut R,
    key: &[u8],
    value_type: u8,
    expiry: Option<u64>,
) -> RdbResult<Option<RdbValue>> {
    let result = match value_type {
        encoding_type::STRING => {
            let value = read_blob(input)?;
            RdbValue::String {
                key: key.to_vec(),
                value,
                expiry,
            }
        }
        encoding_type::LIST => list::read_linked_list(input, key, expiry)?,
        encoding_type::SET => set::read_set(input, key, expiry)?,
        encoding_type::ZSET => sorted_set::read_sorted_set(input, key, expiry, false)?,
        encoding_type::HASH => hash::read_hash(input, key, expiry)?,
        encoding_type::ZSET_2 => sorted_set::read_sorted_set(input, key, expiry, true)?,
        encoding_type::HASH_ZIPMAP => hash::read_hash_zipmap(input, key, expiry)?,
        encoding_type::LIST_ZIPLIST => list::read_list_ziplist(input, key, expiry)?,
        encoding_type::SET_INTSET => set::read_set_intset(input, key, expiry)?,
        encoding_type::ZSET_ZIPLIST => sorted_set::read_sorted_set_ziplist(input, key, expiry)?,
        encoding_type::HASH_ZIPLIST => hash::read_hash_ziplist(input, key, expiry)?,
        encoding_type::LIST_QUICKLIST => list::read_quicklist(input, key, expiry)?,
        encoding_type::HASH_LIST_PACK => hash::read_hash_list_pack(input, key, expiry)?,
        encoding_type::ZSET_LIST_PACK => sorted_set::read_sorted_set_listpack(input, key, expiry)?,
        encoding_type::LIST_QUICKLIST_2 => list::read_quicklist_2(input, key, expiry)?,
        encoding_type::SET_LIST_PACK => set::read_set_list_pack(input, key, expiry)?,
        encoding_type::HASH_METADATA_PRE_GA => hash::read_hash_metadata(input, key, expiry, false)?,
        encoding_type::HASH_METADATA => hash::read_hash_metadata(input, key, expiry, true)?,
        encoding_type::HASH_LIST_PACK_EX_PRE_GA => {
            hash::read_hash_list_pack_ex(input, key, expiry, false)?
        }
        encoding_type::HASH_LIST_PACK_EX => hash::read_hash_list_pack_ex(input, key, expiry, true)?,
        encoding_type::STREAM_LIST_PACKS
        | encoding_type::STREAM_LIST_PACKS_2
        | encoding_type::STREAM_LIST_PACKS_3
        | encoding_type::STREAM_LIST_PACKS_4
        | encoding_type::MODULE
        | encoding_type::MODULE_2 => {
            skip_object(input, value_type)?;
            return Ok(None);
        }
        unknown => return Err(RdbError::UnknownEncoding(unknown)),
    };
    Ok(Some(result))
}

/// Skip a value of type `enc_type` without decoding it.
pub(crate) fn skip_object<R: Read>(input: &mut R, enc_type: u8) -> RdbOk {
    match enc_type {
        encoding_type::STRING
        | encoding_type::HASH_ZIPMAP
        | encoding_type::LIST_ZIPLIST
        | encoding_type::SET_INTSET
        | encoding_type::ZSET_ZIPLIST
        | encoding_type::HASH_ZIPLIST
        | encoding_type::HASH_LIST_PACK
        | encoding_type::ZSET_LIST_PACK
        | encoding_type::SET_LIST_PACK
        | encoding_type::HASH_LIST_PACK_EX_PRE_GA => skip_blob(input),
        encoding_type::HASH_LIST_PACK_EX => {
            skip_bytes(input, 8)?;
            skip_blob(input)
        }
        encoding_type::LIST | encoding_type::SET | encoding_type::LIST_QUICKLIST => {
            for _ in 0..read_length(input)? {
                skip_blob(input)?;
            }
            Ok(())
        }
        encoding_type::HASH => {
            for _ in 0..read_length(input)? {
                skip_blob(input)?;
                skip_blob(input)?;
            }
            Ok(())
        }
        encoding_type::ZSET => {
            for _ in 0..read_length(input)? {
                skip_blob(input)?;
                read_string_score(input)?;
            }
            Ok(())
        }
        encoding_type::ZSET_2 => {
            for _ in 0..read_length(input)? {
                skip_blob(input)?;
                skip_bytes(input, 8)?;
            }
            Ok(())
        }
        encoding_type::LIST_QUICKLIST_2 => {
            for _ in 0..read_length(input)? {
                read_length(input)?;
                skip_blob(input)?;
            }
            Ok(())
        }
        encoding_type::HASH_METADATA_PRE_GA | encoding_type::HASH_METADATA => {
            if enc_type == encoding_type::HASH_METADATA {
                skip_bytes(input, 8)?;
            }
            for _ in 0..read_length(input)? {
                read_length(input)?;
                skip_blob(input)?;
                skip_blob(input)?;
            }
            Ok(())
        }
        encoding_type::STREAM_LIST_PACKS
        | encoding_type::STREAM_LIST_PACKS_2
        | encoding_type::STREAM_LIST_PACKS_3
        | encoding_type::STREAM_LIST_PACKS_4 => skip_stream(input, enc_type),
        encoding_type::MODULE_2 => skip_module_2(input),
        encoding_type::MODULE => Err(RdbError::Unsupported(
            "pre-release module format (Redis 4.0 RC)",
        )),
        unknown => Err(RdbError::UnknownEncoding(unknown)),
    }
}

/// Read until the next item worth reporting: a database switch, metadata or
/// a key/value pair that passes `filter`.
pub(crate) fn process_next_operation<R: Read, F: Filter>(
    input: &mut R,
    filter: &F,
    state: &mut DecoderState,
) -> RdbResult<RdbValue> {
    loop {
        let value_type = match input.read_u8()? {
            op_code::SELECTDB => {
                state.current_database = read_length_u32(input, "SELECTDB")?;
                return Ok(RdbValue::SelectDb(state.current_database));
            }
            op_code::EOF => {
                let mut checksum = Vec::new();
                input.read_to_end(&mut checksum)?;
                state.finished = true;
                return Ok(RdbValue::Checksum(checksum));
            }
            op_code::RESIZEDB => {
                let db_size = read_length_u32(input, "RESIZEDB")?;
                let expires_size = read_length_u32(input, "RESIZEDB")?;
                return Ok(RdbValue::ResizeDb {
                    db_size,
                    expires_size,
                });
            }
            op_code::AUX => {
                let key = read_blob(input)?;
                let value = read_blob(input)?;
                return Ok(RdbValue::AuxField { key, value });
            }
            op_code::EXPIRETIME_MS => {
                state.pending_expiry = Some(input.read_u64::<LittleEndian>()?);
                continue;
            }
            op_code::EXPIRETIME => {
                state.pending_expiry = Some(input.read_u32::<LittleEndian>()? as u64 * 1000);
                continue;
            }
            op_code::IDLE => {
                read_length(input)?;
                continue;
            }
            op_code::FREQ => {
                input.read_u8()?;
                continue;
            }
            op_code::MODULE_AUX => {
                skip_module_aux(input)?;
                continue;
            }
            op_code::FUNCTION2 => {
                skip_blob(input)?;
                continue;
            }
            op_code::SLOT_INFO => {
                // slot id, slot size, expires slot size
                for _ in 0..3 {
                    read_length(input)?;
                }
                continue;
            }
            op_code::FUNCTION_PRE_GA => {
                return Err(RdbError::Unsupported(
                    "pre-release function format (Redis 7.0 RC)",
                ));
            }
            op_code::KEY_META => {
                // Key metadata sits between the expiry and the type byte.
                skip_key_meta(input)?;
                input.read_u8()?
            }
            value_type => value_type,
        };

        let expiry = state.pending_expiry.take();

        if !filter.matches_db(state.current_database) {
            skip_blob(input)?;
            skip_object(input, value_type)?;
            continue;
        }

        let key = read_blob(input)?;
        if !filter.matches_type(value_type) || !filter.matches_key(&key) {
            skip_object(input, value_type)?;
            continue;
        }

        if let Some(value) = read_type(input, &key, value_type, expiry)? {
            return Ok(value);
        }
    }
}
