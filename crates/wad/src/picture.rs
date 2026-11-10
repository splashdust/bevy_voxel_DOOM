//! DOOM's column-based picture format, used by wall patches and sprites.

use crate::bytes::{i16_at, u16_at, u32_at, u8_at};
use crate::{Picture, WadError};

/// A decoded picture lump and the offsets that place it: for a sprite, the
/// left offset is the distance from its origin to its left edge and the top
/// offset is the height of its origin above its top edge.
pub struct Patch {
    pub left_offset: i16,
    pub top_offset: i16,
    pub picture: Picture,
}

/// Decodes a lump in DOOM's picture format. Texels no post covers are `None`.
pub fn decode_patch(data: &[u8]) -> Result<Patch, WadError> {
    let malformed = |m: String| WadError::Malformed(format!("picture: {m}"));
    let short = || malformed(format!("header runs past its {} bytes", data.len()));
    let width = u16_at(data, 0).ok_or_else(short)?;
    let height = u16_at(data, 2).ok_or_else(short)?;
    let left_offset = i16_at(data, 4).ok_or_else(short)?;
    let top_offset = i16_at(data, 6).ok_or_else(short)?;
    if (width as i16) < 0 || (height as i16) < 0 {
        return Err(malformed(format!(
            "size {}x{}",
            width as i16, height as i16
        )));
    }
    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![None; w * h];

    for x in 0..w {
        let past_end = || malformed(format!("column {x} runs past the end"));
        let mut at = u32_at(data, 8 + x * 4).ok_or_else(past_end)? as usize;
        // Posts: top row, length, a pad byte, the texels, a pad byte; 0xFF ends the column.
        loop {
            let top = u8_at(data, at).ok_or_else(past_end)?;
            if top == 0xFF {
                break;
            }
            let len = u8_at(data, at + 1).ok_or_else(past_end)? as usize;
            let texels = data.get(at + 3..at + 3 + len).ok_or_else(past_end)?;
            for (i, &t) in texels.iter().enumerate() {
                let y = top as usize + i;
                if y < h {
                    pixels[y * w + x] = Some(t);
                }
            }
            at += len + 4;
        }
    }
    Ok(Patch {
        left_offset,
        top_offset,
        picture: Picture {
            width,
            height,
            pixels,
        },
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A patch lump from columns of posts `(top, texels)`.
    pub(crate) fn encode_patch(height: u16, columns: &[Vec<(u8, Vec<u8>)>]) -> Vec<u8> {
        let mut head = Vec::new();
        for v in [columns.len() as u16, height, 3, 0xFFFE] {
            head.extend_from_slice(&v.to_le_bytes());
        }
        let mut body = Vec::new();
        let table = head.len() + columns.len() * 4;
        for col in columns {
            head.extend_from_slice(&((table + body.len()) as u32).to_le_bytes());
            for (top, texels) in col {
                body.extend([*top, texels.len() as u8, 0]);
                body.extend(texels);
                body.push(0);
            }
            body.push(0xFF);
        }
        head.extend(body);
        head
    }

    #[test]
    fn posts_paint_texels_and_gaps_stay_transparent() {
        let data = encode_patch(
            5,
            &[
                vec![(0, vec![1, 2]), (3, vec![3])],
                vec![],
                // A post running past the bottom is clipped.
                vec![(4, vec![9, 9, 9])],
            ],
        );
        let p = decode_patch(&data).unwrap();
        assert_eq!((p.left_offset, p.top_offset), (3, -2));
        let pic = &p.picture;
        assert_eq!((pic.width, pic.height), (3, 5));
        let col = |x: usize| (0..5).map(|y| pic.pixels[y * 3 + x]).collect::<Vec<_>>();
        assert_eq!(col(0), [Some(1), Some(2), None, Some(3), None]);
        assert_eq!(col(1), [None; 5]);
        assert_eq!(col(2), [None, None, None, None, Some(9)]);
    }

    #[test]
    fn truncated_patches_are_malformed() {
        let data = encode_patch(4, &[vec![(0, vec![1, 2, 3])]]);
        for len in [0, 7, 11, 14, data.len() - 1] {
            assert!(
                matches!(decode_patch(&data[..len]), Err(WadError::Malformed(_))),
                "length {len}"
            );
        }
        let mut bad_offset = data.clone();
        bad_offset[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_patch(&bad_offset).is_err());
    }
}
