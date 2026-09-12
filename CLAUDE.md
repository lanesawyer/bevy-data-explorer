# bevy-data-explorer

A streaming explorer for large scientific datasets, built on Bevy 0.19. See
`README.md` for what it does and how the pieces fit; this file covers what is
easy to get wrong.

## Extending it

Every format is a Bevy plugin that registers a *source entity*. Adding one
means writing a plugin and adding it — `panel`, `hud` and `main` need no
changes, and `main` discovers sources by querying the world. See
`.agents/skills/add-source-plugin`.

Frames point at a source entity rather than naming a format. Streamers bind to
their source on construction and select panels with `shows.0 == streamer.source`
rather than by type, which is what will let two frames show different datasets
of the same format.

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

- **512px tiles** (`dataset.rs`). Bytes transferred are the same at any tile
  size; round trips are not, and 128px was ten times slower.
- **Per-shard decoder cache** (`tiles.rs`). `retrieve_array_subset` refetches
  the 16KB shard index for every tile.
- **24 tile threads on top of the core count** (`main.rs`). Reads are blocking,
  so one tile holds one thread; Bevy's async-compute pool caps at four.
- **4M section budget** (`slices.rs`). The grid draws every slice at once and
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
  or renders accumulate.
- **Chrome that overlaps the grid needs `BlocksFrameInput`** and, if it holds
  no buttons, an `Interaction` of its own — otherwise the pointer falls through
  to the frame behind.
- **BSN scene components** are patched `@Component { @prop: {expr} }`, with the
  `@` on both. Components with private fields cannot be patched field by field;
  supply them whole with `template_value(...)`.
- **Bevy system tuples cap at 20.** Split into sets rather than one long chain.
