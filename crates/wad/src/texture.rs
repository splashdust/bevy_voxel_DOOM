//! Flats and composite wall textures.

use std::collections::HashMap;

use crate::bytes::{i16_at, i32_at, name8_at};
use crate::picture::{decode_patch, Patch};
use crate::{Picture, Wad, WadError};

/// A wall texture `textures` left out, and why.
#[derive(Clone, Debug)]
pub struct SkippedTexture {
    pub name: String,
    pub reason: String,
}

const FLAT_SIZE: usize = 64 * 64;

pub(crate) fn flats(wad: &Wad) -> Result<HashMap<String, Picture>, WadError> {
    let mut flats = HashMap::new();
    let mut inside = false;
    for e in &wad.entries {
        match e.name.as_str() {
            "F_START" | "FF_START" => inside = true,
            "F_END" | "FF_END" => inside = false,
            // Nested markers such as F1_START are empty.
            _ if !inside || e.size == 0 => {}
            _ => {
                let data = wad.entry_data(e);
                let texels = data.get(..FLAT_SIZE).ok_or_else(|| {
                    WadError::Malformed(format!(
                        "flat {} is {} bytes, not {FLAT_SIZE}",
                        e.name,
                        data.len()
                    ))
                })?;
                let picture = Picture {
                    width: 64,
                    height: 64,
                    pixels: texels.iter().map(|&t| Some(t)).collect(),
                };
                flats.insert(e.name.clone(), picture);
            }
        }
    }
    Ok(flats)
}

/// One `mappatch_t`: where a patch sits in a texture.
struct PatchPlacement {
    x: i16,
    y: i16,
    patch: i16,
}

struct TextureDef {
    name: String,
    width: i16,
    height: i16,
    patches: Vec<PatchPlacement>,
}

pub(crate) fn walls(
    wad: &Wad,
) -> Result<(HashMap<String, Picture>, Vec<SkippedTexture>), WadError> {
    let pnames = pnames(wad.lump("PNAMES").ok_or_else(|| missing("PNAMES"))?)?;
    let mut defs = texture_defs(wad.lump("TEXTURE1").ok_or_else(|| missing("TEXTURE1"))?)
        .map_err(|e| in_lump("TEXTURE1", e))?;
    if let Some(t2) = wad.lump("TEXTURE2") {
        defs.extend(texture_defs(t2).map_err(|e| in_lump("TEXTURE2", e))?);
    }

    // Decoded on first use: one entry per PNAMES index.
    let mut patches: Vec<Option<Result<Patch, String>>> =
        (0..pnames.len()).map(|_| None).collect();
    let mut walls = HashMap::new();
    let mut skipped = Vec::new();
    for def in defs {
        // The engine finds the first texture of a name, so earlier ones win.
        if walls.contains_key(&def.name) {
            continue;
        }
        match composite(wad, &def, &pnames, &mut patches) {
            Ok(picture) => {
                walls.insert(def.name, picture);
            }
            Err(reason) => skipped.push(SkippedTexture {
                name: def.name,
                reason,
            }),
        }
    }
    Ok((walls, skipped))
}

fn missing(name: &str) -> WadError {
    WadError::MissingLump(name.into())
}

fn in_lump(lump: &str, e: WadError) -> WadError {
    match e {
        WadError::Malformed(m) => WadError::Malformed(format!("{lump}: {m}")),
        e => e,
    }
}

fn pnames(lump: &[u8]) -> Result<Vec<String>, WadError> {
    let malformed =
        || WadError::Malformed(format!("PNAMES runs past its {} bytes", lump.len()));
    let count = i32_at(lump, 0).filter(|&n| n >= 0).ok_or_else(malformed)? as usize;
    (0..count)
        .map(|i| name8_at(lump, 4 + i * 8).ok_or_else(malformed))
        .collect()
}

fn texture_defs(lump: &[u8]) -> Result<Vec<TextureDef>, WadError> {
    let past_end =
        |what: String| WadError::Malformed(format!("{what} runs past the end"));
    let count = i32_at(lump, 0)
        .filter(|&n| n >= 0)
        .ok_or_else(|| past_end("texture count".into()))? as usize;
    let mut defs = Vec::with_capacity(count.min(lump.len() / 4));
    for i in 0..count {
        let at = i32_at(lump, 4 + i * 4)
            .filter(|&o| o >= 0)
            .ok_or_else(|| past_end(format!("texture offset {i}")))?
            as usize;
        let field = |off: usize| {
            i16_at(lump, at + off).ok_or_else(|| past_end(format!("texture {i}")))
        };
        let name = name8_at(lump, at).ok_or_else(|| past_end(format!("texture {i}")))?;
        let (width, height, patch_count) = (field(12)?, field(14)?, field(20)?);
        if width < 0 || height < 0 || patch_count < 0 {
            return Err(WadError::Malformed(format!(
                "texture {name} has size {width}x{height} and {patch_count} patches"
            )));
        }
        if lump.len() < at + 22 + patch_count as usize * 10 {
            return Err(past_end(format!("texture {name}")));
        }
        let patches = (0..patch_count as usize)
            .map(|p| {
                let base = 22 + p * 10;
                Ok(PatchPlacement {
                    x: field(base)?,
                    y: field(base + 2)?,
                    patch: field(base + 4)?,
                })
            })
            .collect::<Result<_, WadError>>()?;
        defs.push(TextureDef {
            name,
            width,
            height,
            patches,
        });
    }
    Ok(defs)
}

fn composite(
    wad: &Wad,
    def: &TextureDef,
    pnames: &[String],
    patches: &mut [Option<Result<Patch, String>>],
) -> Result<Picture, String> {
    let (w, h) = (def.width as i32, def.height as i32);
    let mut pixels = vec![None; (w * h) as usize];
    for place in &def.patches {
        let index = usize::try_from(place.patch)
            .ok()
            .filter(|&i| i < pnames.len())
            .ok_or_else(|| {
                format!(
                    "patch index {} is outside PNAMES ({})",
                    place.patch,
                    pnames.len()
                )
            })?;
        let patch = patches[index].get_or_insert_with(|| {
            let name = &pnames[index];
            let lump = wad
                .lump(name)
                .ok_or_else(|| format!("patch {name} is missing"))?;
            decode_patch(lump).map_err(|e| format!("patch {name}: {e}"))
        });
        let pic = &patch.as_ref().map_err(Clone::clone)?.picture;
        let pw = pic.width as i32;
        for py in 0..pic.height as i32 {
            let y = place.y as i32 + py;
            if !(0..h).contains(&y) {
                continue;
            }
            for px in 0..pw {
                let x = place.x as i32 + px;
                if !(0..w).contains(&x) {
                    continue;
                }
                if let Some(t) = pic.pixels[(py * pw + px) as usize] {
                    pixels[(y * w + x) as usize] = Some(t);
                }
            }
        }
    }
    Ok(Picture {
        width: def.width as u16,
        height: def.height as u16,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::tests::encode_patch;
    use crate::test_support::{build_wad, name_field};

    fn pnames_lump(names: &[&str]) -> Vec<u8> {
        let mut b = (names.len() as i32).to_le_bytes().to_vec();
        for n in names {
            b.extend_from_slice(&name_field(n));
        }
        b
    }

    /// `(name, width, height, [(x, y, patch index)])`.
    type Def<'a> = (&'a str, i16, i16, Vec<(i16, i16, i16)>);

    fn texture_lump(defs: &[Def]) -> Vec<u8> {
        let mut head = (defs.len() as i32).to_le_bytes().to_vec();
        let mut body = Vec::new();
        let table = 4 + defs.len() * 4;
        for (name, w, h, patches) in defs {
            head.extend_from_slice(&((table + body.len()) as i32).to_le_bytes());
            body.extend_from_slice(&name_field(name));
            body.extend_from_slice(&[0; 4]);
            body.extend_from_slice(&w.to_le_bytes());
            body.extend_from_slice(&h.to_le_bytes());
            body.extend_from_slice(&[0; 4]);
            body.extend_from_slice(&(patches.len() as i16).to_le_bytes());
            for &(x, y, p) in patches {
                for v in [x, y, p, 1, 0] {
                    body.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        head.extend(body);
        head
    }

    #[test]
    fn flats_between_markers_skip_nested_markers() {
        let a = vec![1u8; 4096];
        let b = vec![2u8; 4096];
        let wad = Wad::from_bytes(build_wad(&[
            ("NOTFLAT", &a),
            ("F_START", &[]),
            ("F1_START", &[]),
            ("FLOOR4_8", &a),
            ("F1_END", &[]),
            ("F_END", &[]),
            ("FF_START", &[]),
            ("floor4_8", &b),
            ("FF_END", &[]),
        ]))
        .unwrap();
        let flats = flats(&wad).unwrap();
        assert_eq!(flats.len(), 1);
        let f = &flats["FLOOR4_8"];
        assert_eq!((f.width, f.height), (64, 64));
        assert!(f.pixels.iter().all(|&t| t == Some(2)));
    }

    #[test]
    fn short_flat_is_malformed() {
        let wad = Wad::from_bytes(build_wad(&[
            ("F_START", &[]),
            ("X", &[0; 100]),
            ("F_END", &[]),
        ]))
        .unwrap();
        assert!(matches!(flats(&wad), Err(WadError::Malformed(_))));
    }

    #[test]
    fn walls_composite_patches_clipped_and_skip_missing_patches() {
        // A 2x2 solid patch and a 1x3 patch with a gap in the middle.
        let solid = encode_patch(2, &[vec![(0, vec![5, 5])], vec![(0, vec![5, 5])]]);
        let gappy = encode_patch(3, &[vec![(0, vec![7]), (2, vec![8])]]);
        let pnames = pnames_lump(&["solid", "GAPPY", "NOWHERE"]);
        let t1 = texture_lump(&[
            // The solid patch hangs off the top-left; the gappy one off the bottom.
            ("WALL", 3, 3, vec![(-1, -1, 0), (2, 1, 1)]),
            ("BROKEN", 4, 4, vec![(0, 0, 0), (0, 0, 2)]),
            ("BADINDEX", 4, 4, vec![(0, 0, 9)]),
        ]);
        let t2 =
            texture_lump(&[("WALL", 1, 1, vec![]), ("OTHER", 1, 1, vec![(0, 0, 1)])]);
        let wad = Wad::from_bytes(build_wad(&[
            ("PNAMES", &pnames),
            ("TEXTURE1", &t1),
            ("TEXTURE2", &t2),
            ("P_START", &[]),
            ("SOLID", &solid),
            ("GAPPY", &gappy),
            ("P_END", &[]),
        ]))
        .unwrap();
        let set = wad.textures_with_skipped().unwrap();
        let wall = &set.textures.walls["WALL"];
        assert_eq!((wall.width, wall.height), (3, 3));
        #[rustfmt::skip]
        assert_eq!(wall.pixels, [
            Some(5), None, None,
            None,    None, Some(7),
            None,    None, None,
        ]);
        assert_eq!(set.textures.walls["OTHER"].pixels, [Some(7)]);
        let mut skipped: Vec<_> = set.skipped.iter().map(|s| s.name.as_str()).collect();
        skipped.sort();
        assert_eq!(skipped, ["BADINDEX", "BROKEN"]);
        assert!(set.skipped.iter().any(|s| s.reason.contains("NOWHERE")));
    }

    #[test]
    fn truncated_texture_lump_is_malformed() {
        let pnames = pnames_lump(&["A"]);
        let t1 = texture_lump(&[("WALL", 3, 3, vec![(0, 0, 0)])]);
        let wad = Wad::from_bytes(build_wad(&[
            ("PNAMES", &pnames),
            ("TEXTURE1", &t1[..t1.len() - 3]),
        ]))
        .unwrap();
        assert!(matches!(wad.textures(), Err(WadError::Malformed(_))));
    }
}
