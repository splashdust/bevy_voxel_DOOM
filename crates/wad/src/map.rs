//! Map markers and the map lumps that follow them.

use std::collections::HashMap;

use crate::bytes::{name8, u16_at};
use crate::{
    Blockmap, LineDef, MapData, Node, Sector, Seg, SideDef, SubSector, Thing, Vertex,
    Wad, WadError,
};

/// The lumps that may follow a map marker, in any order.
const MAP_LUMPS: &[&str] = &[
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS",
    "REJECT", "BLOCKMAP", "BEHAVIOR",
];

/// `ExMy` or `MAPxx`, with digits for x and y.
pub(crate) fn is_map_name(name: &str) -> bool {
    let b = name.as_bytes();
    match b.len() {
        4 => {
            b[0] == b'E' && b[1].is_ascii_digit() && b[2] == b'M' && b[3].is_ascii_digit()
        }
        5 => b.starts_with(b"MAP") && b[3].is_ascii_digit() && b[4].is_ascii_digit(),
        _ => false,
    }
}

pub(crate) fn decode(wad: &Wad, name: &str) -> Result<MapData, WadError> {
    let upper = name.to_ascii_uppercase();
    let entries = &wad.entries;
    let marker = (0..entries.len())
        .rev()
        .find(|&i| {
            entries[i].name == upper
                && entries.get(i + 1).is_some_and(|e| e.name == "THINGS")
        })
        .ok_or_else(|| WadError::MissingMap(name.to_string()))?;

    // The map's lumps are the run of map-lump names right after the marker.
    let mut lumps: HashMap<&str, &[u8]> = HashMap::new();
    for e in &entries[marker + 1..] {
        match MAP_LUMPS.iter().find(|&&n| n == e.name) {
            Some(&n) if !lumps.contains_key(n) => {
                lumps.insert(n, wad.entry_data(e));
            }
            _ => break,
        }
    }
    let lump = |n: &str| {
        lumps
            .get(n)
            .copied()
            .ok_or_else(|| WadError::MissingLump(format!("{upper}/{n}")))
    };

    Ok(MapData {
        things: records(lump("THINGS")?, 10, "THINGS", |r| Thing {
            x: i16_le(r, 0),
            y: i16_le(r, 2),
            angle: i16_le(r, 4),
            kind: u16_le(r, 6),
            flags: u16_le(r, 8),
        })?,
        linedefs: records(lump("LINEDEFS")?, 14, "LINEDEFS", |r| LineDef {
            v1: u16_le(r, 0),
            v2: u16_le(r, 2),
            flags: u16_le(r, 4),
            special: u16_le(r, 6),
            tag: u16_le(r, 8),
            front: side_index(u16_le(r, 10)),
            back: side_index(u16_le(r, 12)),
        })?,
        sidedefs: records(lump("SIDEDEFS")?, 30, "SIDEDEFS", |r| SideDef {
            x_offset: i16_le(r, 0),
            y_offset: i16_le(r, 2),
            upper: name8(&r[4..12]),
            lower: name8(&r[12..20]),
            middle: name8(&r[20..28]),
            sector: u16_le(r, 28),
        })?,
        vertexes: records(lump("VERTEXES")?, 4, "VERTEXES", |r| Vertex {
            x: i16_le(r, 0),
            y: i16_le(r, 2),
        })?,
        segs: records(lump("SEGS")?, 12, "SEGS", |r| Seg {
            v1: u16_le(r, 0),
            v2: u16_le(r, 2),
            angle: i16_le(r, 4),
            linedef: u16_le(r, 6),
            side: u16_le(r, 8),
            offset: i16_le(r, 10),
        })?,
        subsectors: records(lump("SSECTORS")?, 4, "SSECTORS", |r| SubSector {
            seg_count: u16_le(r, 0),
            first_seg: u16_le(r, 2),
        })?,
        nodes: records(lump("NODES")?, 28, "NODES", |r| Node {
            x: i16_le(r, 0),
            y: i16_le(r, 2),
            dx: i16_le(r, 4),
            dy: i16_le(r, 6),
            bbox: [
                [i16_le(r, 8), i16_le(r, 10), i16_le(r, 12), i16_le(r, 14)],
                [i16_le(r, 16), i16_le(r, 18), i16_le(r, 20), i16_le(r, 22)],
            ],
            children: [u16_le(r, 24), u16_le(r, 26)],
        })?,
        sectors: records(lump("SECTORS")?, 26, "SECTORS", |r| Sector {
            floor: i16_le(r, 0),
            ceiling: i16_le(r, 2),
            floor_flat: name8(&r[4..12]),
            ceiling_flat: name8(&r[12..20]),
            light: u16_le(r, 20),
            special: u16_le(r, 22),
            tag: u16_le(r, 24),
        })?,
        blockmap: match lumps.get("BLOCKMAP") {
            Some(b) if !b.is_empty() => Some(blockmap(b)?),
            _ => None,
        },
        name: upper,
    })
}

fn side_index(v: u16) -> Option<u16> {
    (v != 0xFFFF).then_some(v)
}

/// Field reads inside a record whose length `records` has already fixed.
fn u16_le(r: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([r[off], r[off + 1]])
}

fn i16_le(r: &[u8], off: usize) -> i16 {
    u16_le(r, off) as i16
}

fn records<T>(
    lump: &[u8],
    size: usize,
    what: &str,
    decode: impl Fn(&[u8]) -> T,
) -> Result<Vec<T>, WadError> {
    if !lump.len().is_multiple_of(size) {
        return Err(WadError::Malformed(format!(
            "{what} is {} bytes, not a multiple of its {size}-byte record",
            lump.len()
        )));
    }
    Ok(lump.chunks_exact(size).map(decode).collect())
}

fn blockmap(lump: &[u8]) -> Result<Blockmap, WadError> {
    let word = |i: usize| {
        u16_at(lump, i * 2).ok_or_else(|| {
            WadError::Malformed(format!(
                "BLOCKMAP read at word {i} runs past its {} bytes",
                lump.len()
            ))
        })
    };
    let origin_x = word(0)? as i16;
    let origin_y = word(1)? as i16;
    let columns = word(2)?;
    let rows = word(3)?;
    let count = columns as usize * rows as usize;
    if (4 + count) * 2 > lump.len() {
        return Err(WadError::Malformed(format!(
            "BLOCKMAP offset table for {columns}x{rows} cells runs past its {} bytes",
            lump.len()
        )));
    }

    // Cells often share one list, so decode each offset once.
    let mut lists: HashMap<usize, Vec<u16>> = HashMap::new();
    let mut cells = Vec::with_capacity(count);
    for c in 0..count {
        let start = word(4 + c)? as usize;
        if let Some(list) = lists.get(&start) {
            cells.push(list.clone());
            continue;
        }
        // Each list opens with a 0 that is not a linedef.
        let mut at = if word(start)? == 0 { start + 1 } else { start };
        let mut list = Vec::new();
        loop {
            let v = word(at)?;
            if v == 0xFFFF {
                break;
            }
            list.push(v);
            at += 1;
        }
        lists.insert(start, list.clone());
        cells.push(list);
    }
    Ok(Blockmap {
        origin_x,
        origin_y,
        columns,
        rows,
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{build_wad, name_field};
    use crate::NF_SUBSECTOR;

    fn things(list: &[(i16, i16, u16)]) -> Vec<u8> {
        let mut b = Vec::new();
        for &(x, y, kind) in list {
            for v in [x as u16, y as u16, 90, kind, 7] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b
    }

    fn sidedef(sector: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(-8i16).to_le_bytes());
        b.extend_from_slice(&16i16.to_le_bytes());
        b.extend_from_slice(&name_field("startan3"));
        b.extend_from_slice(&name_field("-"));
        b.extend_from_slice(&name_field("door\0zz"));
        b.extend_from_slice(&sector.to_le_bytes());
        b
    }

    fn words(w: &[u16]) -> Vec<u8> {
        w.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn map_lumps(marker: &str, thing_x: i16) -> Vec<(String, Vec<u8>)> {
        let mut sector = Vec::new();
        for v in [0i16, 128] {
            sector.extend_from_slice(&v.to_le_bytes());
        }
        sector.extend_from_slice(&name_field("floor4_8"));
        sector.extend_from_slice(&name_field("CEIL3_5"));
        sector.extend_from_slice(&words(&[160, 9, 3]));
        vec![
            (marker.into(), vec![]),
            ("THINGS".into(), things(&[(thing_x, -32, 1)])),
            ("LINEDEFS".into(), words(&[0, 1, 1, 0, 0, 0, 0xFFFF])),
            ("SIDEDEFS".into(), sidedef(0)),
            ("VERTEXES".into(), words(&[0, 0, 64, 0xFFC0])),
            ("SEGS".into(), words(&[0, 1, 0x4000, 0, 0, 3])),
            ("SSECTORS".into(), words(&[1, 0])),
            (
                "NODES".into(),
                words(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0x8000, 0]),
            ),
            ("SECTORS".into(), sector),
            ("REJECT".into(), vec![0]),
            // 2x1 cells: a list holding linedef 0 after its leading zero, then
            // an empty list without one.
            (
                "BLOCKMAP".into(),
                words(&[0xFFF0, 32, 2, 1, 6, 9, 0, 0, 0xFFFF, 0xFFFF]),
            ),
        ]
    }

    fn lumps_ref(v: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
        v.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect()
    }

    #[test]
    fn map_names_are_markers_followed_by_things() {
        let mut all = map_lumps("E1M1", 0);
        all.extend(map_lumps("MAP01", 0));
        all.push(("E1M9".into(), vec![]));
        all.push(("PLAYPAL".into(), vec![0; 4]));
        all.push(("EXMY".into(), vec![]));
        all.push(("THINGS".into(), vec![]));
        let wad = Wad::from_bytes(build_wad(&lumps_ref(&all))).unwrap();
        assert_eq!(wad.map_names(), ["E1M1", "MAP01"]);
    }

    #[test]
    fn map_decodes_its_own_lumps_and_records() {
        let mut all = map_lumps("E1M1", 100);
        all.extend(map_lumps("E1M2", 200));
        let wad = Wad::from_bytes(build_wad(&lumps_ref(&all))).unwrap();

        let m = wad.map("e1m1").unwrap();
        assert_eq!(m.name, "E1M1");
        assert_eq!(m.things.len(), 1);
        assert_eq!(m.things[0].x, 100);
        assert_eq!(m.things[0].y, -32);
        assert_eq!(m.things[0].angle, 90);
        assert_eq!(m.things[0].kind, 1);
        assert_eq!(m.linedefs[0].front, Some(0));
        assert_eq!(m.linedefs[0].back, None);
        assert_eq!(m.linedefs[0].flags, 1);
        let s = &m.sidedefs[0];
        assert_eq!((s.x_offset, s.y_offset), (-8, 16));
        assert_eq!((s.upper.as_str(), s.lower.as_str()), ("STARTAN3", "-"));
        assert_eq!(s.middle, "DOOR");
        assert_eq!(m.vertexes[1].y, -64);
        assert_eq!(m.segs[0].angle, 0x4000);
        assert_eq!(m.segs[0].offset, 3);
        assert_eq!(m.subsectors[0].seg_count, 1);
        assert_eq!(m.nodes[0].bbox[1], [9, 10, 11, 12]);
        assert_eq!(m.nodes[0].children, [NF_SUBSECTOR, 0]);
        assert_eq!(m.sectors[0].floor_flat, "FLOOR4_8");
        assert_eq!(m.sectors[0].ceiling, 128);
        assert_eq!(m.sectors[0].light, 160);
        let bm = m.blockmap.unwrap();
        assert_eq!(
            (bm.origin_x, bm.origin_y, bm.columns, bm.rows),
            (-16, 32, 2, 1)
        );
        assert_eq!(bm.cells, vec![vec![0], vec![]]);

        assert_eq!(wad.map("E1M2").unwrap().things[0].x, 200);
    }

    #[test]
    fn missing_blockmap_is_none_and_missing_map_errors() {
        let mut all = map_lumps("MAP07", 0);
        all.retain(|(n, _)| n != "BLOCKMAP");
        let wad = Wad::from_bytes(build_wad(&lumps_ref(&all))).unwrap();
        assert!(wad.map("MAP07").unwrap().blockmap.is_none());
        assert!(matches!(wad.map("MAP08"), Err(WadError::MissingMap(_))));
    }

    #[test]
    fn corrupt_map_lumps_are_malformed() {
        let mut all = map_lumps("E1M1", 0);
        all[2].1.pop();
        let wad = Wad::from_bytes(build_wad(&lumps_ref(&all))).unwrap();
        assert!(matches!(wad.map("E1M1"), Err(WadError::Malformed(_))));

        let mut all = map_lumps("E1M1", 0);
        // A list with no 0xFFFF terminator.
        all[10].1 = words(&[0, 0, 1, 1, 5, 0, 3]);
        let wad = Wad::from_bytes(build_wad(&lumps_ref(&all))).unwrap();
        assert!(matches!(wad.map("E1M1"), Err(WadError::Malformed(_))));

        let mut all = map_lumps("E1M1", 0);
        all[10].1 = words(&[0, 0, 100, 100]);
        let wad = Wad::from_bytes(build_wad(&lumps_ref(&all))).unwrap();
        assert!(matches!(wad.map("E1M1"), Err(WadError::Malformed(_))));
    }
}
