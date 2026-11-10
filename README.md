# doom_voxels

An experiment: vanilla DOOM maps from a WAD file, shown as a
[`bevy_voxel_world`](https://github.com/splashdust/bevy_voxel_world) voxel
world you can fly through. Every voxel is one colour from DOOM's palette, with
DOOM's sector light baked in, drawn flat and unlit so the map reads as it does
in DOOM. With `--light propagated` the crate's light propagation lights it
instead, from the sky and from DOOM's light fixtures, and with `--light
sector-glow` propagated light carries DOOM's sector light itself.

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
| `--light MODE` | `sector` (default): DOOM's sector light baked in, no propagation. `propagated`: full-bright colours times propagated light. `both`: sector light baked in, times propagated light. `sector-glow`: full-bright colours, every surface glowing at its sector's light. `flat`: full-bright, no light at all |
| `--contrast K` | stretches baked sector light away from a middle light (about light 128), 1.25 by default; 1 is DOOM's own. Dark sectors take darker COLORMAP tables; bright ones get brighter through a gain on every colour, since no table is brighter than the palette |
| `--min-light B` | the least brightness propagated light leaves, from 0 to 1, 0.08 by default and 0 in `sector-glow` |
| `--falloff BASE` | each light level is `BASE` times as bright as the one above, 0.9 by default; 1 is linear. In `sector-glow` lamp light follows COLORMAP instead |
| `--sky on\|off` | sky light, on by default and off in `sector-glow` |
| `--sky-reach N` | how many voxels sky light spreads past where it falls, 1 to 32, 32 by default |
| `--voxel-reach N` | how many voxels a lamp's light spreads, 1 to 32, 32 by default and 14 in `sector-glow` |
| `--lamp-gain G` | how bright lamp light is at level 15, before the cap at full brightness, 1.5 by default and 1 in `sector-glow` |
| `--precision 16\|256` | light levels each voxel holds, 16 by default and 256 in `sector-glow` |
| `--sky-fill on\|off` | carry sky light through openings into the rooms beside the sky, on by default |
| `--lamp-level full\|sector` | light flats and walls glow at level 15, or at their sector's light level / 16 |
| `--stepped` | show each voxel's light as a flat step instead of smoothing it |
| `--flash-time S` | how long a shot's flash glows, 0.12 s by default. A long time keeps flashes on for a screenshot |
| `--no-vsync` | don't wait for the display, so the HUD's frame rate shows what a frame costs |
| `--closed-doors` | keep door sectors closed; they are baked open by default |
| `--pos X,Y,Z` | start the camera here, in voxels, instead of at player 1's start |
| `--yaw DEG` | start yaw: 0 faces DOOM's north, positive turns left |
| `--pitch DEG` | start pitch, positive looks up |
| `--blast-radius R` | the radius in voxels of the ball a shot carves, 4 by default |
| `--fire N` | once the world is ready, fire N shots in a small spiral around the view centre |
| `--screenshot PATH` | wait until the world in view has been generated and meshed, and its light has settled, save a PNG and exit. With `--fire`, it waits for the carved chunks too |
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
- **Colour.** The material index is a `u16`: the palette index in its low
  byte and, for a voxel that gives off light, its level in bits 8 to 11. The
  texture index mapper passes the palette index on, and a custom material
  (`PaletteMaterial`, with the crate's vertex shader and `src/palette.wesl` as
  fragment shader) looks it up in a 256-colour uniform built from the map's
  palette at runtime. The colour is multiplied by the mesher's ambient
  occlusion and, in the light modes that propagate light, by the face's
  light. The camera has no tonemapping or dithering, so the window shows the
  palette's bytes.
- **Light.** In `propagated`, `both` and `sector-glow` the world turns on the crate's light
  propagation, and the crate wraps `PaletteMaterial` in its light bindings.
  The shader stays unlit: it scales the colour by the face's sky light, the
  lamp light around it and its own glow if it is a lamp, capped at full
  brightness and floored at `--min-light`. Sky light enters columns with no
  solid up to the map top, so it falls into sky sectors only: every other
  column is solid from its ceiling up to the top. Sky fill carries it on
  through windows and doors into the rooms beside the sky. What glows, all
  worked out at import:
  - the bright texels of DOOM's light flats (`CEIL1_2`, `TLITE6_4`,
    `FLOOR1_7` and the rest) and of wall textures whose name holds `LITE`,
    or `BRICKLIT`. Bright means at least 85% of the brightest texel of its
    texture, so a strip light glows and its frame doesn't;
  - light-giving things (candles, candelabras, torches, tech lamps, floor
    lamps, burning barrels), each a 2×2-voxel pillar about its sprite's
    height, a plain stand under a top in the sprite's colour that glows.
    Nothing else of things is drawn.

  A glowing voxel keeps its full-bright palette colour in `both` too, since
  a lamp dimmed by its own sector reads as off. Every source glows at level
  15 by default: light reaches only 32 voxels at most, a fraction of a DOOM
  room, and a lower level only shortens that. `--lamp-level sector` ties
  light flats and walls to their sector's level instead. A shot also lights
  a flash: the voxel in front of the face it hits glows for `--flash-time`
  and goes back to air.
- **Sector glow.** `--light sector-glow` is a hack that stands in for
  seeding open voxels with a base light level, which the crate can't do:
  every surface is a light source. Every solid voxel beside air glows at a
  level standing for the light DOOM draws it in, which is its sector's
  light through the floor, ceiling or wall table, fake contrast included.
  The level of a floor or ceiling comes from its own column's sector, a
  wall's from the sector of the air it faces, a grate's from its side, and
  a pillar's stand from the sector it stands in. Lamps keep level 15, and
  voxels inside the solid don't glow, so a crater shows only the light that
  reaches into it. At a voxel reach of 14 light loses one level per voxel,
  so the face of a surface glowing at level `E` reads `E − 1` from the air
  in front of it. The voxelizer measures how much each COLORMAP table
  darkens the palette, as the mean share of sRGB luma it leaves, picks each
  table's level from that share, and gives the app the brightness each
  level should show. The shader scales the colour's sRGB value, as COLORMAP
  does, rather than its linear one, and only faces of level-15 lamps glow
  as lamps: every other surface shows the light in front of it. Sky light
  is off, since sky sectors glow at their own light. At rest a view looks
  much as in `sector`, and it reacts to the world: light leaks through a
  hole shot between two rooms, crater insides darken inward, and sector
  edges soften over a few voxels.
- **Screenshots.** `--screenshot` waits until every spawned chunk has been
  ready, and `VoxelWorld::light_is_settled` true, for 60 frames in a row with
  no chunk spawning or despawning, or two minutes at most. `--fire` waits
  for the same signal before its first shot, and each shot starts the count
  again. In a lit world each shot prints how long its light took to land.
- **The weapon.** A shot is a ray from the camera along its forward vector,
  through `VoxelWorld::raycast`, out to the view distance. Around the voxel
  it hits, `set_voxels_in` carves a ball whose edge is jittered by a hash of
  each voxel's position, and darkens solid voxels in a shell 1.5 voxels
  thick outside it by a few COLORMAP tables. The darkening starts from the
  colour `VoxelMap::voxel` gives, so shooting the same spot again doesn't
  darken it further, and a scorched lamp goes out. Air is never made solid,
  except for a shot's flash, which rays pass through. The scripted shots of
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
- No things: monsters, items and decorations aren't shown, except the
  pillars that stand in for light-giving things in the light modes.
- Propagated light is white: a red torch lights its alcove white. And it
  reaches 32 voxels at most, 64 map units at 2 units per voxel, so a lamp
  lights a pool beside it and most of a big room stays at `--min-light`.
  `--units-per-voxel 4` doubles the reach in map units at the cost of
  detail.
- `both` multiplies two darkenings, so DOOM's dim rooms come out darker
  than in either `sector` or `propagated`.
- DOOM's light effects (flickering, glowing, strobing sectors) don't
  happen, and sector light levels play no part in `propagated` beyond
  `--lamp-level sector`.
- `sector-glow` reads each face's level from the air in front of it, so a
  face also takes the light of brighter surfaces within a few voxels: the
  floor along a wall drawn brighter than it, or a dark sector beside a
  bright one, comes out a little brighter than DOOM draws it. Levels are
  whole, so a face can be about a twenty-eighth off its sector's light.
- `sector-glow` darkens in proportion, where COLORMAP's dark tables also
  shift hues, so dim rooms keep more of their colour than in DOOM.
- In `sector-glow` light loses a level per voxel, so a leak through a hole
  shot between a bright room and a dark one lights only a few voxels around
  the hole on the dark side, less the thicker the wall. `--voxel-reach 32`
  carries it about twice as far, and lets every bright sector's light
  spread twice as far into its darker neighbours.
- Pillars stand where the thing's centre is, cut off by walls and ceilings,
  and don't block shots any differently from walls.
- With the camera close to a wall only a few chunks spawn, and the light
  beside a lamp in the next chunk can read black until more of the room
  spawns, though `light_is_settled` says it has settled.
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

- **Turning light on for a custom material just worked.** The crate wraps
  `PaletteMaterial` in `VoxelWorldMaterial<PaletteMaterial>`, calls its
  `specialize` first, so the vertex layout and the prepass check carry over,
  and the shader only needed `sample_light`, `sky_brightness` and
  `voxel_brightness` under `VOXEL_LIGHT`.
- **The light module's brightness is made for PBR.** `voxel_brightness`
  multiplies by the look's `voxel_brightness` and the view's exposure, as
  light added to Bevy's emissive channel needs. An unlit shader that wants
  level 15 to read as 1 has to set `voxel_brightness` to one over the
  camera's exposure.
- **Level 0 is always black,** whatever the curve says. With a gentle
  `LevelCurve::exponential`, level 1 is still fairly bright, so the end of
  light's reach shows as a sharp-edged pool. The app builds its own curve,
  shifted to reach 0 at level 0.
- **Emission is per material, from a list.** Saying which voxels glow takes
  a material index wide enough to hold an emissive bit and a level, and a
  list of every emissive material a lookup can return, worked out at import.
  The light task searches that list for each solid voxel whose material
  differs from the one before it, and DOOM's textures change material at
  almost every voxel, so the list has to stay short: it holds only the
  palette colours that can glow. A function from material to emission, or a
  table, would suit a world like this better.
- **Light is one colour and reaches 32 voxels.** Torches can't tint their
  light, and at DOOM's scale no lamp can light a room.
- **No base level for open voxels.** DOOM lights every place by its sector.
  The nearest the crate allows is making every surface a lamp, which needs
  a list of every `(palette colour, level)` pair a surface can show:
  between 700 and 1,600 on the maps tried, searched per solid voxel as
  above. A way for a world to seed open voxels with a level, from a lookup
  like the voxel lookup, would make `sector-glow` a few lines.
- **Emission is in whole levels.** `Emission::level` rounds, and light
  loses levels in steps tied to the reach, so the brightness of a sector's
  light lands on one of 15 steps.
- **The glow of a lamp's own face can't be told apart from its level.**
  The light module adds a lamp face's glow for any emissive voxel. With every
  surface emissive, the app's own uniform tells its shader to glow only
  faces at level 15.
- **The look's brightness scales linear colour.** COLORMAP darkens in
  proportion to sRGB, and the same linear factor darkens a dark colour's
  sRGB value far more than a bright one's. Matching DOOM took scaling in
  sRGB in the app's shader, outside the look's settings.

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
