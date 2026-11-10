# doom_voxels

An experiment: vanilla DOOM maps from a WAD file, shown as a
[`bevy_voxel_world`](https://github.com/splashdust/bevy_voxel_world) voxel
world you can fly through. Every voxel is one colour from DOOM's palette, with
DOOM's sector light baked in, drawn flat and unlit so the map reads as it does
in DOOM.

The workspace has three parts:

| | |
|---|---|
| `crates/wad` (`doom_wad`) | reads WAD files: map lumps, the palette, COLORMAP, flats and wall textures |
| `crates/voxelize` (`doom_voxelize`) | turns one map into voxels: `VoxelMap` |
| `src/` (`doom_voxels`) | the Bevy app: the voxel world, the palette material, the fly camera |

## Running

```sh
cargo run --release -- --wad path/to/freedoom1.wad
cargo run --release -- --wad path/to/DOOM.WAD --map E1M3
DOOM_WAD=path/to/freedoom2.wad cargo run --release -- --map MAP07
```

### Where to get a WAD

- [Freedoom](https://github.com/freedoom/freedoom/releases): free maps and
  art under a BSD-style licence. `freedoom1.wad` has E1M1-style maps,
  `freedoom2.wad` MAP01-style ones.
- Your own `DOOM.WAD` or `DOOM2.WAD`, read where it is.

Never commit a WAD: id's are copyrighted, and `.gitignore` keeps `*.wad` out.

## Controls

| | |
|---|---|
| mouse | look (the cursor is captured) |
| W A S D | fly forward, back, left, right |
| Space / Ctrl | up / down |
| Shift | three times faster |
| left click | fire; hold for a chaingun's rate. The click that captures the cursor doesn't fire |
| Esc | free the cursor; click to capture it again |

## Command line

| flag | |
|---|---|
| `--wad PATH` | the WAD; falls back to the `DOOM_WAD` environment variable |
| `--map NAME` | the map, like `E1M1` or `MAP01`; the WAD's first map by default |
| `--list-maps` | print the WAD's maps and exit |
| `--units-per-voxel N` | DOOM map units per voxel, 2 by default |
| `--no-sector-light` | full-bright colours instead of DOOM's sector light |
| `--closed-doors` | keep door sectors closed; they are baked open by default |
| `--pos X,Y,Z` | start the camera here, in voxels, instead of at player 1's start |
| `--yaw DEG` | start yaw: 0 faces DOOM's north, positive turns left |
| `--pitch DEG` | start pitch, positive looks up |
| `--blast-radius R` | the radius in voxels of the ball a shot carves, 4 by default |
| `--fire N` | once the world is ready, fire N shots in a small spiral around the view centre |
| `--screenshot PATH` | wait until the world in view has been generated and meshed, save a PNG and exit. With `--fire`, it waits for the carved chunks too |
| `--quit-after S` | exit after S seconds |

At start it prints how long building the voxel map took and the map's voxel
bounds. Voxel `[x, y, z]` with `s` units per voxel covers DOOM's x from
`x*s`, height from `y*s` and DOOM's y from `-(z+1)*s`: DOOM's east is +x and
its north is −z.

## How it works

- **The world.** One `VoxelWorldConfig`, `DoomWorld`, holds the `VoxelMap` in
  an `Arc`. Its voxel lookup calls `VoxelMap::voxel`, and its fill hint asks
  `VoxelMap::uniform_in` about the chunk's box, so the solid below the map and
  the air above it are never generated. Chunks spawn out to about 2048 map
  units and despawn only when far away, so turning round doesn't rebuild the
  room behind you. There is no LOD: a coarse level samples every second voxel
  and would lose one-voxel walls.
- **Colour.** The material index is the palette index. The texture index
  mapper passes it on, and a custom material (`PaletteMaterial`, with the
  crate's vertex shader and `src/palette.wesl` as fragment shader) looks it up
  in a 256-colour uniform built from the map's palette at runtime. The colour
  is multiplied by the mesher's ambient occlusion and nothing else. The camera
  has no tonemapping or dithering, so the window shows the palette's bytes.
- **Screenshots.** `--screenshot` waits until every spawned chunk has been
  ready for 60 frames in a row with no chunk spawning or despawning, or two
  minutes at most. `--fire` waits for the same signal before its first shot,
  and each shot starts the count again.
- **The weapon.** A shot is a ray from the camera along its forward vector,
  through `VoxelWorld::raycast`, out to the view distance. Around the voxel
  it hits, `set_voxels_in` carves a ball whose edge is jittered by a hash of
  each voxel's position, and darkens solid voxels in a shell 1.5 voxels
  thick outside it by a few COLORMAP tables. The darkening starts from the
  colour `VoxelMap::voxel` gives, so shooting the same spot again doesn't
  darken it further. Air is never made solid. The scripted shots of
  `--fire` are spread so the craters sit side by side at the distance the
  centre ray hits.
- **The voxelizer** (`crates/voxelize`). At import every voxel column gets its
  sector from the map's BSP, with a nearest-linedef test for the void outside
  the map. A voxel is solid below its sector's floor, air up to the ceiling
  (or to the map top under a sky ceiling) and solid above. The solid voxel
  under the air takes the floor flat, the one over it the ceiling flat, and
  solid voxels beside air take the wall texture of the linedef between them,
  with DOOM's pegging rules. Masked middle textures (grates) are one-voxel
  slabs whose transparent texels are air. Doors are baked open. Sector light
  goes through COLORMAP at a fixed viewing distance.
- **Inside the solid.** Hidden solid voxels carry the nearest surface on
  into the solid, so a crater shows material rather than a hole: the floor
  flat goes on down under a floor, the ceiling flat up over a ceiling, and
  the wall texture of the nearest linedef back into walls and the void. Each
  column knows its nearest column of another sector with air within 10
  columns, found at import. The colour darkens through COLORMAP with depth,
  slowly at first and reaching the darkest table at 10 voxels; from there on
  it is palette index 0. `uniform_in` calls a box all interior only when the
  box grown by 11 voxels holds no surface, so it agrees with `voxel`.
- **The WAD reader** (`crates/wad`) has no dependencies and reads the map
  lumps, PLAYPAL, COLORMAP, flats and composite wall textures.

## Known gaps

- A corner column touching two different walls shows one wall's texture.
- Walls beside sky sectors run up to the map top with their texture, where
  DOOM would paint sky above them.
- Animated flats and textures, and switches, show their first frame.
- Doors, lifts and crushers don't move.
- No things: monsters, items and decorations aren't shown.
- No collision: the camera only flies.
- With `--closed-doors`, the bottom face of a closed door shows the ceiling
  flat instead of the door texture.
- Inside the solid, each column takes one neighbouring sector, so where two
  walls or a wall and a step meet, the inside shows only one of them. The
  top of the world over non-sky ceilings stays black.
- Shots pass through the transparent holes of grates, and nothing is
  thrown out of a crater. Craters aren't saved.

## Which bevy_voxel_world

While the experiment lives in the crate's repository, at
`experiments/doom_voxels`, it builds against that checkout by path:

```toml
bevy_voxel_world = { path = "../.." }
```

After breaking it out into a repo of its own, point it at a commit of the
crate instead:

```toml
bevy_voxel_world = { git = "https://github.com/splashdust/bevy_voxel_world", rev = "<commit>" }
```

and to keep working against a local checkout, patch it:

```toml
[patch."https://github.com/splashdust/bevy_voxel_world"]
bevy_voxel_world = { path = "../bevy_voxel_world" }
```

## What it taught us about bevy_voxel_world

The experiment is partly a dogfooding exercise for the crate. What it had to
work around:

- **Per-voxel colour known only at runtime.** `voxel_texture` takes an asset
  path, and the palette comes from the WAD. The way through is a custom
  material whose fragment shader reads the palette index the texture index
  mapper passes in `tex_idx`. That means copying the `specialize` boilerplate
  every custom voxel material repeats (skip the prepass, set
  `vertex_layout()`), and putting the `.wesl` into `Assets<Shader>` by hand
  under a made-up UUID, since the app has no asset folder.
- **No unlit option.** The default material is lit PBR. Showing colours
  exactly takes the custom material above, plus `Tonemapping::None` and no
  deband dithering on the camera.
- **The config must be `Default`.** The world can't exist without a map, so
  `DoomWorld` holds an `Option<Arc<VoxelMap>>` that is never `None` in use,
  and the lookup delegate `expect`s it.
- **No "streaming is done" signal.** `--screenshot` has to query every
  `Chunk<C>`, ask `chunk_state` for each key, and wait for a run of frames
  with all chunks ready and none coming or going, because spawning rays find
  more chunks only after the ones in front are generated. A count of pending
  chunks, or a flag saying streaming has caught up with the cameras, would
  replace the heuristic.
- **The lookup delegate gets a position and a level, not a `ChunkKey`.** The
  fill hint gets a key with `min_voxel()` and `voxel_extent()`; the delegate
  rebuilds the same box by hand from `CHUNK_SIZE_I`, which lives in
  `custom_meshing`, and adds the one-voxel padding itself.
- **No range on `raycast`.** It traces to the edge of the published
  chunks, so the weapon checks the hit's distance after the fact. A
  maximum distance would stop the trace early.
- **Reading voxels while setting them.** `set_voxels_in` borrows the voxel
  world mutably, so the closure can't call `get_voxel`. Taking
  `get_voxel_fn` first works, because its reader holds no lock the setter
  needs, but that is only clear from the crate's source.
- **No "edits have remeshed" signal.** Waiting for a screenshot after a shot
  reuses the settling heuristic above: chunks the shot touched turn
  `Pending` and back to `Ready`.
- **LOD would lose thin walls.** A coarse level samples every `1 << L`th
  voxel, so one-voxel walls and steps vanish or flicker between levels. The
  app uses no LOD, so the view distance is bounded by the chunk count.
