# bevy-data-explorer

A streaming explorer for large scientific datasets, built on
[Bevy](https://bevyengine.org). Datasets are shown side by side in independent
panels, each streaming only what its own view needs and pulling in finer detail
as you zoom. Five formats are supported so far:

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
- **CSV and TSV tables**: delimited text shown as a table filling its frame,
  with a header that stays put and scrollbars, for the metadata that comes
  alongside the imagery — gene panels, region lists, cell annotations. Read
  whole, but only the rows on screen are built.
- **Parquet tables**: shown the same way. The reference one is the Allen adult
  mouse terminology: 1,332 brain structures across 10 columns.
- **Brain Knowledge Platform specimens**: one project's specimen records,
  asked of the platform's GraphQL API and shown as the same table. Every
  project whose specimens can be tabulated is offered in the dataset
  dropdown — seven of the platform's 172, from 38 specimens to 10,901. The
  reference one is the SEA-AD donor metadata: 84 donors across 32 columns of
  clinical and neuropathology features.

None of the streamed formats is ever loaded in its entirety. An annotation
document and a table are small enough to be read whole.

## Running

```sh
cargo run --release                          # an empty window, and the examples
cargo run --release -- <url-or-dir>          # any OME-Zarr root
cargo run --release -- metadata.json         # a manifest describing one
cargo run --release -- slide.dzi             # a Deep Zoom image, URL or file
cargo run --release -- genes.csv             # a CSV or TSV table, URL or file
cargo run --release -- terms.parquet         # a Parquet table, likewise
cargo run --release -- '<bkp-endpoint>/?specimens=<project>'  # a specimen table
cargo run --release -- --points <url|file>   # a Scatterbrain metadata JSON
cargo run --release -- --slices <url|file>   # a sectioned Scatterbrain JSON
cargo run --release -- --z 3 <source>        # pick a z slice
cargo run --release -- --cache-mb 1024       # a larger tile cache
cargo run --release -- --point-budget 8000000
cargo run --release -- slide.dzi --layer annotation.svg   # draw one over the other
cargo run --release -- --bookmark saved.json # a bookmark, as a file or a bde1: line
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
at any time, without restarting, by pasting it into a new frame's search. See
**New frame** below.

Above the collapse control is a button naming the theme it would switch to.
Bevy reports the desktop's preference on the window, and that is where the
theme starts; pressing the button stops it following, since a theme changing
back under you is worse than not following at all. Feathers ships only a dark
theme, so the light one is made by turning the dark one over: near-neutral
tokens have their OKLCH lightness flipped, which keeps each one the same
distance from the background, while colors that carry meaning — the accent,
the axis colors — and the white label text on them stay where they are. It is
a starting point rather than a designed palette. Text is the part worth
knowing about: a button keeps its pale label, because the button under it keeps
its dark background, while a menu item's and a text field's turn over with
theirs. Getting that wrong writes white on white.

Everything the app paints follows it, frames included: what a frame clears to,
the translucent panel its header sits on, the rules between cells, the outline
round the selected one, the range controls and the status lines. The colors
this app names for itself that Feathers has no token for live in one `Palette`,
and leave it two ways — as theme tokens for controls that are spawned once, so
Feathers repaints them, and as a resource read where a color is chosen while
running, such as a histogram bucket inside or outside the chosen span. What does
not follow is the data: a dataset's own category colors mean what the dataset
says they mean in either theme.

`F12`, or the log button (a scroll) beside the theme one, opens a panel across the
bottom holding the last few hundred log records — the same ones that go to the
terminal, kept in memory by a `tracing` layer. It takes its height off the frame
grid rather than covering it, newest line first, warnings and errors in their
own colors. Drag its top edge to make it taller, up to three quarters of the
window; it opens at 240px again each run. The **copy** button puts the whole log on the clipboard, which is
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
hidden, keeping the whole distribution in view. The button in a sub-section's header colors points by
that property; the checkboxes inside filter points down to the values still
ticked, and excluded points are dropped as a node is built rather than hidden
afterwards, so they cost no vertices.

The properties themselves are a plain component on the source entity, so
nothing that reads them knows where they came from. A format builds them from
the dataset's own column list — real names and identifiers, so coloring works
against live data, with placeholder value labels, since the files hold codes
and not what they stand for.

A catalog entry can name a service that knows better, and a source whose
address a catalog lists is described by it however it was opened. The Brain
Knowledge Platform does this for every dataset it lists: its GraphQL API gives
the properties the portal shows and in what order, which of them are levels of
one taxonomy, the property the portal colors
by first, each value's label and color, and each numeric column's extent and
histogram. Points are then drawn in the platform's colors, and each value
carries a swatch. How many cells hold each value takes the API seconds to
count, so the counts are asked for once the labels are showing and written in
beside them when they land. A dataset any catalog lists also takes the catalog's
name for it in place of the reference id its address gives. The built-in
examples keep what their files say about their cells, even where
the platform also knows them, because no catalog vouches for their addresses.

A taxonomy, or an atlas of regions within regions, is one property drawn as a
tree: each value nests under its parent a level up, and a node expands to show
its children, which are only built when it is opened. Ticking a node admits
every cell under it at whatever level it sits. Ticks are kept minimal —
ticking every child of a node ticks the node, and unticking one child of a
ticked node splits the tick among its siblings — so what is admitted is always
what the boxes show, and a dot marks a collapsed node with ticks inside it. It
all reduces to a set of codes in the finest level's column, which is the only
column the streamers read to filter. The color button in a tree's header opens
a menu of its levels, and the one chosen is what the points are colored by.

Numeric properties are offered only when a service gives their extent and
histogram. A categorical placeholder invents only the labels, over codes that
are the dataset's own; a numeric one would have to invent the bounds and the
whole histogram, and a span chosen against that filters by numbers that came
from nowhere. Points are colored by code, so a numeric property filters but
does not color.

The sidebar holds accordions. They are generic containers — a title, an open
flag, and whatever children a caller hangs off the body — because which
sections appear will vary with the dataset. The first is **View
configuration**, which acts on the *selected* frame: the one outlined in blue,
which follows whichever frame you last clicked or dragged in. It names the
dataset and carries a transparency slider. With no frame selected it says so
and shows nothing else — a slider with nothing to act on invites a drag that
changes nothing.

A dataset that mixes channels into color — a multichannel image, where each
channel is painted in its own color and the colors added — also gets a row
per channel there: its color, a box to show or hide it, and a brightness
slider. Brightness is a percentage of how the dataset publishes the channel,
since a raw intensity window like 0–1377 out of 65535 means nothing at a
glance: 100 is as published, 200 reaches full brightness at half the
intensity, and 0 is dark. The rows read and write a `SourceChannels` component
on the source rather than anything of the image's, so the number keys, which
toggle the same channels, and the boxes always agree, and a format that
composites channels some other way offers the same controls by carrying one.
Once anything differs from how the dataset publishes it, a **reset** button
beside the heading puts every channel back, sliders included.

Every change is instant, because channels are mixed on the GPU rather than
when a tile is read. A tile keeps each channel's intensity as it was stored —
half floats, four channels to each layer of an array texture, up to sixteen —
and its shader paints each channel in its color by as much as its intensity
reaches through its window, from a small uniform. Showing, hiding or
brightening a channel rewrites that uniform on each resident tile and nothing
is read again, so a slider can be dragged and watched. It used to mean reading
every visible tile afresh, a second or two a change. A stack drawn in 3D mixes
the same way, from the same shader code, so it follows as instantly.

What that costs is memory: eight bytes a pixel for up to four channels, where a
baked tile was four, so the same tile cache holds half as many tiles.
Intensities are divided by the largest their integer type holds before being
stored, which keeps them where a half float has three significant figures —
enough for a window as narrow as the reference stack's 0–1377 of 65535.

Every keyboard shortcut acts on that same frame and no other: paging a stack,
stepping sections, toggling a channel, resetting a view. A key that reached
every open dataset would page or toggle the one nobody was looking at, and with
the pointer parked over the sidebar there would be nothing to say which frame it
had been talking to. The pointer still decides what a drag or a wheel applies
to, since those say where they mean. The outline appears only once there is more
than one frame — drawn round the whole grid it reads as a border on the window
rather than as an answer to a question nobody asked.

**New frame**, beside the sidebar's title, opens a frame with nothing in it
yet, and an empty frame is where a dataset is
found: a search over everything open and everything the catalogs offer — the
examples, the Brain Knowledge Platform, the BKP Registry — with buttons
narrowing it to images, cells, tables or annotations. Each result names the
catalog it came from, under that catalog's heading and on its own row, so a
filtered list still says where each one is from. Choosing one fills the frame;
closing the browser closes the frame. One that has not been opened yet is
fetched; one that has — including one whose frames have all been closed — is
shown as the source it already is, rather than being downloaded again to arrive
at the same dataset twice. A frame that already shows something can browse too,
from **Browse all datasets…** on its title's dropdown, and keeps what it shows
until something else is chosen.

Point clouds draw each point as a screen-space quad rather than with point
topology, because the hardware fixes point primitives at one pixel and offers
no way to size them. The four vertices of a point share its position and carry
a unit corner offset that the shader expands in clip space, so the size is a
uniform: the point size control costs nothing to drag and never rebuilds a
mesh. Sizing is offered only for sources that advertise it, so an image never
shows a control that means nothing to it.

Fading dims the color rather than lowering alpha. Alpha compounds with
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

The sidebar's sections are Feathers containers. Each is a headed box with its
own background, border and rounded corners, which is what separates one section
from the next rather than a run of headings down a flat column. A section of
the dock itself — View configuration, Layers, Cell properties — is drawn as a
**pane**; a section inside one, such as a single cell property, is drawn as a
**group**, whose darker header says it belongs to the pane above it rather than
competing with it. Feathers has no collapsing container, so the open and close
behavior is still ours: the header is a full-width plain button that hides the
body under it, and a closed header rounds all four corners, since it no longer
has a body to finish the box.

The app starts maximized, since several frames beside a sidebar need the room.

Each accordion can carry a menu button on the right that opens a popup. A menu
is capped at the bottom of the window and scrolls once its contents no longer
fit. Frames are opened, cloned and closed by `PanelRequest`, whichever button
asks — the sidebar's New frame, a frame's own corner buttons, its browser — so
the rules about what may be opened or closed live in one place and the routes
cannot drift apart.

The search in a frame's browser also takes an address: anything typed that looks
like a URL or a path is offered first as one to read, so Enter opens what was
pasted. Nothing asks which format it is — the address is read and the format
worked out from what comes back, so an OME-Zarr store, a single point cloud and
a sectioned dataset are all pasted into the same search. A `.dzi`, `.svg`, `.csv`, `.tsv` or `.parquet` is named by its
extension, since nothing else uses those. A `.json` is tried as Scatterbrain
metadata first and as an image manifest second; anything else is tried as a
Zarr store. A
sectioned dataset is recognized by its metadata listing more than one slide.
The command line reads what it is given the same way, so `--points` and
`--slices` say only that a dataset gets a frame of its own, not what it is.

Whatever is recognized is registered as a source like any other and fills the
frame it was chosen in, so it is indistinguishable afterwards from one named on
the command line — it appears in every picker, carries its own transparency and
point size, and offers its cell properties in the sidebar. An address that
matches nothing leaves the reason in the browser, naming what was tried rather
than failing silently. The read is a blocking fetch and parse, so it runs on a
task and the window keeps drawing while it is in flight; reads are taken one at
a time, in the order they were asked for.

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

Each frame's header carries an info button, which opens the inspector on it,
and a camera, which saves a picture of it. The picture is the frame's own viewport cut out of
a window screenshot, at the size it is on screen, with that frame's chrome
hidden for the shot so the header, its buttons and the selection outline are not
burnt into it. Where the frame drew nothing the picture is transparent: the
window's own alpha carries brightness rather than opacity when HDR is on, so the
color the frame clears to is keyed out instead — which cuts hard, leaving a
dark fringe on antialiased edges. Files go to `screenshots/`, named after the
dataset and the moment, and the frame says where its last one went. The encode
runs on a task, since several megapixels of PNG is enough work to freeze the
window if done where the pixels arrive.

While anything a frame shows is being fetched — tiles, nodes, slices for the
3D view, or a dataset it is about to switch to — a thin bar sweeps across its
top edge. It says only that more is coming: streaming has no end to measure,
since what is wanted changes with every pan, so the bar keeps going while what
has arrived is drawn under it, and lingers a moment so bursts of tiles do not
make it flicker.

The copy button in a panel's corner duplicates it, and the `x` closes it. Closing
renumbers the remaining frames so the grid stays contiguous. The last frame can
be closed too: the window returns to the empty state it started in, examples and
all.

Duplicating: The copy inherits the source's
current center and zoom rather than its fitted defaults, so it starts as the
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
| copy button | duplicate that panel |
| `x` button | close that panel |
| info button | open the inspector on that frame |
| cube / square button | look at a stack in depth, or go back to the flat view; offered only where the data has depth |
| drag, in 3D | turn the volume; right-, middle- or shift-drag slides it, scroll zooms toward the pointer, `R` turns it back |
| dataset name | show a different dataset in that frame |
| `...` button | add or remove that frame's layers |
| drag sidebar edge | resize the sidebar, or collapse it — the pointer becomes a resize cursor over the handle |
| `<` / `>` button | collapse or expand the sidebar |
| `1`–`9` | toggle an image channel |
| `G` | sections panel: grid ↔ single slice |
| `←` `→`, `[` `]` | step through slices |

Each frame carries a header in its top corner — the dataset's name, which opens
a menu for pointing that frame at a different dataset, a button that opens the
inspector on it, and a menu of what is layered over it. Frames are drawn over imagery that is bright in places and
black in others, so the header and the status beneath it sit on a translucent
panel. The frame's own controls, duplicate and close, stay in the opposite
corner: what manages the frame is kept apart from what describes the data.

Pointing a frame at another dataset is what the source indirection was for. The
camera moves onto that source's render layer and is reframed to its extent;
nothing about the format is involved. The name's menu offers every open dataset
and every known one not read yet; choosing one of those reads it, says so on
the frame's status line while it does, and swaps it in once it lands. The menu
opens with the keyboard in a search field: typing narrows the list to datasets
whose name, kind or address contain every word typed, Enter takes the first
match, the arrow keys move through the rest, and Escape closes it. The
frame's layers stay where they are, over the new dataset.

Each panel carries its own overlay: the image reports the pyramid level, scale
and tile cache; the point cloud reports octree depth, nodes loaded and points
resident.

Hovering a frame shows what is under the pointer in its bottom corner. The
sections and point-cloud panels name the cell — Scatterbrain gives a point no
id of its own, so its address is the octree node plus its offset within that
node's columns, and for a sectioned dataset the slice as well — along with its
value in whatever property the points are colored by. The image has no cells
to name, so it reports the place instead: the full-resolution pixel, the
pyramid level being drawn, and the tile that covers it.

Hovering also **enlarges every other cell sharing that value**, which is what
turns a color into something you can trace through a dense cloud. It costs no
geometry: each vertex already had room to carry its point's category alongside
its corner, so the highlight is a uniform naming one category and the shader
draws those points larger. Nothing is rebuilt, refetched or re-uploaded beyond
a few bytes per resident node, so it keeps up with the pointer over millions of
points. With no property selected there are no groups to pick out, and the
tooltip still names the cell.

## Layers

A frame can draw several datasets, one over another: annotations over the slide
they were drawn on, a point cloud over the image it was measured from. The
dataset a frame opened onto is the bottom of its stack, and anything open — or
anything any catalog lists — can be put on top: from the frame's own `...`
menu, from the **Layers** section of the sidebar, or with `--layer` on the
command line, which stacks onto the first frame and can be repeated. Both menus
offer what could go on top through the same searchable picker as a frame's
title, so every catalog is there, not only the examples.

A catalogued dataset that is not open yet is read when it is chosen and layered
once it lands, so nothing has to be opened in a frame of its own first. Choices made
while one is still being read wait their turn rather than being dropped, since
the menu they were made from is far from any status line that could say so. A
dataset is recognized as open by the address it was read from, which every
source records — including one named on the command line — so choosing it
again reaches the source it already is rather than fetching it twice.

Nothing refuses a layer for what it is. Whether two datasets mean anything drawn
together is the judgement of whoever is looking, and a viewer that refuses is a
viewer people take screenshots from to overlay by hand. What a layer is *not* is
rescaled: it is drawn in its own coordinates, so two datasets line up when they
were measured alike. One whose unit differs from the frame's is still layered,
and marked — `um over px, not rescaled` — so a coincidental alignment is not
mistaken for a real one.

The Layers section lists the selected frame's stack top first, each with its
own transparency and a button to take it off, over the list of everything else
that could go on top.

A layer's transparency is true see-through, unlike fading a dataset from View
configuration, which dims it. Everything a source draws overdraws itself — an
image keeps coarser tiles under the finer ones, a point cloud stacks dozens of
points on a pixel — so fading its geometry directly compounds wherever it
overlaps, which is why fading a frame dims instead. A layer does not have that
problem because it is never faded where it is drawn: each layer camera renders
into an image the size of its frame, cleared to nothing, and that image is laid
over the frame at the layer's opacity. Its overlaps are settled before any
transparency is applied, so the dataset beneath shows through evenly, whatever
format either is. The opacity belongs to the layer rather than to its dataset,
so one dataset can be faint over one frame and fully shown in another.

The cost is an image per layer the size of its frame — four bytes a pixel, so
about 8 MB for a 2000 × 1000 cell — resized with the frame, and nothing drawn at
all for a layer at zero. The image holds color already multiplied by its
coverage and is laid over as if it did not, so a soft edge on a layer comes out
slightly darker than it would drawn straight onto the frame; tiles are opaque
and do not show it.

Each layer is a camera of its own, sharing its frame's view and projection,
drawing its source's render layer into that image; the images are UI images
below every piece of frame chrome, in stack order. Nothing a format spawns has
to know it is in a layer, and one source can be the base of one frame and a
layer of another at once. A layer camera carries the same `ShowsSource` a frame
does, which is all a streamer asks of a view, so a layered dataset streams
exactly as it would in a frame of its own. Closing a frame despawns its layers
and their images, duplicating one copies them at the same opacities, and
pointing a frame at a dataset already in its stack drops that layer rather than
drawing it twice.

Hovering asks every source in the stack what is under the pointer, and the
tooltip lists the answers topmost first — over a slide with annotations, the
structure and region first, then the pixel.

A stack holds at most eight datasets, which is the band of camera orders each
cell is given.

## Bookmarks

The **Bookmarks** section of the sidebar saves what is on screen to come back
to, or to send to someone. A bookmark holds every frame in grid order: the
dataset it shows, its view (or its 3D orbit), and the layers over it at their
opacities. For each dataset it also holds the slice showing, whether sections
are in a grid, opacity, point size, each channel's visibility and gain, what
points are colored by, and every cell filter. It leaves out app preferences
such as the theme and the sidebar's width. A bookmark someone sends you should
not change those.

Saved bookmarks are one JSON file each, in
`$XDG_DATA_HOME/bevy-data-explorer/bookmarks` (or `~/.local/share/...`).
Choose one in the list to restore it. The buttons beside it copy it to the
clipboard as a single `bde1:` line, export it through a save dialog, or delete
it. **Load from clipboard** and **Import…** take either form back, add it to the list and
open it. `--bookmark` does the same from the command line.

A bookmark is not a dump of the ECS. Frames point at source entities and
sources hold streamers and render layers, none of which means anything in
another process. So a bookmark names things as a person would: datasets by
address, channels by label, cell filters by column and code. Restoring replays
it through the same paths opening a dataset by hand takes. Every address is
read at once, and a dataset already open is reused. The frames replace what is
on screen only once every read has landed or failed. Cell filters wait until
the catalog's service has replaced the format's placeholder properties, since
filters set any earlier would be thrown away with the placeholders.

Views are saved as the world they showed rather than as a zoom factor, and
fitted into whatever cell they are restored into. A smaller window therefore
shows the same region instead of a smaller piece of it. Anything that no longer
matches — an address that fails, a channel or property the dataset dropped —
is skipped and reported, and the rest is restored. A bookmark naming local
files says so in the list, because it will not open on anyone else's machine.

## Releases

Pushing a `v*` tag builds Linux, Windows and a universal macOS binary and
publishes them as a GitHub release. `workflow_dispatch` rehearses the same
build against an existing tag and leaves the release as a draft.

The macOS binary is not signed or notarized, so Gatekeeper quarantines it on
download and refuses to open it. Clear the quarantine from the folder it was
extracted into:

```sh
xattr -d com.apple.quarantine ./bevy-data-explorer
```

If it is still killed on launch on Apple Silicon, sign it ad hoc as well —
joining the two architectures into one binary can leave its signature invalid:

```sh
codesign --force --sign - ./bevy-data-explorer
```

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
points at. Adding a format means writing its systems plugin, a `spawn_source`,
and teaching `formats::discover` to recognize it; nothing else changes, and
frames are opened by querying the world for sources rather than listing them.

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
frame keys — the arrows, `[` and `]`, or `PageUp`/`PageDown` — and the slider
in **View configuration** move it; paging acts on the selected frame, so two
specimens open side by side are paged one at a time. Nothing in the sidebar
knows an image is what it is paging: the stack is a component on the source,
the way a point size is, and any format that has one can offer it. A sectioned
point cloud does, so its sections page the same way, and a step from the grid
shows the slice it stepped to. Whether it shows every section at once is a
second component beside the stack, which `G` and the **Show every slice** box
under the slider both switch. A stack opens on its middle
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

### Stacks in three dimensions

A stack whose metadata measures z the way it measures x and y — a spatial
axis, in the same unit — can be looked at in depth: its frame's header offers
a cube, which turns the frame from the slice it is paging to the whole specimen,
seen from a little off its face, and a square turns it back to the view it left.
The Tissuecyte stack qualifies: its sections are 0.1 mm apart in the same
millimetres as its 0.35 µm pixels, so piled at that spacing they are the block
the instrument cut. A stack whose z is not spatial, or in another unit, still
pages; it just says nothing about where its slices lay, and drawing it in depth
would invent that.

Nothing else is offered this way yet, and not for want of a renderer. The
sectioned point cloud looks like the obvious candidate and is not: its
metadata places each slice in x and y only, lists them in no anatomical order,
and the one per-slice column it has — a section label — is a code whose order
does not follow the anatomy either. Stacked at a guessed spacing it would be a
reconstruction nobody made. The button is driven by a `SourceVolume` component
on the source rather than by format, so a point cloud that does carry z — or
a format that is 3D to begin with — offers it by advertising one.

The volume is read whole, once, the first time a frame turns to it: at the
finest level of the pyramid that fits 16M voxels, which for the reference stack
is the coarsest, 312 × 234 × 142, read in about a second. Its channels are
kept apart as a tile's are — the first four, which is what one 3D texture
holds — uploaded as one 3D texture, and drawn as a single box whose shader
mixes them as it marches each pixel's ray through it and
keeps the brightest sample — a maximum-intensity projection, the usual way to
look at fluorescence in depth, and one that needs nothing sorted. Dark tissue
lets the frame show through; the brightest signal hides it.

Zooming in reads finer detail. What the 3D frames can see of the volume —
every point along every ray they cast, which head-on is a column through all
142 slices and side-on is most of the block — is read again at the finest
level where it fits the same budget, and drawn inside the whole in place of
the coarse voxels there. Since a pyramid shrinks x and y and never z, the
budget buys a region about 330 pixels square through the whole stack at any
level: a millimetre square in the middle of the reference stack comes back at
level 4, eight times finer than the whole, in about two seconds, and a smaller
view goes finer still. The region is widened to whole chunks, so a small turn
asks for the region already held rather than one a pixel over, and nothing is
read until the view has held still for a moment — a turn crosses many regions
on the way to the one it stops at. The whole stays drawn around the detail, so
turning away shows the rest of the specimen at once, coarsely, rather than a
hole.

A 3D frame is still the same camera. Its projection turns perspective and it
draws a render layer the volume was given of its own, so it shows none of the
tiles the same source streams for flat frames, and its place in the grid — its
draw order, and whether it is the one camera that clears the window — does not
change. Everything that reads a frame's view as flat already passes over one
that is not, so a 3D frame streams no tiles and shows no tooltip rather than
answering wrongly. Duplicating one duplicates it in 3D, turned the same way;
pointing it at another dataset puts it back in 2D.

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
Both shapes are modeled as a list of slides so the rest of the viewer does not
have to know which it opened.

Because the slices share a coordinate system they would otherwise pile up, so
each is re-centered on its own bounds and given a layout offset. Slices differ
in size, so the grid uses a cell sized to the largest of them, which keeps the
anatomy aligned from one row to the next rather than drifting. The offset lives
in each node's transform, so switching between grid and single-slice layouts
only rewrites transforms and visibility — nothing is refetched.

### SVG annotations

Not an SVG renderer. Annotation tools export regions as polygons in the pixel
space of the slide they were drawn on, with labels in attributes of their own
(`structure-label`, `region-label`), and that is what is read: `polygon`,
`polyline`, `line`, `rect` and straight-segment `path`s, their stroke color,
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

### CSV and TSV tables

RFC 4180, with the tab allowed in place of the comma and the delimiter decided
from whichever separates the first line more. Quoted fields may hold the
delimiter, a doubled quote or a newline. A row shorter or longer than the
header is padded or trimmed rather than rejected, since one bad line should not
cost the file. A column whose every value parses as a number is set flush
right. A file is read as far as 100,000 rows and says how many it left.

A table is not a view onto anything: its rows are records rather than a place,
so there is nothing to pan over and nothing finer to zoom into. Its frame is
filled with the table instead — a header that stays put while the rows move
under it, the numbering gutter down the side, and the same scrollbars every
scrolling area in the app gets. Holding shift turns the wheel sideways, which
is the only way across for a wheel with no sideways axis of its own. The
frame's camera still clears the cell behind it, which is what keeps the table
on the theme's own background, and the table starts below the frame's own
chrome — measured from the header rather than assumed, since a header is as
tall as the status it reports.

The drawing lives in `view/table.rs` rather than in the format, keyed off a
`SourceTable` on the source entity. So a table looks the same whatever produced
the rows, and a format that has rows to show writes that component rather than
learning to draw. It is the third of the things `source/` carries between a
format and the grid, alongside hover and region.

A table is read a page at a time — a hundred rows, with buttons along the
bottom for the first, previous, next and last, and a line saying which rows are
on screen of how many. Two different things serve that page, and a frame does
not know which: a table read whole keeps the rest of itself out of sight and
slices what is asked for, while one read from an API fetches the page it is
turned to. Which is why the page a frame is on is a component on the source —
another of the questions the grid asks and a format answers, alongside hover
and region.

Within a page, only the rows on screen exist. Even a hundred rows of thirty
columns is three thousand UI nodes, so the content node is given the page's
full height and the rows inside it are placed absolutely: the scrollbar
measures the page while a screenful is what gets built. Columns are sized from
the characters a value has rather than by laying the text out, which would mean
reacting to the measurement a frame later; a value too wide for its column is
cut with an ellipsis by the same estimate that sized it, so a column as wide as
its widest value never truncates it.

Pressing a heading sorts the table by that column: ascending, then descending,
then unsorted again, with an arrow beside the heading saying which. Pressing
another heading sorts by it instead; holding shift adds it as a further key,
deciding only among rows the keys before it leave tied, and each sorted
heading is numbered by where it falls. The sort is another component on the
source, served the way the page is: a table read whole sorts its rows here —
numbers as numbers, words without regard to case, gaps last whichever way —
and one read from an API asks for its pages in that order. A sorted table
starts again from its first page.

The section's menu in the sidebar picks which columns are drawn, as the cell
properties' menu picks which properties are listed. A hidden column is only
left out of the frame; a filter or a sort on it still applies. The page, the
filters, the sort and the hidden columns are all saved in a bookmark.

What a table's frame does not offer, because none of it means anything for
rows: the zoom line in the overlay; the button that saves a picture, since the
table is drawn over the frame rather than into it and the camera has nothing
but the cleared background to photograph; and anything to do with layers,
neither the frame's own menu nor the sidebar's section — a table shares no
coordinates with an image, so stacking one on the other would put two
unrelated things in one cell. The same goes the other way: a table is never offered as a
layer over another frame, a frame repointed at a table drops its layers, and a
request to layer one — from a bookmark, say — is refused.

### Parquet tables

Read whole and without Arrow, row by row, into the same table a CSV becomes.
Every top-level field is a column. A list is shown as its elements joined by
commas, a null as an empty cell, so a column of integers with gaps still reads
as numbers. Snappy, zstd, LZ4 and gzip are read; brotli is not, and a file
compressed with it says so. The same 100,000-row ceiling applies.

### Brain Knowledge Platform specimens

Not a file format: a question asked of the platform's GraphQL API, addressed
as the endpoint with the project named on it, so the thing being talked to is
the thing being named and another deployment is reached by writing its host.

```
https://idf-api-prod.aibs-idk-prod.net/?specimens=JGN327NUXRZSHEV88TN
```

A specimen is not a row. It carries a list of annotations and a list of
measurements, each tagged with the feature it belongs to, and which features a
specimen has varies within one project — so the columns are the union of the
features seen, and a specimen with nothing under a feature leaves that cell
empty. A measurement names its unit in the column rather than in every cell.

Three things the records do that a naive flattening gets wrong, all found by
reading the live API before writing the reader:

- **An annotation can name more than one taxon.** A SEA-AD donor with two
  clinical diagnoses carries both, so they are listed rather than one being
  picked.
- **The platform repeats some measurements.** Every SEA-AD donor carries sex
  and age at death twice over, with the same value both times, so the first
  reading is taken; showing a value beside itself would say something the data
  does not.
- **Its column order changes between requests.** The table fixes one: what
  identifies a specimen, then its annotations by name, then its measurements
  by name.

The title, the count and the first page come back in one request — three root
fields — so opening a table costs one round trip however large the project is.
That matters: reading the Genetic Tools Atlas whole was 34MB and some twenty
seconds before a frame appeared, against a page of a hundred rows now. Turning
a page fetches that page and nothing else.

The columns are settled by the first page and kept, so turning a page does not
relay the table out under the pointer. A later page carrying a feature no
earlier one had grows the columns rather than losing the value — they only ever
gain, which the frame notices and lays itself out again — and a column's width
grows the same way, since only the page in hand can be measured and a column
that narrowed on every page would shuffle the table sideways each time.

A table's rows can be narrowed from the sidebar, in the section under the cell
properties and built from the same pieces: a sub-section per column, a checkbox
per value with its count beside it, and a button on the header that clears the
lot. Narrowing happens at the platform rather than here — the ticked values go
into the same query the page fetch already builds — so the row count and every
page after it are of the narrowed table.

Ticks within one column widen and columns narrow each other, which is what a
row of checkboxes is read to mean and, as it happens, exactly what the platform
does with a field named twice. The counts beside each value are of the whole
project rather than of what is on screen, so a count says what ticking it would
bring back and does not shift under the pointer while ticking. A column holding
more values than the cell properties above it will list — a donor id, say, one
per row — is cut to the same figure and says how many are left.

Every column is asked about, annotations and measurements alike, and the
platform decides which it can answer. A column it cannot comes back null beside
the ones it could, so one column's failure costs only itself — and that null is
also what marks a column out as numbers: grouping by one answers *"Unable to
cast object of type 'System.Double' to type 'System.String'"*. A column of
numbers is narrowed by taking a span of it rather than by ticking every reading
anyone took, drawn with the same histogram and two-ended control the cell
properties use.

A span is asked for when its column is opened, not when the table is. The
platform has no query that gives a column's extent — `measurementStats` sits on
`aio_specimenFacetedSearchProperties`, which answers with an empty list for
every project — so a distribution is a cumulative count at each of twenty-odd
bucket edges, one index query apiece at about 0.14s. Asking for every numeric
column of the SEA-AD donors up front would be a minute of waiting for
histograms nobody looked at.

The edges are built around the numbers already on the page and widened by half
their span at each end, since a hundred rows of ten thousand will not hold
either end; the outermost buckets that hold anything are then the extent, so a
column asked about far wider than it runs is still drawn across what it has.
Two things its range parser will not take: scientific notation — `-1e12` comes
back *"Invalid range format"* — and, as ever, a page it has decided is too far
in.

Two things the records do that only showed up on the last page, both now
fixtured:

- **A list can arrive as `null`** rather than as an empty one, for a specimen
  with no measurements at all. Serde's `default` covers a field that is
  missing, not one that is present and null.
- **A page can be refused.** The platform's search index rejects an offset and
  limit summing past 10,000, so the tail of a project larger than that answers
  with an error. The rows on screen are left alone and the frame is put back on
  the page it is actually showing — without that it asks for the missing page
  again every frame, forever.

Which projects have a table is the platform's own answer rather than a guess.
Every project carries a list of capabilities, and `SPECIMEN`, `DONOR` and
`PARTITIONED_SPECIMEN` are the three that mean there are specimens to
tabulate. That was measured against the live API rather than read off the
names: every project carrying one of them answered with rows, and every
project carrying only `BKP_DATASET`, `OME_ZARR`, `SPECIMEN_FILES` or nothing
answered with none. `SPECIMEN_FILES` is the trap — it reads as though it
should qualify, it does not, and no project carries it alone.

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
- **A slice is read out of a deep chunk block by block.** The Tissuecyte
  stack keeps forty slices in every chunk, so a level-0 tile of one slice
  covered sixteen chunks of 2.3 MB each: 37 MB downloaded to draw one slice
  in forty. Blosc compresses a chunk in independent blocks — here 256 KB,
  eight slices of one channel — and lists where each starts, so a tile now
  reads each chunk's header and table once, then only the blocks holding its
  slice: three of fifteen. Against the live store the tiles come back byte for
  byte the same, in 0.25–0.45 s where they took 1.1 s. Only plain Zarr v2
  blosc chunks with a fill of zero are read this way; anything else, or any
  block read that fails, is read whole through zarrs as before.
- **Tiles are cached well past leaving the viewport.** Zooming in narrows the
  wanted set to a handful of fine tiles; the surrounding coarse ones are
  exactly what is needed again on the way back out. They are evicted
  least-recently-wanted, under a memory budget, rather than on sight.

### Interoperability notes

- OME-NGFF specifies omero channel colors as six bare hex digits, and
  `ome_zarr_metadata` enforces that. Real converters write `#RRGGBB`, and
  sometimes the CSS shorthand `#0df`. Both are normalized rather than rejected.
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
- A known dataset's unit is not known until it has been read, so the menus can
  only warn that a layer is measured differently once it is open.
- A custom dataset is opened into a frame but cannot be closed again as a
  *source*: closing its frame leaves the source registered, still holding
  whatever it has streamed, and its render layer is not handed back.
- The sections panel always draws slices in metadata order. It carries no
  notion of anatomical position, so the grid is a contact sheet rather than a
  reconstruction.
- Brightness moves only the top of a channel's window, and only up to four
  times as published; the bottom stays where the dataset put it. An image
  mixes at most sixteen channels, and a volume its first four.
- Reads are synchronous, so a request already under way cannot be abandoned.
  Queued work is dropped when the view moves on, which is where a backlog
  actually builds up during a fast pan.
- Only the first multiscale image in a store is shown, at a single z slice.
- A stack in 3D holds one region of detail at a time, for everything its 3D
  frames can see together, so two frames zoomed into different places share
  a region wide enough for both, at a coarser level. It is drawn as a
  maximum-intensity projection only. Layers are not drawn over a 3D frame,
  and it answers no hover.
- Point coloring is fixed to the first categorical column; the others are
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
teaches you to ignore it. It stays a local step, as `AGENTS.md` describes.

Clippy is not in CI either, for now: the tree has around forty-five warnings,
mostly argument counts on systems and complex query types, and turning it on
would mean either failing every pull request or ignoring the job. Clearing them
is worth its own change, and the job can go in with it.

Dependabot opens weekly pull requests for the crates and for the actions in the
workflow. Patch and minor crate updates are grouped into one, so a quiet week is
one pull request rather than nine; majors arrive on their own, since those are
the ones worth reading.

## Icons

Button icons are glyphs from [Lucide](https://lucide.dev) 1.47.0, embedded as
its icon font (`src/widgets/assets/lucide.ttf`, ISC license alongside it). The
codepoints in `src/widgets/icons.rs` come from that release's `font/info.json`
and move between releases, so upgrade the font and the table together.
