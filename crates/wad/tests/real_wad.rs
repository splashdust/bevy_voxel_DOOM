//! Reads the WAD at `DOOM_WAD`; skipped when it is unset.

use doom_wad::{Wad, NF_SUBSECTOR};

#[test]
fn reads_a_real_wad() {
    let Some(path) = std::env::var_os("DOOM_WAD") else {
        println!("DOOM_WAD is unset; skipping the real-WAD test");
        return;
    };
    let wad = Wad::open(&path).expect("open the WAD");

    let names = wad.map_names();
    println!("{} maps: {}", names.len(), names.join(" "));
    let first = if names.iter().any(|n| n.starts_with('E')) {
        "E1M1"
    } else {
        "MAP01"
    };
    assert!(names.iter().any(|n| n == first), "{first} is missing");

    for name in &names {
        let m = wad.map(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!m.sectors.is_empty() && !m.linedefs.is_empty(), "{name}");
        let (sectors, sides) = (m.sectors.len(), m.sidedefs.len());
        for s in &m.sidedefs {
            assert!(
                (s.sector as usize) < sectors,
                "{name}: sidedef sector {}",
                s.sector
            );
        }
        for l in &m.linedefs {
            assert!(
                (l.v1 as usize) < m.vertexes.len() && (l.v2 as usize) < m.vertexes.len()
            );
            assert!(
                l.front.is_some_and(|f| (f as usize) < sides),
                "{name}: front side"
            );
            assert!(
                l.back.is_none_or(|b| (b as usize) < sides),
                "{name}: back side"
            );
        }
        for n in &m.nodes {
            for &c in &n.children {
                let (index, len) = if c & NF_SUBSECTOR != 0 {
                    ((c & !NF_SUBSECTOR) as usize, m.subsectors.len())
                } else {
                    (c as usize, m.nodes.len())
                };
                assert!(index < len, "{name}: node child {c:#x}");
            }
        }
        let bm = m.blockmap.as_ref().expect("vanilla maps have a BLOCKMAP");
        assert_eq!(bm.cells.len(), bm.columns as usize * bm.rows as usize);
        assert!(bm
            .cells
            .iter()
            .flatten()
            .all(|&l| (l as usize) < m.linedefs.len()));
        if name == first {
            println!(
                "{name}: {} things, {} linedefs, {} sidedefs, {} vertexes, {} segs, \
                 {} subsectors, {} nodes, {} sectors, blockmap {}x{}",
                m.things.len(),
                m.linedefs.len(),
                m.sidedefs.len(),
                m.vertexes.len(),
                m.segs.len(),
                m.subsectors.len(),
                m.nodes.len(),
                m.sectors.len(),
                bm.columns,
                bm.rows
            );
            assert!(m.things.iter().any(|t| t.kind == 1), "player 1 start");
        }
    }

    assert_eq!(wad.palette().expect("palette").len(), 256);
    assert_eq!(wad.colormap().expect("colormap").tables.len(), 34);

    let loaded = wad.textures_with_skipped().expect("textures");
    let t = &loaded.textures;
    println!("{} flats, {} wall textures", t.flats.len(), t.walls.len());
    for s in &loaded.skipped {
        println!("skipped {}: {}", s.name, s.reason);
    }
    let startan3 = &t.walls["STARTAN3"];
    assert_eq!((startan3.width, startan3.height), (128, 128));
    assert!(
        startan3.pixels.iter().all(Option::is_some),
        "STARTAN3 is opaque"
    );
    let flat = &t.flats["FLOOR4_8"];
    assert_eq!((flat.width, flat.height, flat.pixels.len()), (64, 64, 4096));
    for (name, w) in &t.walls {
        assert_eq!(
            w.pixels.len(),
            w.width as usize * w.height as usize,
            "{name}"
        );
    }

    // Every texture and flat a map names should exist.
    for name in &names {
        let m = wad.map(name).unwrap();
        for s in &m.sidedefs {
            for tex in [&s.upper, &s.lower, &s.middle] {
                if tex != "-" && !t.walls.contains_key(tex) {
                    println!("{name}: sidedef names unknown texture {tex}");
                }
            }
        }
        for s in &m.sectors {
            for flat in [&s.floor_flat, &s.ceiling_flat] {
                assert!(t.flats.contains_key(flat), "{name}: unknown flat {flat}");
            }
        }
    }
}
