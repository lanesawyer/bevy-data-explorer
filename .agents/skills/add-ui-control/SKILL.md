---
name: add-ui-control
description: 'Add UI to bevy-data-explorer the way it already works: a sidebar section or control, a dock, or chrome over the frame grid. Covers Feathers events, cursors, input blocking, BSN patching, section order and schedule stages. Use when: adding a button, slider, checkbox, menu, sidebar section, dock or panel, or any control over the frames.'
argument-hint: 'What the control does, and where it lives'
---

# Add UI Control

Each of these rules has already cost a bug here, and most of them build
cleanly and fail silently. Follow them from the start. `debug-bevy-ui` is for
when something has already gone wrong.

## Step 1: Decide where it lives

- **A setting on the selected frame's dataset** goes in "View configuration"
  (`src/ui/view_config.rs`), or in the cell panel if it's about cells.
- **A new group of controls** is a new sidebar section: its own module under
  `src/ui/` and its own plugin.
- **A panel hung from a window edge** is a dock (step 5).
- **Something drawn per frame over the grid** is frame chrome (`src/view/`).
- **A generic control used by both `view` and `ui`** goes in `src/widgets/`.
  `widgets` must not import `ui`.

## Step 2: Build it from the shared widgets

Use Feathers controls and the helpers `src/widgets/` exports, one widget to a
file: `spawn_accordion`, `spawn_menu`, `spawn_icon_menu`, `spawn_modal` (a
screen over the whole window, like help and settings), `spawn_slider`,
`button_icon`, `button_text`, `caption`, `link_button`. A new generic widget
gets a file of its own there, re-exported from `widgets/mod.rs`.

- **Sliders** come from `spawn_slider`, never raw `FeathersSlider`. It adds
  the `SliderPrecision` that Feathers leaves out, without which the slider
  never redraws, and snaps the value to where the track was clicked.
- **Button captions** use `button_text`, not `label`, or the text stays dark
  on a dark button in the light theme.
- **Icons** come from `Icon` (Lucide). The bundled text font has no icon
  glyphs, so anything else renders as a question mark.
- **Colors** are theme tokens (`ThemeBackgroundColor`, `ThemeTextColor`),
  never literals. Otherwise the control ignores a theme switch.
- **Text** comes from `text(content, size::SMALL)` and `text_dim(...)` in
  `widgets/text.rs`, with a size from the `size` scale (`SMALL`, `SECONDARY`,
  `BODY`, `FRAME_TITLE`, `DOCK_TITLE`, ...). Never size a Feathers `label`
  with `InheritableFont`, which it ignores, and never with a literal: a size
  worth having is worth naming for where it sits.
- **Spacing** (every gap, padding and margin) comes from `space` in
  `widgets/spacing.rs`, named for what the space is for: `STACKED` for lines
  within one item, `ICON_LABEL` inside a button, `CONTROLS` between controls
  in a row, `LIST_ITEMS` down a long list, `ROWS` between rows of a section,
  `HEADING` to set a heading apart, `GROUPS` between groups, and the
  `*_INSET` names for padding inside an item, a small box, a panel or a
  screen. Never a literal and never a raw `step`: if no name fits, add one
  to `space` pointing at a step and say in its doc comment what it is for.
  `a_spacing_is_named_rather_than_typed` fails `cargo test` on any gap,
  padding or margin written as a number, inline or in a constant named for
  one. Borders and `Val::Px(0.0)` are not spacing and are left alone.
- **BSN**: patch a component with `@Component { @prop: {expr} }`, with the
  `@` on both the component and each prop. A component with private fields
  can't be patched field by field, so supply it whole with
  `template_value(...)`.

## Step 3: React to it correctly

- **Feathers and `bevy_ui_widgets` controls trigger events.** They carry no
  `Interaction`, so `Changed<Interaction>` never fires. Use an observer:
  `On<Activate>` for buttons, `On<ValueChange<bool>>` for checkboxes. Put a
  marker component on the control, and have the observer check it with a
  query and return early otherwise. Sliders here are read the other way: a
  `ControlsPlace` system compares the slider's `SliderValue` with the
  source's component and writes whichever changed.
- **Plain `bevy_ui::Button` / `Interaction`** is only for drag targets
  (resize handles, slider thumbs). Filter on `Changed<Interaction>`, since
  `Pressed` reads true on every frame the button is held.
- **Acting on the selected frame**: resolve the target through
  `SelectedPanel` then `ShowsSource`, as `view_config.rs` does. When the
  selection moves, load the new source's value *into* the control, so the
  previous source's value doesn't leak across (see `sync_opacity_slider`).
- **Capabilities**: show a control only when the selected source carries
  the component it drives (`SourcePointSize`, `SliceStack`, and so on). Hide the row
  with `Display::None` otherwise. Never offer a setting that means nothing
  to the dataset.
- **Don't respawn on value changes.** Build once and write values onto the
  existing entities. Rebuild only when the *structure* changes (a frame
  opened, a property added). Anything respawned flashes, and loses typed text
  and scroll position.

If the control changes what's on screen and should survive a bookmark, it
also needs `add-bookmark-setting`.

## Step 4: Keep the pointer and cursor honest

- **Anything that overlaps the grid needs `BlocksFrameInput`.** If it holds
  no buttons, it also needs an `Interaction` of its own. Otherwise clicks and
  scrolls fall through and pan the frame behind it.
- **Anything purely decorative drawn over a control** (outlines, rules,
  highlights, scrims) needs `Pickable::IGNORE`, or it blocks picking and the
  control underneath looks dead, including its hover.
- **Never write the window's `CursorIcon`.** Put an `EntityCursor` on the
  hovered entity. To hold a cursor through a drag, use `hold_drag_cursor` in
  `widgets/dock.rs`.
- **Positions**: `ComputedNode::size` and `UiGlobalTransform` are *physical*
  pixels, while `Val::Px`, `Node.left` and `Window::cursor_position` are
  *logical*. Convert with `ComputedNode::inverse_scale_factor`. On a display
  with a scale factor of 1 the mistake is invisible.
- Separate UI roots that overlap need a `GlobalZIndex`. There's no implicit
  order between them.
- In rows that can overflow, give controls `flex_shrink: 0` and the text
  beside them `min_width: 0`, or long text squeezes the controls out of sight.

## Step 5: Register it

**A sidebar section** spawns at `Startup` in `Boot::DockContent`, adds itself
as a child of `SidebarContent`, and states its position:

```rust
/// Where it sits and why, relative to the sections around it.
const SECTION_ORDER: u32 = 25;

commands.entity(accordion.section).insert(SectionOrder(SECTION_ORDER));
```

The current order is view configuration 10, layers 15, cell properties 20 and
bookmarks 30. Pick a gap and say in the doc comment why it goes there.

**A dock** implements `widgets::Dock` (a `Resource` and `Component` with a
`Handle` marker, an `EDGE`, and `drag_to`), places `dock_handle(EDGE)` along
its inner edge, and registers with `app.add_dock::<MyDock>()`. That's what
gives it dragging and the held resize cursor. It subtracts its space from
`FrameArea` in `Stage::DockReserve`, so frames never need to know it exists.
`Inspector` and `LogPanel` are the smallest examples.

**Systems** declare a stage in `app/schedule.rs` and never order themselves
against another module's system:

| Work | Stage |
| --- | --- |
| read the pointer or record control state | `ControlsRead` |
| spawn or despawn to match what's shown | `ControlsBuild` |
| position, sync values, show or hide | `ControlsPlace` |
| write through to the source | `ControlsApply` |
| per-frame chrome spawned or despawned | `FrameChrome` |
| per-frame chrome placed | `Chrome` |
| read-only overlay text or tooltips | `Overlay` |

If none of these fits, add a stage and explain in its doc comment why the
boundary is there. Keep chains well under Bevy's limit of 20 systems per tuple,
and split them before they grow toward it.

## Step 6: Verify

Use the `verify` skill, then check by hand in one launch: the control fires,
the hover shows, the cursor is right, clicking it doesn't pan the frame
behind, it follows a change of selected frame, and it survives a theme switch.
If it's a dock, also drag it and collapse it.
