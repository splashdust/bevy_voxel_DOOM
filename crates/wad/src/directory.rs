//! The WAD header and lump directory.

use crate::bytes::{i32_at, name8};
use crate::WadError;

pub(crate) struct Entry {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

const HEADER_SIZE: usize = 12;
const ENTRY_SIZE: usize = 16;

pub(crate) fn parse(data: &[u8]) -> Result<Vec<Entry>, WadError> {
    let malformed = |m: String| WadError::Malformed(m);
    if data.len() < HEADER_SIZE {
        return Err(malformed(format!(
            "header needs {HEADER_SIZE} bytes, file has {}",
            data.len()
        )));
    }
    let ident = &data[0..4];
    if ident != b"IWAD" && ident != b"PWAD" {
        return Err(malformed(format!(
            "identification {:?} is neither IWAD nor PWAD",
            String::from_utf8_lossy(ident)
        )));
    }
    let count = i32_at(data, 4).unwrap_or(-1);
    let table = i32_at(data, 8).unwrap_or(-1);
    if count < 0 || table < 0 {
        return Err(malformed(format!(
            "negative lump count {count} or directory offset {table}"
        )));
    }
    let (count, table) = (count as usize, table as usize);
    let end = count
        .checked_mul(ENTRY_SIZE)
        .and_then(|n| n.checked_add(table))
        .filter(|&end| end <= data.len())
        .ok_or_else(|| {
            malformed(format!(
                "directory of {count} entries at {table} runs past the end of the file"
            ))
        })?;
    data[table..end]
        .as_chunks::<ENTRY_SIZE>()
        .0
        .iter()
        .map(|e| {
            let name = name8(&e[8..16]);
            let offset = i32_at(e, 0).unwrap_or(-1);
            let size = i32_at(e, 4).unwrap_or(-1);
            if offset < 0 || size < 0 {
                return Err(malformed(format!(
                    "lump {name} has negative offset {offset} or size {size}"
                )));
            }
            let (offset, size) = (offset as usize, size as usize);
            if size > 0 && offset.checked_add(size).is_none_or(|e| e > data.len()) {
                return Err(malformed(format!(
                    "lump {name} ({size} bytes at {offset}) runs past the end of the file"
                )));
            }
            Ok(Entry { name, offset, size })
        })
        .collect()
}
