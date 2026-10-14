//! Module-owned data is opaque to rdb-rs. Module values, module aux data and
//! key metadata all use the same self-describing opcode stream, which lets
//! them be skipped without the module (`rdbLoadCheckModuleValue`).

use super::common::utils::{read_length, skip_blob, skip_bytes};
use crate::constants::module_op_code;
use crate::types::{RdbError, RdbOk};
use std::io::Read;

/// Size of a key metadata class spec.
const KEY_META_CLASS_SPEC_SIZE: u64 = 4;

/// Skip `(opcode, payload)*` up to and including the EOF opcode.
fn skip_module_values<R: Read>(input: &mut R) -> RdbOk {
    loop {
        match read_length(input)? {
            module_op_code::EOF => return Ok(()),
            module_op_code::SINT | module_op_code::UINT => {
                read_length(input)?;
            }
            module_op_code::FLOAT => skip_bytes(input, 4)?,
            module_op_code::DOUBLE => skip_bytes(input, 8)?,
            module_op_code::STRING => skip_blob(input)?,
            opcode => {
                return Err(RdbError::ParsingError {
                    context: "skip_module_values",
                    message: format!("Unknown module opcode: {}", opcode),
                });
            }
        }
    }
}

/// Skip a `MODULE_2` object: module id followed by the opcode stream.
pub fn skip_module_2<R: Read>(input: &mut R) -> RdbOk {
    read_length(input)?;
    skip_module_values(input)
}

/// Skip a `MODULE_AUX` record: module id, `when` (as a UINT value), opcode stream.
pub fn skip_module_aux<R: Read>(input: &mut R) -> RdbOk {
    read_length(input)?;
    let when_opcode = read_length(input)?;
    if when_opcode != module_op_code::UINT {
        return Err(RdbError::ParsingError {
            context: "skip_module_aux",
            message: format!("Bad when_opcode: {}", when_opcode),
        });
    }
    read_length(input)?;
    skip_module_values(input)
}

/// Skip a `KEY_META` block (RDB 13): class count, then per class a 4 byte
/// class spec followed by the opcode stream.
pub fn skip_key_meta<R: Read>(input: &mut R) -> RdbOk {
    for _ in 0..read_length(input)? {
        skip_bytes(input, KEY_META_CLASS_SPEC_SIZE)?;
        skip_module_values(input)?;
    }
    Ok(())
}
