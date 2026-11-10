//! Writes debug images of a voxelized map and times `voxel()`.
//!
//! `DOOM_WAD=path cargo run --release --example debug_images -- E1M1 out_dir [z ...]`
//!
//! - `MAP_floors.png`: top-down floor colour of each column (void black),
//!   DOOM north up;
//! - `MAP_sectors.png`: top-down sector ids, a random colour each;
//! - `MAP_section_zN.png`: the map cut at voxel row z = N and seen looking
//!   north (−z); cut faces are drawn darker, open sky light blue. Rows
//!   default to the spawn row. `CROP=a:b` limits the columns and
//!   `SCALE=n` enlarges each voxel to n×n pixels. With `EAST=1` the numbers
//!   are x columns instead, seen looking east (+x), and `CROP` limits z:
//!   `MAP_section_xN.png`.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::Instant;

use doom_voxelize::{Options, Voxel, VoxelMap};
use doom_wad::Wad;

fn main() {
    let wad_path = std::env::var_os("DOOM_WAD").expect("set DOOM_WAD to a WAD file");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = args.first().cloned().unwrap_or_else(|| "E1M1".into());
    let out = Path::new(args.get(1).map_or(".", |s| s.as_str())).to_path_buf();
    std::fs::create_dir_all(&out).unwrap();
    let wad = Wad::open(wad_path).unwrap();
    let started = Instant::now();
    let m = VoxelMap::build(&wad, &name, &Options::default()).unwrap();
    println!("{name}: built in {:.3} s", started.elapsed().as_secs_f32());
    let palette = *m.palette();
    let (min, max) = m.bounds();
    let (w, d) = ((max[0] - min[0]) as u32, (max[2] - min[2]) as u32);

    let mut floors = Vec::with_capacity((w * d * 3) as usize);
    let mut sectors = Vec::with_capacity((w * d * 3) as usize);
    for z in min[2]..max[2] {
        for x in min[0]..max[0] {
            let (f, s) = match m.column_air(x, z) {
                Some((floor, _)) => {
                    let Voxel::Solid(c) = m.voxel([x, floor - 1, z]) else {
                        unreachable!()
                    };
                    let id = m.column_sector(x, z).unwrap() as u32 + 1;
                    let h = id.wrapping_mul(2654435761);
                    (
                        palette[c as usize],
                        [h as u8, (h >> 8) as u8, (h >> 16) as u8],
                    )
                }
                None => ([0; 3], [0; 3]),
            };
            floors.extend(f);
            sectors.extend(s);
        }
    }
    write_png(&out.join(format!("{name}_floors.png")), w, d, &floors);
    write_png(&out.join(format!("{name}_sectors.png")), w, d, &sectors);

    let (spawn, _) = m.spawn();
    let rows: Vec<i32> = if args.len() > 2 {
        args[2..].iter().map(|a| a.parse().unwrap()).collect()
    } else {
        vec![spawn[2].floor() as i32]
    };
    let east = std::env::var("EAST").is_ok();
    // (across, along): the image's horizontal axis and the viewing axis.
    let (a_axis, v_axis) = if east { (2, 0) } else { (0, 2) };
    let (cx0, cx1) = std::env::var("CROP")
        .ok()
        .map_or((min[a_axis], max[a_axis]), |c| {
            let (a, b) = c.split_once(':').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        });
    let scale: usize = std::env::var("SCALE").map_or(1, |s| s.parse().unwrap());
    println!("bounds {min:?}..{max:?}, spawn {spawn:?}");
    for z0 in rows {
        let h = (max[1] - min[1]) as u32;
        let sw = (cx1 - cx0) as u32;
        let mut px = Vec::with_capacity((sw * h * 3) as usize);
        for y in (min[1]..max[1]).rev() {
            for a in cx0..cx1 {
                let mut c = [150, 200, 255];
                let steps: Box<dyn Iterator<Item = i32>> = if east {
                    Box::new(z0..max[v_axis])
                } else {
                    Box::new((min[v_axis]..=z0).rev())
                };
                for (i, v) in steps.enumerate() {
                    let p = if east { [v, y, a] } else { [a, y, v] };
                    if let Voxel::Solid(v) = m.voxel(p) {
                        c = palette[v as usize];
                        if i == 0 {
                            c = c.map(|v| v / 2);
                        }
                        break;
                    }
                }
                px.extend(c);
            }
        }
        let (sw, h, px) = enlarge(sw, h, &px, scale);
        let axis = if east { 'x' } else { 'z' };
        write_png(
            &out.join(format!("{name}_section_{axis}{z0}.png")),
            sw,
            h,
            &px,
        );
    }

    // voxel() cost over 32³ chunks around the spawn, and over all chunks
    // the uniform test can't skip in a band through the map.
    let [sx, sy, sz] = spawn.map(|v| v.floor() as i32);
    let mut count = 0u64;
    let mut solid = 0u64;
    let started = Instant::now();
    for cx in -4..4 {
        for cy in -2..2 {
            for cz in -4..4 {
                let lo = [sx + cx * 32, sy + cy * 32, sz + cz * 32];
                if m.uniform_in(lo, lo.map(|v| v + 32)).is_some() {
                    continue;
                }
                for x in lo[0]..lo[0] + 32 {
                    for y in lo[1]..lo[1] + 32 {
                        for z in lo[2]..lo[2] + 32 {
                            count += 1;
                            solid += matches!(m.voxel([x, y, z]), Voxel::Solid(_)) as u64;
                        }
                    }
                }
            }
        }
    }
    let t = started.elapsed().as_secs_f64();
    println!(
        "voxel(): {count} voxels in mixed chunks near the spawn, {:.1} ns each ({solid} solid)",
        t * 1e9 / count.max(1) as f64
    );
}

fn enlarge(w: u32, h: u32, rgb: &[u8], n: usize) -> (u32, u32, Vec<u8>) {
    let mut out = Vec::with_capacity(rgb.len() * n * n);
    for row in rgb.chunks(w as usize * 3) {
        let mut line = Vec::with_capacity(row.len() * n);
        for p in row.chunks(3) {
            for _ in 0..n {
                line.extend_from_slice(p);
            }
        }
        for _ in 0..n {
            out.extend_from_slice(&line);
        }
    }
    (w * n as u32, h * n as u32, out)
}

fn write_png(path: &Path, w: u32, h: u32, rgb: &[u8]) {
    let file = BufWriter::new(File::create(path).unwrap());
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgb).unwrap();
    println!("wrote {}", path.display());
}
