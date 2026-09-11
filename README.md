# bevy-data-explorer

A streaming OME-Zarr viewer built on [Bevy](https://bevyengine.org) and
[`zarrs`](https://crates.io/crates/zarrs). It opens a multiscale image straight
from an object store and lets you pan and zoom around it, pulling in finer
detail as you go — the full-resolution level of the reference image is
75803 × 56233 px, so nothing is ever loaded in its entirety.

## Running

```sh
cargo run --release                      # the default reference image
cargo run --release -- <url-or-dir>      # any OME-Zarr root
cargo run --release -- metadata.json     # a manifest describing one
cargo run --release -- --z 3 <source>    # pick a z slice
cargo run --release -- --cache-mb 1024   # a larger tile cache
```

Use `--release`. Tile decoding is real work and a debug build makes it obvious.

| input | action |
| --- | --- |
| drag | pan |
| scroll | zoom about the cursor |
| `R` | reset the view |
| `1`–`9` | toggle a channel |

The overlay reports the pyramid level in use, the scale in physical units, and
how much of the tile cache is resident.

## How it works

`zarrs` handles the format: Zarr v3 metadata, the codec pipeline, and partial
reads of sharded arrays. `ome_zarr_metadata` parses the multiscale and channel
metadata. This crate is the viewer on top — the pyramid mapped into a shared
world coordinate system, tile streaming, caching and the camera.

Reading is driven by what is on screen. Each frame the viewer picks the level
whose pixels are closest to screen pixels, then requests the tiles covering the
viewport at that level and at every coarser one. Coarse tiles are few and
arrive first, and are drawn underneath, so moving into new territory shows a
blurry version immediately that sharpens as finer tiles land.

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

- Channel toggling recomputes tiles, because the composite is baked into RGBA
  on the CPU. Interactive window/level adjustment wants a shader instead.
- Reads are synchronous, so a request already under way cannot be abandoned.
  Queued work is dropped when the view moves on, which is where a backlog
  actually builds up during a fast pan.
- Only the first multiscale image in a store is shown, at a single z slice.
