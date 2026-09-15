---
name: add-source-plugin
description: 'Add a new data format to bevy-data-explorer as a Bevy plugin that registers a source entity. Use when: supporting a new file format, adding a dataset type, or wiring a reader into the viewer.'
argument-hint: 'The format name, and a metadata URL or file to read'
---

# Add Source Plugin

Every format is a plugin that registers a source entity. Done properly, nothing
outside the new module changes: `main` discovers sources by querying the world,
and nothing in `view` or `ui` ever names a format.

## Step 1: Probe the format first

Use the `probe-dataset` skill. Do not write a reader against what the metadata
appears to say.

## Step 2: Write the format layer

Keep parsing and byte decoding free of Bevy types, in its own module, with
tests against a fixture in `testdata/`. Reuse an existing library where one
exists — `zarrs` handles Zarr v3 entirely, including sharded partial reads.

## Step 3: Write the plugin

Put the module under `src/formats/`, and register against `crate::source`.

```rust
use crate::app::schedule::Stage;
use crate::source::{self, SourceExtent};

impl Plugin for MyFormatPlugin {
    fn build(&self, app: &mut App) {
        let source = source::register(
            app,
            source::SourceInfo {
                name: ...,
                unit: ...,    // physical unit, for reporting zoom
                detail: ...,  // provenance, shown in listings
                stat: ...,    // headline figure, e.g. "3.74M CELLS"
            },
            SourceExtent { centre, size, finest },
        );

        app.insert_resource(MyStreamer::new(data, source))
            .add_systems(Update, (...).chain().in_set(Stage::Sources));
    }
}
```

Declare a stage, never an ordering against another module's system. The stages
are defined and ordered in `src/app/schedule.rs`, which is the only place that
decides what runs before what. `Stage::Sources` already runs after the frames
have their viewports and after the dock controls have written through, and
`HoverProbing` already runs after `Stage::Sources`, so a hover resolver needs
no `.after(...)` of its own.

`SourceExtent` is in **display** coordinates, where y is negated so images read
top-down. If what the source shows changes at runtime, keep the extent updated
— a frame opened later is framed from it, and a stale one leaves that frame
unable to zoom out.

## Step 4: Bind the streamer to its source

Store the source entity on the streamer and select panels by it:

```rust
let source = streamer.source;
for (camera, transform, projection, _) in panels
    .iter()
    .filter(|(_, _, _, shows)| shows.0 == source)
```

Union what every matching camera needs, rather than taking the first. Duplicated
frames are separate cameras on one render layer, and a frame that is not
consulted will not stream its own detail. Do not filter the query `With<Panel>`:
a source layered over another frame is shown by a layer camera, which carries
`ShowsSource` but is not a `Panel`, and filtering it out leaves the layer empty.

## Step 5: Draw on the source's layer

Spawn geometry with `RenderLayers::layer(source.layer)`, reading the layer from
the `DataSource` component. Never hardcode a layer: they are allocated at
registration so two sources cannot collide, and layer 0 is deliberately never
handed out because anything spawned without a layer lands there and appears in
every frame.

## Step 6: Report status

Write a `SourceStatus` each frame. Exclude anything view-dependent — one source
can be shown in several frames at different zooms, and the overlay adds those
lines itself.

## Step 7: Advertise capabilities

Insert a marker component for settings the format supports, like
`SourcePointSize`. The sidebar offers a control only when the selected source
carries the matching component, so a format never shows a setting that means
nothing to it.

## Step 8: Verify

Use the `verify` skill. Confirm the new source gets a frame, streams as the
view moves, and reports sensible counts at startup.
