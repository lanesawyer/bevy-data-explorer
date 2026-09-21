# bevy-data-explorer

A streaming explorer for large scientific datasets, built on Bevy 0.19. See
`README.md` for what it does and how the pieces fit; this file covers what is
easy to get wrong.

## Extending it

Every format is a module under `src/formats/` with two halves: a systems
plugin, added once by `FormatsPlugin` whether or not anything of that format is
open, and a `spawn_source` that registers one dataset as a *source entity*.
`formats::discover` recognises a dataset by reading it and `spawn_discovered`
hands it to its format, and that one path serves the command line, the URL
field and the examples alike. Adding a format touches `formats` and nothing in
`view`, `ui` or `main`; frames are opened by querying the world for sources.
See `.agents/skills/add-source-plugin`.

What the dataset dropdown offers comes from *catalogs* (`src/catalog/`), each
an implementation of `Catalog` registered in `main` with `add_catalog`. A
catalog only lists names and URLs, listed asynchronously; opening an entry goes
through `discover` like any typed URL, so a catalog never names a format.

An entry may also carry a `CellService` (`DescribeCells`) that knows the
dataset's cells better than its files: labels, colors, counts.
`catalog/cells.rs` matches a source's `SourceUrl` to its entry and replaces the
format's placeholder `CellProperties` with the service's answer. Formats hand
the service a `CellColumns` rather than their own types. Giving a new catalog
real cell properties means implementing `DescribeCells` for it, as
`catalog/bkp/cells.rs` does. It never means touching a format or the panel.
See `.agents/skills/add-catalog`.

A bookmark (`src/bookmark/`) saves intent, not entities: addresses, labels,
column ids and codes, restored through `discover` and the frame spawners like
anything opened by hand. A new per-source setting worth keeping goes in
`SourceState` as an `Option` (absent means "leave alone"), is read in
`capture.rs` and written back in `apply_pending_settings`. Anything that
depends on something arriving asynchronously waits there, the way cell filters
wait for `Described`. See `.agents/skills/add-bookmark-setting`, and
`.agents/skills/add-ui-control` for the controls that set these.

The tree is layered, and each layer imports only from those above it in this
list:

    source/    the vocabulary every format and frame is written against
    render/    the point pipeline both point-cloud formats draw through
    formats/   the readers, one plugin each
    catalog/   lists of datasets to offer: the examples, BKP, and so on
    widgets/   generic controls, used by both view and ui
    view/      the frame grid: grid, camera, chrome, requests, input, overlay
    bookmark/  saving what is on screen, and restoring it
    ui/        the docks and the controls inside them
    cli.rs     the command line
    main.rs    parse arguments, register what they name, run

`app/` is in two halves. Its submodules — `net` (the async runtime reads go
through), `schedule` (the stages), `theme`, `prefs` and `logs` — are the
ground floor: every layer but `source` imports them, and they import nothing
but `source`. `app/mod.rs` is the shell on top: it builds the window and task
pool and adds every layer's plugin, so it imports all of them. Keep the
submodules free of anything above `source`; a submodule that needs `view` or
`ui` belongs in that layer instead.

The docks on the right stack rather than share an edge. Each subtracts its own
width from `FrameArea`, which keeps the frames clear of both, but that says
nothing about where either is *drawn* — so a dock inboard of another places
itself past the one outside it and offsets its drag handle by the same amount.
`ui/selection.rs` sits inboard of `ui/inspector.rs` and is the example.

`src/source` imports nothing else from the crate. Keep it that way: it is what
lets a frame point at any dataset without naming a format. Something a format
and a frame both need, such as `ShowsSource`, lives there rather than in
`view`, and a keyboard shortcut that acts on a source belongs in
`view/input.rs`, not in the format that reads it.

Two of the things in `source/` are questions the grid asks and a format
answers, and both work the same way: the grid writes a component onto the
source entity, the plugin writes another back, and the UI reads the answer
without knowing what produced it. `hover` asks what is under the pointer;
`region` asks where a rectangle dragged over a frame lands in the dataset's own
coordinates, which only the format knows because only it knows whether its
points were drawn where their coordinates put them or laid out into a grid of
sections. A plugin that cannot answer simply never writes the answer.

`source/table.rs` goes the other way: a format that has records rather than a
place writes a `SourceTable` and draws nothing at all. `TablePaging`,
`TableSort` and `TableFilters` beside it are more questions of the same kind:
the frame's buttons write the page, its headings the sort, and the sidebar
ticks the values, and whatever produced the rows serves them —
`formats/table.rs` slices and sorts a table it read whole,
`formats/specimens.rs` fetches one sorted and narrowed at the platform.
`HiddenColumns` is the frame's alone: no format reads it. Neither
knows about the controls, and `ui/table_filters.rs` names no format: a second
source of rows is filtered by it without a line changing there.

A numeric column is narrowed by a span, and that span is drawn by the same
control the cell properties use. `ui/cell_panel/range.rs` takes a `RangeOwner`
saying where its `NumericRange` lives — a cell property or a table column — so
there is one histogram-and-two-ends widget rather than two that drift apart.
Anything else that grows a numeric range should name itself there too. `view/table.rs` fills
that frame with a scrolling table — there is nothing to pan over, so its frame
scrolls rather than moves. Only the rows on screen are built. A format's whole
job there is producing rows, which is why neither `formats/csv` nor
`formats/specimens` has any systems: everything they know is known the moment
they are read, and `formats/table.rs` is where both of them end up.

`formats/specimens` is not a file format at all — it is a query against the
Brain Knowledge Platform, addressed as the API endpoint with the project on
its query string. A format may talk to an API; what it may not do is reach
into `catalog`, which is below it. The endpoint comes off the address rather
than from `catalog::bkp::PRODUCTION` for exactly that reason.

The platform gets two catalogs rather than one. `catalog/bkp` lists a
dataset's visualizations; `catalog/bkp/projects` lists the projects whose
specimens are a table, which is a different query against the same API. Apart,
because each keeps its own slot and a slow or failing one then costs only its
own entries. A project qualifies on the capabilities it declares, never on
what its title suggests.

Frames point at a source entity rather than naming a format. Every streamer is
a component of the source entity it serves, and selects panels with
`shows.0 == streamer.source` rather than by type, so two frames can show
different datasets of the same format. Do not make a streamer a `Resource`
again — that is what previously limited images and sectioned data to one
apiece, and it hides the limit until someone opens a second.

## Verify before claiming

The app opens a window, so it never exits on its own. Always run it under a
timeout and grep the output rather than waiting:

```sh
cargo fmt && cargo build 2>&1 | grep -E "^(error|warning)"
cargo clippy --all-targets 2>&1 | grep -E "^(error|warning)"
cargo test 2>&1 | grep -E "^test result"
timeout 45 cargo run 2>&1 | grep -iE "panic|ERROR"
```

A clean `cargo build` is not enough on its own. Shader errors, pipeline
validation failures and missing-component queries only appear at runtime, and
several real bugs here built cleanly and did nothing.

## Constants that look arbitrary and are not

These came from measuring against the live stores. Changing them without
re-measuring will regress something:

- **512px tiles** (`formats/image/dataset.rs`). Bytes transferred are the same at any tile
  size; round trips are not, and 128px was ten times slower. A sharded level
  cuts the tile from one shard; an unsharded one (every v2 store) reads whole
  chunks from the array so they are fetched together — 159ms against 1.1s for
  the same 512px region read chunk by chunk.
- **Per-shard decoder cache** (`formats/image/mod.rs`). `retrieve_array_subset` refetches
  the 16KB shard index for every tile. Unsharded levels have no index to hold,
  so they skip the cache entirely rather than filling it with one entry a
  chunk.
- **24 tile threads on top of the core count** (`app/mod.rs`). Reads are blocking,
  so one tile holds one thread; Bevy's async-compute pool caps at four.
- **Block reads of deep chunks** (`formats/image/blocks.rs`). A tile of one
  slice fetches only the blosc blocks holding it, not the chunk: 0.3 s
  against 1.1 s a tile on the Tissuecyte stack, byte for byte the same. The
  live test `block_reads` is what proves both; run it after touching either.
- **4M section budget** (`formats/slices/mod.rs`). The grid draws every slice at once and
  their root subsamples alone come to ~3M points. Below that, whole slices
  vanish rather than the grid thinning.

## Traps hit more than once

- **Physical versus logical pixels.** Layout (`ComputedNode::size`,
  `UiGlobalTransform`) is physical; `Val::Px`, `Node.left` and
  `Window::cursor_position` are logical. Scale by
  `ComputedNode::inverse_scale_factor`. This caused both a misaligned slider
  thumb and a menu that dismissed itself.
- **`Interaction` versus `Activate`.** `bevy_ui::Button` drives `Interaction`;
  `bevy_ui_widgets` and Feathers controls trigger an `Activate` event and carry
  no `Interaction` at all. Polling `Changed<Interaction>` on a Feathers button
  silently never fires.
- **Only cell 0 clears the window.** The rest draw over it deliberately. Any
  code that reorders or removes frames must keep exactly one clearing camera,
  or renders accumulate. Layer cameras (`view/layers.rs`) render into images of
  their own, not the window, and are laid over their frame as UI images; they
  clear those images, never the window.
- **A frame's projection is not always orthographic.** A frame looking at a
  stack in 3D (`view/orbit.rs`) is the same camera with a perspective
  projection, drawing its source's volume layer. Anything that reads a frame's
  view as flat — selecting tiles or nodes, hover, pan and zoom — matches
  `Projection::Orthographic` and skips the frame otherwise; do the same, and
  never `unwrap` it.
- **A camera with `ShowsSource` is not necessarily a frame.** Layer cameras
  carry it too, which is what makes a layered source stream. Anything that
  *iterates* frames — counting them, laying them out, drawing chrome — filters
  `With<Panel>`; streamers deliberately do not.
- **Never write the window's `CursorIcon` directly.** Feathers sets it every
  frame from what is hovered, so a direct write is either overwritten or, if
  it writes a default back, silently breaks every other control's cursor — the
  sidebar's did, and the log panel's resize cursor never showed. Put an
  `EntityCursor` on the hovered entity, and hold a cursor through a drag with
  `hold_drag_cursor` in `widgets/dock.rs`. A new dock gets both, and its
  drag, by implementing `widgets::Dock` and registering with `add_dock`.
- **Chrome that overlaps the grid needs `BlocksFrameInput`** and, if it holds
  no buttons, an `Interaction` of its own — otherwise the pointer falls through
  to the frame behind.
- **A Feathers `label` ignores an inherited font.** It carries
  `PropagateOver<TextFont>`, so an `InheritableFont` on the panel around it is
  dropped and the text quietly stays at 13px. Text is spawned through
  `widgets::text` / `text_dim` with a size from `widgets::size`, which patch
  `TextFont` on the label itself; a container's `InheritableFont` still reaches
  buttons and fields, which is all it is for.
- **BSN scene components** are patched `@Component { @prop: {expr} }`, with the
  `@` on both. Components with private fields cannot be patched field by field;
  supply them whole with `template_value(...)`.
- **Ordering lives in `app/schedule.rs` and nowhere else.** A system declares
  `.in_set(Stage::...)`; it never orders itself against another module's
  system. Two bugs came from ordering by chain membership — a menu positioned
  before it was built, and an overlay documented as running after the sources
  that did not. If a new system needs a slot that does not exist, add a stage
  and say in its doc comment why the boundary is there.
- **`system_clipboard` is load-bearing in the feature list.** `bevy_ui_widgets`
  already binds Ctrl+V in the text field, but without that Cargo feature
  Bevy's clipboard is an in-app buffer: copy and paste work inside the window
  and pasting a URL from a browser does nothing. It pulls in `arboard`, and
  the `wayland` feature already wires that through.
- **Bevy system tuples cap at 20.** The stages keep chains short; if one is
  approaching twenty, that is the signal to split it rather than to grow it.
