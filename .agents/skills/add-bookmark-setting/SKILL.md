---
name: add-bookmark-setting
description: 'Make a per-source or per-frame setting survive a bookmark in bevy-data-explorer: a field in SourceState or FrameState, captured in capture.rs, and restored in apply_pending_settings. Use when: adding a control whose state should be saved and shared, a setting that is lost on restore, or a bookmark that restores everything but one thing.'
argument-hint: 'The setting, and the component that holds it'
---

# Add Bookmark Setting

A bookmark saves intent, not entities. It records what the user set up, named
the way you'd describe it to someone else: datasets by address, channels by
label, cell properties by column id and code. Restoring replays that through
`discover` and the frame spawners like anything opened by hand. Entity ids,
render layers and indices into a list the dataset might reorder mean nothing
in another process, so none of them is saved.

Any new control that changes what's on screen should get a pass through this
skill. If it doesn't, a shared link quietly restores everything except that
control.

## Step 1: Decide where it belongs

- **Per dataset** (opacity, point size, slice, channels, cell filters):
  `SourceState` in `src/bookmark/snapshot.rs`.
- **Per frame** (view, orbit, layers): `FrameState`, `ViewState`,
  `OrbitState` or `LayerState`.
- **App preference** (theme, dock widths): leave it out on purpose. A bookmark
  someone sends you shouldn't change your theme.

## Step 2: Add the field

On `SourceState`, a new setting is an `Option`, where absent means "leave
alone":

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub my_setting: Option<MyValue>,
```

- Name the value in terms that survive another process: a column id rather
  than an index, a label rather than a position, a world extent rather than a
  zoom factor (see `ViewState`).
- Nested state gets its own struct deriving `Serialize, Deserialize, Debug,
  Clone, PartialEq`. If it belongs to the cells, it goes in `CellsState`,
  not beside it.

**Versioning:** a field that's merely *added* with `serde(default)` is read as
its default by older files, so `VERSION` stays where it is. Raise `VERSION` only
when an existing field changes meaning. That refuses every older link, so
avoid it unless there's no alternative. Never rename or retype a field in
place.

## Step 3: Capture it

In `source_state` in `src/bookmark/capture.rs`, read it off the source entity:

```rust
my_setting: source.get::<MySetting>().map(|setting| setting.0),
```

Only a source that carries the component gets a value, so a format that
doesn't offer the setting saves nothing for it. Leave out state that's about
to be replaced, as `cells` does while a service is still `Fetching`.

Put conversions that are more than a field read in `snapshot.rs` as a pair,
`x_of` and `apply_x` (see `cells_of` and `apply_cells`), so the two sides sit
next to each other and can be tested together.

## Step 4: Restore it

In `apply_pending_settings` in `src/bookmark/restore.rs`:

```rust
if let (Some(value), Some(mut current)) = (state.my_setting.take(), my_setting) {
    current.0 = clamp_my_setting(value);
}
```

- `take()` the value, so it's applied once.
- **Clamp to what the data allows.** A bookmark can be edited by hand or
  predate a change to the limits. `clamp_point_size` and the span clamp in
  `apply_cells` are the pattern.
- Match things by name. If something the bookmark names no longer exists, skip
  it, return it, and `warn!` with the dataset's name. Never fail the restore.
- Reset before applying when "not saved" should mean "off". `apply_cells`
  calls `clear_all` first so the result is what was saved, not that plus
  whatever was already ticked.

**Settings that depend on something arriving asynchronously** have to wait for
it. Cell filters wait while `PropertyState` isn't `Ready`, or while the source
has `CellColumns` but isn't yet `Described`. If they didn't, the catalog's
service would replace the properties a moment later and throw the filters away.
Such a setting keeps `waiting` true, which leaves `PendingSettings` on the
entity until it lands. It gives up with a warning after `CELLS_PATIENCE_SECS`.

The query in `apply_pending_settings` is already wide. If adding a component
would push it toward Bevy's tuple limit, group the optional components into a
`#[derive(QueryData)]` struct rather than splitting the system in two.

Per-frame settings are restored in `drive_restore` when the frames are laid
out, not here.

## Step 5: Test

Add to the tests in `snapshot.rs`:

- a round trip: `apply_x(x_of(edited))` gives back what was edited;
- a value outside the data's limits comes back clamped;
- something the dataset no longer has is reported, not an error.

Then check that an old bookmark without the field still reads.
`codec::from_text` on a fixture that has no such field should give `None`.

## Step 6: Verify

Use the `verify` skill, then do it by hand once: change the setting, save a
bookmark, change it back, restore, and check that it returns. Do the same
through a shared line (copy, then paste into the bookmarks section). That
path goes through `codec.rs` and catches a field that serializes but doesn't
round-trip.
