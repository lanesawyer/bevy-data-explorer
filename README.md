# bevy-data-explorer

A streaming explorer for large scientific datasets, built on
[Bevy](https://bevyengine.org). Datasets are shown side by side in independent
panels, each streaming only what its own view needs and pulling in finer detail
as you zoom. Two formats are supported so far:

- **OME-Zarr** multiscale images, read through
  [`zarrs`](https://crates.io/crates/zarrs). The reference image is
  75803 × 56233 px at full resolution.
- **Scatterbrain**, the Allen Institute's point-cloud format, in two shapes: a
  single cloud (the reference one holds 4,042,976 points in an octree) and a
  *sectioned* dataset of many slices (53 slices, 3,739,961 points), which gets
  its own panel with grid and single-slice layouts.

Neither is ever loaded in its entirety.

## Running

```sh
cargo run --release                          # both reference datasets
cargo run --release -- <url-or-dir>          # any OME-Zarr root
cargo run --release -- metadata.json         # a manifest describing one
cargo run --release -- --points <url|file>   # a Scatterbrain metadata JSON
cargo run --release -- --slices <url|file>   # a sectioned Scatterbrain JSON
cargo run --release -- --points none --slices none   # image panel only
cargo run --release -- --z 3 <source>        # pick a z slice
cargo run --release -- --cache-mb 1024       # a larger tile cache
cargo run --release -- --point-budget 8000000
```

Use `--release`. Tile decoding is real work and a debug build makes it obvious.

Panels are laid out in a grid of up to four columns by two rows — eight in
all. One or two panels sit in a single row; beyond that the grid goes two deep
and grows sideways, which keeps cells closer to square than a single row of
eight would. Input goes to whichever panel the pointer is over, so the views
pan and zoom independently.

The `+` in a panel's corner duplicates it, and the `x` closes it. Closing
renumbers the remaining frames so the grid stays contiguous, and the last frame
cannot be closed — an empty window offers no way back.

Duplicating: The copy inherits the source's
current centre and zoom rather than its fitted defaults, so it starts as the
same view and can then be driven somewhere else — useful for watching an
overview and a detail of the same data at once. Duplicates cost no extra
geometry: they are another camera on the same render layer, drawing the same
entities from a different viewpoint. Streaming follows the union of what every
panel needs, so a duplicate pulls in its own detail rather than riding on
whatever the original happened to load.

| input | action |
| --- | --- |
| drag | pan the panel under the cursor |
| scroll | zoom that panel about the cursor |
| `R` | reset that panel's view |
| `+` button | duplicate that panel |
| `x` button | close that panel |
| `1`–`9` | toggle an image channel |
| `G` | sections panel: grid ↔ single slice |
| `←` `→`, `[` `]` | step through slices |

Each panel carries its own overlay: the image reports the pyramid level, scale
and tile cache; the point cloud reports octree depth, nodes loaded and points
resident.

## Architecture

Frames, their overlays and their chrome are declared with **BSN** (Bevy Scene
Notation), the scene system introduced in Bevy 0.19. Spawning is a scene patch
rather than a component tuple, so shared chrome is a scene function that each
button layers its own patch over:

```rust
fn button_chrome() -> impl Scene {
    bsn! { Button Node { width: { Val::Px(BUTTON_PX) }, .. } BackgroundColor({ IDLE_BUTTON }) }
}

commands.spawn_scene(bsn! {
    button_chrome()
    PanelButton { panel: { panel }, action: { action } }
    Children [( Text({ action.glyph().to_string() }) .. )]
});
```

Components patched this way need `Default + Clone`, since a scene writes its
fields over defaults. Components that keep private state — `RenderLayers`,
`Projection` — cannot be patched field by field and are supplied whole with
`template_value`.

The per-tile and per-node geometry spawned by the streamers stays a plain
component tuple. Those run every frame as data arrives, where a scene buys
nothing over a bundle.


Every format is a Bevy plugin. On build it registers itself as a *source
entity* carrying the few things a frame needs to know about what it is showing:
a name, the render layer its geometry is drawn on, how much world it occupies,
and a line of status for the overlay. The plugin then inserts its own streamer
and systems.

Frames refer to a source by entity rather than by a format tag, which is what
keeps `panel` and `hud` from knowing anything about OME-Zarr or Scatterbrain —
the overlay reads a name and a status string off whichever entity its panel
points at. Adding a format means writing a plugin and adding it; nothing else
changes, and `main` discovers sources from the world rather than listing them.

Render layers are allocated at registration, one per source, so two sources can
never draw into each other's frames. Layer 0 is deliberately never handed out:
anything spawned without an explicit layer lands there and would appear
everywhere.

Swapping what a frame displays is then a component write plus a layer change on
its camera, which is the point of the indirection.

## How it works

Each panel is a 2D camera with its own viewport, pan/zoom state and render
layer, so one panel's contents cannot leak into another.

### OME-Zarr images

`zarrs` handles the format: Zarr v3 metadata, the codec pipeline, and partial
reads of sharded arrays. `ome_zarr_metadata` parses the multiscale and channel
metadata. This crate adds the pyramid mapped into world space, tile streaming,
caching and the camera.

Reading is driven by what is on screen. Each frame the viewer picks the level
whose pixels are closest to screen pixels, then requests the tiles covering the
viewport at that level and at every coarser one. Coarse tiles are few and
arrive first, and are drawn underneath, so moving into new territory shows a
blurry version immediately that sharpens as finer tiles land.

### Scatterbrain point clouds

A Potree-style octree of 2D points. Columns are stored one per directory, split
by node: `{metadata}/{column}/{referenceId}/{node}.bin`. Coordinates are raw
little-endian `f32` pairs with no header and categorical columns are raw `u16`,
so a file's length is exactly the node's point count times the column stride —
the cheapest possible integrity check, and one the reader enforces.

The format is *additive*: a node holds its own subsample of its region and its
children add further points, which is why the node counts in the tree sum to
the dataset total rather than each level restating the whole cloud. Selection
walks down from the root keeping any node that is on screen, and descends while
that node's region is still large enough in screen terms to be worth more
detail. Every node visited is drawn, so zooming in genuinely increases point
density rather than swapping one level for another.

Child indices pack one bit per axis — `x` in bit 2, `y` in bit 1, `z` in bit 0.
This data is planar, so the z bit is always clear and only the even indices 0,
2, 4 and 6 appear, which is why the node names look like they skip numbers.

### Sectioned datasets

The same format also describes a specimen cut into slices: instead of one tree
at the top level, the metadata carries a `slides` list, each with its own
octree. All slides share one reference id and one coordinate system — the slide
index is encoded in the node file name (`s13r6.bin`) rather than in the path.
Both shapes are modelled as a list of slides so the rest of the viewer does not
have to know which it opened.

Because the slices share a coordinate system they would otherwise pile up, so
each is re-centred on its own bounds and given a layout offset. Slices differ
in size, so the grid uses a cell sized to the largest of them, which keeps the
anatomy aligned from one row to the next rather than drifting. The offset lives
in each node's transform, so switching between grid and single-slice layouts
only rewrites transforms and visibility — nothing is refetched.

### Things that were measured rather than assumed

The reference store shards a 4096 × 4096 region into a 32 × 32 grid of 128 px
inner chunks, compressed with blosc over zstd. A few decisions came out of
measuring against it, and are worth knowing before changing them:

- **Tiles are 512 px, not one inner chunk.** The HTTP store batches the byte
  ranges of the inner chunks behind a single request, so covering a region
  costs the same number of bytes whatever the tile size — but ten times the
  wall clock at 128 px versus 1024 px, purely in round trips.
- **Shard decoders are cached.** Reading a tile with `retrieve_array_subset`
  re-fetches the 16 KB shard index every time. Holding a `partial_decoder` per
  shard drops a tile from two reads to one.
- **Tile threads are added on top of the core count.** A tile read is blocking
  from end to end, so one tile occupies one thread. Bevy's default
  async-compute pool caps at four threads, which allows about three concurrent
  requests. These threads are blocked on sockets rather than using CPU, so
  carving them out of the core count would starve the ECS schedule for nothing.
- **Tiles are cached well past leaving the viewport.** Zooming in narrows the
  wanted set to a handful of fine tiles; the surrounding coarse ones are
  exactly what is needed again on the way back out. They are evicted
  least-recently-wanted, under a memory budget, rather than on sight.

### Interoperability notes

- OME-NGFF specifies omero channel colours as six bare hex digits, and
  `ome_zarr_metadata` enforces that. Real converters write `#RRGGBB`, and
  sometimes the CSS shorthand `#0df`. Both are normalised rather than rejected.
- `zarrs_http` joins keys onto the base URL with an unconditional `/`, so a
  root that already ends in one produces `...zarr//zarr.json` — a different,
  missing key on an object store. Store roots are trimmed, since manifests
  conventionally carry the trailing slash.

## Limits

- Points are drawn with `PointList` topology, one vertex each, which keeps a
  multi-million point cloud affordable but means the hardware draws each as a
  single pixel. Point sizing needs a custom shader.
- Panels can be duplicated and closed but not reordered or resized, and cells
  are a uniform split. A partly filled grid — three panels in a 2x2 — leaves an
  empty cell rather than redistributing the space.
- A frame cannot yet be repointed at a different source from the UI. The
  indirection that would allow it is in place, but nothing drives it.
- Each streamer is a resource, so a format supports one open dataset at a time.
  Two OME-Zarr images side by side would need the streamer to move onto the
  source entity as a component.
- The sections panel always draws slices in metadata order. It carries no
  notion of anatomical position, so the grid is a contact sheet rather than a
  reconstruction.
- Channel toggling recomputes tiles, because the composite is baked into RGBA
  on the CPU. Interactive window/level adjustment wants a shader instead.
- Reads are synchronous, so a request already under way cannot be abandoned.
  Queued work is dropped when the view moves on, which is where a backlog
  actually builds up during a fast pan.
- Only the first multiscale image in a store is shown, at a single z slice.
- Point colouring is fixed to the first categorical column; the others are
  parsed but not yet selectable.
