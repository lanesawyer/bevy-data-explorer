# bevy-data-explorer

A streaming explorer for large scientific datasets, built on
[Bevy](https://bevyengine.org). Datasets are shown side by side in independent
panels, each streaming only what its own view needs and pulling in finer detail
as you zoom. Four formats are supported so far:

- **OME-Zarr** multiscale images, read through
  [`zarrs`](https://crates.io/crates/zarrs), in both Zarr v3 and v2. The
  reference image is 75803 × 56233 px at full resolution; the reference v2
  image is 53718 × 26669 px.
- **Deep Zoom** (`.dzi`) images, the tile pyramids OpenSeadragon reads and
  pathology slides are often published as. Tiles are JPEG or PNG; the
  reference slide is 15936 × 11526 px across 15 levels.
- **Scatterbrain**, the Allen Institute's point-cloud format, in two shapes: a
  single cloud (the reference one holds 4,042,976 points in an octree) and a
  *sectioned* dataset of many slices (53 slices, 3,739,961 points), which gets
  its own panel with grid and single-slice layouts.
- **SVG annotations**: outlines drawn over a slide in its own pixels, with the
  labels the annotation tool wrote on them. The reference document outlines 16
  structures in 46 polygons over the reference Deep Zoom slide.

None of the streamed formats is ever loaded in its entirety. An annotation
document is small enough to be read whole.

## Running

```sh
cargo run --release                          # an empty window, and the examples
cargo run --release -- <url-or-dir>          # any OME-Zarr root
cargo run --release -- metadata.json         # a manifest describing one
cargo run --release -- slide.dzi             # a Deep Zoom image, URL or file
cargo run --release -- --points <url|file>   # a Scatterbrain metadata JSON
cargo run --release -- --slices <url|file>   # a sectioned Scatterbrain JSON
cargo run --release -- --z 3 <source>        # pick a z slice
cargo run --release -- --cache-mb 1024       # a larger tile cache
cargo run --release -- --point-budget 8000000
cargo run --release -- slide.dzi --layer annotation.svg   # draw one over the other
```

Use `--release`. Tile decoding is real work and a debug build makes it obvious.

Nothing is loaded unless it is named. Started with no arguments the window is
empty, and offers every dataset it knows the address of along with a field for
the URL of anything else — a first run that spends a minute fetching reference
datasets nobody asked for is a first run spent waiting. All of them are listed
rather than one per kind, because they differ in more than kind: a Zarr v2 store
against a v3 one, a flat image against a stack of sections, a single cloud
against a sectioned one. Each goes through exactly the path a typed URL does.

The command line is not the only way in either: a dataset can be opened by URL
from the sidebar at any time, without restarting. See **Custom visualization**
below.

Above the collapse control is a button naming the theme it would switch to.
Bevy reports the desktop's preference on the window, and that is where the
theme starts; pressing the button stops it following, since a theme changing
back under you is worse than not following at all. Feathers ships only a dark
theme, so the light one is made by turning the dark one over: near-neutral
tokens have their OKLCH lightness flipped, which keeps each one the same
distance from the background, while colours that carry meaning — the accent,
the axis colours — and the white label text on them stay where they are. It is
a starting point rather than a designed palette. Text is the part worth
knowing about: a button keeps its pale label, because the button under it keeps
its dark background, while a menu item's and a text field's turn over with
theirs. Getting that wrong writes white on white.

Everything the app paints follows it, frames included: what a frame clears to,
the translucent panel its header sits on, the rules between cells, the outline
round the selected one, the range controls and the status lines. The colours
this app names for itself that Feathers has no token for live in one `Palette`,
and leave it two ways — as theme tokens for controls that are spawned once, so
Feathers repaints them, and as a resource read where a colour is chosen while
running, such as a histogram bucket inside or outside the chosen span. What does
not follow is the data: a dataset's own category colours mean what the dataset
says they mean in either theme.

`F12`, or the **log** button beside the theme one, opens a panel across the
bottom holding the last few hundred log records — the same ones that go to the
terminal, kept in memory by a `tracing` layer. It takes its height off the frame
grid rather than covering it, newest line first, warnings and errors in their
own colours. The **copy** button puts the whole log on the clipboard, which is
the point of the panel: a log someone can read is useful, a log they can paste
into a message is what actually comes back.

A sidebar is docked on the left for controls that belong to no single frame —
filters and dataset information, eventually. Drag its edge to resize it between
a readable minimum and half the window, or collapse it to a ribbon with the
toggle; dragging the edge well past the minimum collapses it too, so the edge
does not jam at the minimum with no way to close it. The title shortens to
`BDE` when collapsed.

Point clouds also get a **Cell properties** section, one sub-section per
property. Properties come in two kinds. A categorical one lists its values with
checkboxes, which start clear: a tick picks a value out, and with none ticked
the property filters nothing. Each sub-section offers to clear its own filters
once it has any, and the section's header carries a control naming how many are
applied across all of them; a numeric one draws a histogram of its distribution with a
two-ended control under it, so a span is chosen against the shape of the data
rather than blind. Buckets outside the chosen span are dimmed rather than
hidden, keeping the whole distribution in view. The button in a sub-section's header colours points by
that property; the checkboxes inside filter points down to the values still
ticked, and excluded points are dropped as a node is built rather than hidden
afterwards, so they cost no vertices.

The properties themselves are a plain component on the source entity, so
nothing that reads them knows where they came from. They are built from the
dataset's own column list today — real names and identifiers, so colouring
works against live data, with placeholder value labels. A lookup against
whatever service knows the real ones replaces that by writing the same
component.

No numeric property is offered yet, so the range control above describes
something you will not currently see. A categorical placeholder invents only
the labels, over codes that are the dataset's own; a numeric one would have to
invent the bounds and the whole histogram, and a span chosen against that
filters by numbers that came from nowhere. The control, the filtering and their
tests all stay — what is missing is the measurement, not the code.

The sidebar holds accordions. They are generic containers — a title, an open
flag, and whatever children a caller hangs off the body — because which
sections appear will vary with the dataset. The first is **View
configuration**, which acts on the *selected* frame: the one outlined in blue,
which follows whichever frame you last clicked or dragged in. It names the
dataset and carries a transparency slider. With no frame selected it says so
and shows nothing else — a slider with nothing to act on invites a drag that
changes nothing.

Every keyboard shortcut acts on that same frame and no other: paging a stack,
stepping sections, toggling a channel, resetting a view. A key that reached
every open dataset would page or toggle the one nobody was looking at, and with
the pointer parked over the sidebar there would be nothing to say which frame it
had been talking to. The pointer still decides what a drag or a wheel applies
to, since those say where they mean. The outline appears only once there is more
than one frame — drawn round the whole grid it reads as a border on the window
rather than as an answer to a question nobody asked.

Its menu is **Edit layout**: the frames that are open, and under **Open a
dataset** every dataset the app knows the address of. That second list is what
fills the grid without pasting a URL. One that has not been opened yet is
fetched; one that has — including one whose frames have all been closed — gets
a frame onto the source it already is, rather than being downloaded again to
arrive at the same dataset twice. A catalogue served over HTTP will fill that
list later.

Point clouds draw each point as a screen-space quad rather than with point
topology, because the hardware fixes point primitives at one pixel and offers
no way to size them. The four vertices of a point share its position and carry
a unit corner offset that the shader expands in clip space, so the size is a
uniform: the point size control costs nothing to drag and never rebuilds a
mesh. Sizing is offered only for sources that advertise it, so an image never
shows a control that means nothing to it.

Fading dims the colour rather than lowering alpha. Alpha compounds with
overdraw — a dense point cloud stacks dozens of points on a single pixel, and
`1 - (1 - a)^n` is already 99% by eight layers, so the sectioned data looked
untouched until the slider was near zero. Dimming fades a layer uniformly
however many times it overdraws, and against a dark background looks the same
as a single transparent layer. The factor is applied in sRGB so halfway along
the slider looks half as bright. The tradeoff is that it is a dim rather than a
true see-through, which will matter once two sources share one frame.

Transparency is stored per source, so two frames showing different datasets
fade independently and selecting one loads its own value into the slider rather
than carrying the previous one across. It is applied by render layer rather
than by asking each format plugin to implement it, so a new format fades
without any code written for it — including tiles and octree nodes that stream
in after the value was set.

Widgets come from **Feathers**, Bevy 0.19's widget collection: the slider,
buttons and labels are Feathers controls, themed from its dark theme, so they
match rather than being hand-styled one at a time.

The app starts maximized, since several frames beside a sidebar need the room.

Each accordion can carry a menu button on the right that opens a popup. A menu
is capped at the bottom of the window and scrolls once its contents no longer
fit. View
configuration's is an **Edit layout** menu listing every frame with its dataset
name, provenance and headline figure, and buttons to clone or close it, over the
list of datasets to open. Both that menu and a frame's own corner buttons raise
the same `PanelRequest`, so the rules about what may be opened or closed live in
one place and the two routes cannot drift apart.

At the bottom of that menu is **Custom visualization**: a text field for the
URL of a dataset that was not named on the command line. Nothing asks which
format it is — the URL is read and the format worked out from what comes back,
so an OME-Zarr store, a single point cloud and a sectioned dataset are all
pasted into the same field. A `.json` is tried as Scatterbrain metadata first
and as an image manifest second; anything else is tried as a Zarr store. A
sectioned dataset is recognised by its metadata listing more than one slide,
which is the same distinction `--points` and `--slices` make by hand.

Whatever is recognised is registered as a source like any other and opens into
a new frame, so it is indistinguishable afterwards from one named on the
command line — it appears in the list above, carries its own transparency and
point size, and offers its cell properties in the sidebar. A URL that matches
nothing leaves the reason under the field, naming what was tried rather than
failing silently. The read is a blocking fetch and parse, so it runs on a task
and the window keeps drawing while it is in flight; the field takes one at a
time.

A text field takes the keyboard while it has focus, so the frame shortcuts —
`r`, the digits, the arrows — stand down for as long as something is being
typed into. That is settled once a frame, before anything reads a key, because
the shortcuts are spread across the grid and two format plugins. Dismissing the
menu hands the keyboard back, since a closed menu is only hidden and a field
inside one would otherwise go on swallowing keystrokes with nothing on screen
to show where they were going.

An inspector docks on the right, opened from a frame's info button and closed
from its own. It follows the selection rather than pinning itself to the frame
that opened it, so clicking between frames retells what each one is showing. It
resizes the same way as the sidebar, dragging from its inner edge.

Neither dock talks to the grid. It takes a slice off a `FrameArea`,
which is the only thing frames and their chrome measure against, so none of
that code knows it exists. Chrome that can overlap the grid — the drag handle
straddles its own edge — is marked `BlocksFrameInput`, since position alone
cannot decide whether a drag belongs to the handle or the frame beneath it.

Panels are laid out in a grid of up to four columns by two rows — eight in
all. One or two panels sit in a single row; beyond that the grid goes two deep
and grows sideways, which keeps cells closer to square than a single row of
eight would. Input goes to whichever panel the pointer is over, so the views
pan and zoom independently.

Each frame's header carries `i`, which opens the inspector on it, and `png`,
which saves a picture of it. The picture is the frame's own viewport cut out of
a window screenshot, at the size it is on screen, with that frame's chrome
hidden for the shot so the header, its buttons and the selection outline are not
burnt into it. Where the frame drew nothing the picture is transparent: the
window's own alpha carries brightness rather than opacity when HDR is on, so the
colour the frame clears to is keyed out instead — which cuts hard, leaving a
dark fringe on antialiased edges. Files go to `screenshots/`, named after the
dataset and the moment, and the frame says where its last one went. The encode
runs on a task, since several megapixels of PNG is enough work to freeze the
window if done where the pixels arrive.

The `+` in a panel's corner duplicates it, and the `x` closes it. Closing
renumbers the remaining frames so the grid stays contiguous. The last frame can
be closed too: the window returns to the empty state it started in, examples and
all.

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
| hover | identify what is under the pointer, and enlarge the cells sharing its value |
| drag | pan the panel under the cursor |
| scroll | zoom that panel about the cursor |
| `R` | reset that panel's view |
| `+` button | duplicate that panel |
| `x` button | close that panel |
| `i` button | open the inspector on that frame |
| frame menu | show a different dataset in that frame |
| drag sidebar edge | resize the sidebar, or collapse it — the pointer becomes a resize cursor over the handle |
| `<` / `>` button | collapse or expand the sidebar |
| `1`–`9` | toggle an image channel |
| `G` | sections panel: grid ↔ single slice |
| `←` `→`, `[` `]` | step through slices |

Each frame carries a header in its top corner — the dataset's name, a button
that opens the inspector on it, and a menu for pointing that frame at a
different dataset. Frames are drawn over imagery that is bright in places and
black in others, so the header and the status beneath it sit on a translucent
panel. The frame's own controls, duplicate and close, stay in the opposite
corner: what manages the frame is kept apart from what describes the data.

Pointing a frame at another dataset is what the source indirection was for. The
camera moves onto that source's render layer and is reframed to its extent;
nothing about the format is involved.

Each panel carries its own overlay: the image reports the pyramid level, scale
and tile cache; the point cloud reports octree depth, nodes loaded and points
resident.

Hovering a frame shows what is under the pointer in its bottom corner. The
sections and point-cloud panels name the cell — Scatterbrain gives a point no
id of its own, so its address is the octree node plus its offset within that
node's columns, and for a sectioned dataset the slice as well — along with its
value in whatever property the points are coloured by. The image has no cells
to name, so it reports the place instead: the full-resolution pixel, the
pyramid level being drawn, and the tile that covers it.

Hovering also **enlarges every other cell sharing that value**, which is what
turns a colour into something you can trace through a dense cloud. It costs no
geometry: each vertex already had room to carry its point's category alongside
its corner, so the highlight is a uniform naming one category and the shader
draws those points larger. Nothing is rebuilt, refetched or re-uploaded beyond
a few bytes per resident node, so it keeps up with the pointer over millions of
points. With no property selected there are no groups to pick out, and the
tooltip still names the cell.

## Layers

A frame can draw several datasets, one over another: annotations over the slide
they were drawn on, a point cloud over the image it was measured from. The
dataset a frame opened onto is the bottom of its stack, and anything open can
be put on top — from the frame's own `...` menu (`+ layer`, `- layer`), from the
**Layers** section of the sidebar, or with `--layer` on the command line, which
stacks onto the first frame and can be repeated.

Nothing refuses a layer for what it is. Whether two datasets mean anything drawn
together is the judgement of whoever is looking, and a viewer that refuses is a
viewer people take screenshots from to overlay by hand. What a layer is *not* is
rescaled: it is drawn in its own coordinates, so two datasets line up when they
were measured alike. One whose unit differs from the frame's is still layered,
and marked — `um over px, not rescaled` — so a coincidental alignment is not
mistaken for a real one.

The Layers section lists the selected frame's stack top first, each with its
own transparency and a button to take it off, over the list of everything else
that could go on top. Transparency is the source's own, the same value View
configuration sets, so a dataset fades the same way whether it is a layer or a
frame. Outlines lower their alpha rather than dimming, since they barely
overdraw and should let a pale slide show through.

Each layer is a camera of its own, sharing its frame's viewport, view and
projection, drawing its source's render layer and ordered straight after the
one beneath it. Stacking by camera order rather than by depth is what lets any
source sit on any other without its plugin knowing; one source can be the base
of one frame and a layer of another at once. A layer camera carries the same
`ShowsSource` a frame does, which is all a streamer asks of a view, so a layered
dataset streams exactly as it would in a frame of its own. Closing a frame
despawns its layers, duplicating one copies them, and pointing a frame at a
dataset already in its stack drops that layer rather than drawing it twice.

Hovering asks every source in the stack what is under the pointer, and the
tooltip lists the answers topmost first — over a slide with annotations, the
structure and region first, then the pixel.

A stack holds at most eight datasets, which is the band of camera orders each
cell is given.

## Releases

Pushing a `v*` tag builds Linux, Windows and a universal macOS binary and
publishes them as a GitHub release. `workflow_dispatch` rehearses the same
build against an existing tag and leaves the release as a draft.

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


Every format is a Bevy plugin, and a plugin's *systems* and its *datasets* are
registered separately. The systems go in once whether or not the command line
named anything of that format, which is what lets a URL typed in later open
into the same machinery — including a format that was switched off at startup.
Registering a dataset takes the world rather than the `App`, so the same call
serves one opened while the plugins are being built and one opened after the
window is up.

A registered dataset is a *source entity* carrying the few things a frame needs
to know about what it is showing:
a name, the render layer its geometry is drawn on, how much world it occupies,
and a line of status for the overlay. The plugin then inserts its own streamer
and systems.

Hovering works the same way, and is the second half of that surface. The grid
knows where the pointer is and which frame it is in; only a plugin knows what
lives there. So the two meet halfway: the grid writes a `HoverProbe` onto the
source entity of whichever frame the pointer is over, each plugin answers with
a `HoverInfo`, and the frame's tooltip draws whatever came back. A plugin that
cannot answer simply never writes one, and its frames show no tooltip. At most
one source carries a probe at a time, so a plugin resolving hover never has to
work out whether the pointer is really over its own frame.

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

`zarrs` handles the format: Zarr v3 and v2 metadata, the codec pipeline, and
partial reads of sharded arrays. `ome_zarr_metadata` parses the multiscale and
channel metadata. This crate adds the pyramid mapped into world space, tile
streaming, caching and the camera.

Which Zarr version a store is written in never reaches this crate: `zarrs` reads
`zarr.json` where there is one and `.zgroup`/`.zarray` where there is not, and
presents either as the same array. What does reach it is sharding, which only v3
has. A sharded level is one object per shard, so a tile is cut from a shard
through a decoder that holds its index, and successive tiles from that shard
cost no further round trips. An unsharded level is one object per chunk, so a
tile spans several chunks and is read straight from the array, which fetches
them together: against the reference v2 image a 512px tile read that way took
159ms, where the same region as sixteen per-chunk reads took 1.1s.

Reading is driven by what is on screen. Each frame the viewer picks the level
whose pixels are closest to screen pixels, then requests the single overview
tile at the coarsest level and the tiles covering the viewport at the chosen
one — and nothing else. A tile off screen shares the connection with the ones
on it, so a margin around the view and the levels either side of the chosen
one are fetched only once every visible tile has landed, and are abandoned the
moment the view moves. Coarser tiles already resident stay drawn underneath,
so a zoom shows a blurry version immediately that sharpens as finer tiles land.
The same holds for Deep Zoom images.

A volumetric image — a specimen cut into sections — is paged through rather
than shown all at once. The source advertises a stack of slices, and both the
frame keys — the arrows, `[` and `]`, or `PageUp`/`PageDown`, the same ones
the sectioned panel uses — and the slider in **View configuration** move it; paging acts on the selected frame, so two specimens
open side by side are paged one at a time. Nothing in the sidebar knows an image
is what it is paging: the stack is a component on the source, the way a point
size is, and any format that has one can offer it. A stack opens on its middle
slice, since the first section of a block is usually empty and opening on it
reads as a dataset that failed to load; `--z` names one instead.

Reads are asynchronous, which is what lets one be given up on. A tile the view
has moved off has its slot dropped, and dropping the slot aborts the read behind
it: panning across the reference stack for six seconds abandoned 93 tile reads,
each of them up to 37 MB of chunk fetched and decoded for a view nobody was
looking at any more. The same applies to a view that leaves the image
altogether, which wants nothing and should therefore be reading nothing.

What that cost is the chunk cache. zarrs' decoded-chunk cache is synchronous —
`// TODO: AsyncChunkCache` upstream — so paging a stack now reads its chunks
again rather than finding the slice beside the last one already decoded, which
measured 600ms against 47ms before. The fix that fits this viewer is to keep the
whole z-block a tile decodes rather than one slice of it: the chunks contain
forty slices whatever we ask for, so reading the block costs what reading one
slice already costs, and paging within it costs nothing.

Addresses are taken as they were copied. Neuroglancer writes the format in front
and names buckets its own way — `zarr2://s3://bucket/key` — so the prefix is
dropped, since the bytes decide what a source is anyway, and the bucket becomes
the URL it is served from.

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

### SVG annotations

Not an SVG renderer. Annotation tools export regions as polygons in the pixel
space of the slide they were drawn on, with labels in attributes of their own
(`structure-label`, `region-label`), and that is what is read: `polygon`,
`polyline`, `line`, `rect` and straight-segment `path`s, their stroke colour,
width and dash, and every `*-label` attribute. Fills, filters and text are not
drawn; an outline with a fill and no stroke is outlined in its fill, since it
would otherwise be an annotation nobody can see. A `viewBox` is scaled into the
declared size.

Anything that would put an outline somewhere other than where its points say —
a `transform`, or a curve in a path — is skipped and counted in the frame's
status rather than drawn in the wrong place.

Outlines are one mesh, a quad per segment, widened in the shader to the wider of
the stroke as written and two screen pixels. A ten-pixel stroke over a
sixteen-thousand-pixel slide is a hundredth of a pixel at the overview, so
without the floor the annotations vanish exactly when they are most useful for
finding your way; zoomed in, they are drawn at their true width. Dashes are cut
from distance along the outline the same way, so the dashed black outline the
reference document draws over each excluded region stays dashed at any zoom.
None of it is rebuilt as the view moves, and two frames at different zooms draw
the same mesh correctly.

Hovering names the outline whose stroke is under the pointer, or else the
innermost region the pointer is inside.

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
- **Resident points are kept on the CPU as well as on the GPU.** A mesh cannot
  be read back, so hit-testing the pointer needs the coordinates themselves.
  That is ten bytes a point against the eighty each already costs as vertices,
  and only the octree nodes whose regions actually reach the pointer are
  searched — the path from the root down, rather than everything on screen.
- **Tiles are cached well past leaving the viewport.** Zooming in narrows the
  wanted set to a handful of fine tiles; the surrounding coarse ones are
  exactly what is needed again on the way back out. They are evicted
  least-recently-wanted, under a memory budget, rather than on sight.

### Interoperability notes

- OME-NGFF specifies omero channel colours as six bare hex digits, and
  `ome_zarr_metadata` enforces that. Real converters write `#RRGGBB`, and
  sometimes the CSS shorthand `#0df`. Both are normalised rather than rejected.
- An axis is a name, a type and a unit. The reference v2 image also writes a
  `scale` on every axis — the same number its `coordinateTransformations`
  already carries — and the metadata crate refuses the whole document over it.
  Undefined axis fields are dropped before parsing.
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
- A layer is drawn in its own coordinates. There is no registration between
  two datasets yet, so an overlay measured differently from its frame is
  marked but not moved into place.
- An annotation example can only be layered once it has been opened; nothing
  can say which frame a document belongs over until it has been read.
- A custom dataset is opened into a frame but cannot be closed again as a
  *source*: closing its frame leaves the source registered, still holding
  whatever it has streamed, and its render layer is not handed back.
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

## Continuous integration

Every pull request runs formatting, a build of all targets and the test suite,
with `RUSTFLAGS=-D warnings` so the rule this repo already had — a clean build
is the bar — is enforced rather than remembered. Bevy links against the
windowing and input libraries even for a build that never opens a window, so
the workflow installs those; audio is not in the feature list, so ALSA is not
among them.

The windowed smoke run is not in CI. It needs a GPU and a display to prove
anything, and a run that cannot fail for the reasons that matter is a run that
teaches you to ignore it. It stays a local step, as `CLAUDE.md` describes.

Clippy is not in CI either, for now: the tree has around forty-five warnings,
mostly argument counts on systems and complex query types, and turning it on
would mean either failing every pull request or ignoring the job. Clearing them
is worth its own change, and the job can go in with it.

Dependabot opens weekly pull requests for the crates and for the actions in the
workflow. Patch and minor crate updates are grouped into one, so a quiet week is
one pull request rather than nine; majors arrive on their own, since those are
the ones worth reading.
