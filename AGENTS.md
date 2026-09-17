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

The tree is layered, and the layers only point one way:

    source/    the vocabulary every format and frame is written against
    formats/   the readers, one plugin each
    render/    the point pipeline both point-cloud formats draw through
    view/      the frame grid: grid, camera, chrome, requests, input, overlay
    widgets/   generic controls, used by both view and ui
    ui/        the docks and the controls inside them
    app/       the shell: window, task pool, theme, schedule
    cli.rs     the command line
    main.rs    parse arguments, register what they name, run

`src/source` imports nothing from above it. Keep it that way: it is what lets
a frame point at any dataset without naming a format.

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
- **A camera with `ShowsSource` is not necessarily a frame.** Layer cameras
  carry it too, which is what makes a layered source stream. Anything that
  *iterates* frames — counting them, laying them out, drawing chrome — filters
  `With<Panel>`; streamers deliberately do not.
- **Chrome that overlaps the grid needs `BlocksFrameInput`** and, if it holds
  no buttons, an `Interaction` of its own — otherwise the pointer falls through
  to the frame behind.
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
