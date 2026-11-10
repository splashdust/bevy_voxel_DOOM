//! Little-endian reads that return `None` past the end of the slice.

pub(crate) fn u8_at(d: &[u8], off: usize) -> Option<u8> {
    d.get(off).copied()
}

pub(crate) fn u16_at(d: &[u8], off: usize) -> Option<u16> {
    let b = d.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn i16_at(d: &[u8], off: usize) -> Option<i16> {
    u16_at(d, off).map(|v| v as i16)
}

pub(crate) fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn i32_at(d: &[u8], off: usize) -> Option<i32> {
    u32_at(d, off).map(|v| v as i32)
}

/// An 8-byte name field: cut at the first NUL, upper-cased.
pub(crate) fn name8(b: &[u8]) -> String {
    b.iter()
        .take(8)
        .take_while(|&&c| c != 0)
        .map(|&c| (c as char).to_ascii_uppercase())
        .collect()
}

/// The 8-byte name at `off`, or `None` when it runs past the end.
pub(crate) fn name8_at(d: &[u8], off: usize) -> Option<String> {
    d.get(off..off.checked_add(8)?).map(name8)
}
