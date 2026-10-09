---
name: debug-bevy-ui
description: 'Diagnose Bevy 0.20 UI, input and rendering faults in bevy-data-explorer — dead buttons, misplaced chrome, input reaching the wrong thing, blank or accumulating renders. Use when: a widget does nothing, UI lands in the wrong place, clicks pass through, or something renders wrongly.'
---

# Debug Bevy UI

Check these before reading further code. Each has already caused a bug here,
and most of them build cleanly and fail silently.

## A button does nothing

Which button is it?

- `bevy_ui_widgets` and **all Feathers controls** trigger an `Activate` entity
  event. Use `app.add_observer(...)` on `On<Activate>` and read
  `activate.entity`.
- `bevy_ui::Button` and `Interaction` are deprecated in 0.20 and nothing here
  uses them. Polling `Changed<Interaction>` on a Feathers control never fires.

Drag targets that are not buttons — a dock's resize handle — start from an
`On<PointerPress>` observer that checks the entity and sets a `dragging` flag,
then follow the held mouse button until it is released (`widgets/dock.rs`).

## A popup closes as soon as it is clicked

Dismiss logic that asks "is any descendant being interacted with?" is blind to
Feathers controls, so every press inside reads as outside. Closing on
mouse-*down* then removes the button before the mouse-*up* that would have
activated it, so nothing inside can ever fire.

Use the picking `HoverMap` and walk `ChildOf` ancestors instead. Count the
button that opened the popup as inside, or pressing it will dismiss and
immediately reopen.

## UI is in the wrong place, or hit tests miss

Layout reports **physical** pixels: `ComputedNode::size`, `UiGlobalTransform`,
`content_size`. Positions you set and the cursor are **logical**:
`Val::Px`, `Node.left`, `Window::cursor_position`. Convert with
`ComputedNode::inverse_scale_factor`. On a display with a scale factor of one
the bug is invisible, which is how it has survived twice.

Prefer percentages where possible; they need no conversion.

## Chrome is laid out inside one panel

Bevy sizes the UI layout root from its target camera's **viewport**, not the
window. With only viewport cameras present it picks one, and every UI position
is then measured against that panel. Give the UI its own camera with no
viewport and `IsDefaultUiCamera`.

## A widget under a decorative overlay does nothing, not even hover

Picking — which is what `bevy_ui_widgets` and every Feathers control rely on
— respects `Pickable::should_block_lower`, which **blocks** by default. So a
decorative node drawn over a widget leaves it looking completely dead, hover
included. Add
`Pickable::IGNORE` to anything that only draws: selection outlines, rules,
scrims, highlights.

Confirm it by checking the widget for `bevy_ui_widgets::Button`. If the
component is there and the size is right, the scene expanded correctly and the
fault is that nothing is reaching it.

## Input reaches the frame behind the chrome

Two separate requirements:

- The chrome needs `BlocksFrameInput`. It is found through picking's
  `HoverMap`, so it must not be `Pickable::IGNORE`.
- The frame's own bounds test must check **all four edges**. Testing only for
  positions before the grid origin catches left-hand chrome but lets right-hand
  chrome through, where the cell lookup clamps to the last frame and drives it.

## Something is drawn over, or renders accumulate

- Overlapping UI from **separate roots** has no implicit order. Use
  `GlobalZIndex`. A cell border and the rule between cells occupy the same
  pixels, so one will cover the other.
- Only the camera in cell 0 clears the window; the rest draw over it
  deliberately. If frames are reordered or removed and nothing is left
  clearing, every rendered frame lands on top of the last.

## A shader or pipeline fails at runtime

- Declaring `@group(0) @binding(0) view` collides with the binding Bevy's mesh
  imports already provide. Import `bevy_sprite::mesh2d_view_bindings::view`.
- A vertex buffer stride must be a multiple of four. Packing attributes to 18
  bytes is rejected outright.
- `Material2d::specialize` must list attributes in the order the shader's
  locations expect.

## Widgets draw but never update

Check the driving system's query for a component the widget's scene does not
insert. Feathers' slider omits `SliderPrecision`, while the system that moves
the fill and rewrites the text requires it — so the query matches nothing and
the widget keeps its placeholder while the value underneath updates normally.

## Something flashes wrong for a frame before settling

Anything spawned with state that a *later* system derives shows its default
first. A closed accordion body drew its contents once before being hidden; a
Feathers checkbox draws its mark until the styling system runs.

Two fixes, in order of preference:

1. Stop respawning. If a panel rebuilds whenever a value changes, every widget
   in it flashes on every interaction. Rebuild only when the *structure*
   changes and write values onto the existing entities.
2. If it must be spawned, derive the initial state at spawn from the same
   helper the update uses, so the two cannot disagree.

## A control is simply not there

Before hunting the wiring, check whether flexbox removed it. A row that clips
its overflow, holding a growing sibling with long text, shrinks every other
item — flex items shrink by default — until the controls at the end are cut
off. Nothing errors and the entity exists, so queries and observers all look
fine.

Give controls `flex_shrink: 0` and let the text beside them yield with
`min_width: 0` and its own clip.

## Spacing looks uneven

Check the value before the layout. Every gap, padding and margin is a name in
`widgets::space`, so two things spaced differently are either using different
names, which is a choice to revisit at the call site, or one of them is not
going through `space` at all: a Feathers default, or a margin on one child
adding to its parent's gap.

Two causes that are not the value itself:

- **Things placed one by one** rather than laid out in a row drift apart,
  because their widths are assumed rather than measured. A Feathers tool
  button is at least 24px wide (`ROW_HEIGHT`), not whatever a constant says.
  Put siblings in one flex row with one gap.
- **A scrolling area** keeps a lane for its scrollbar on the right whether or
  not the bar shows (`scrollbar_width`, set in `widgets/scrollbar.rs`), so its
  content sits further from the right edge than the left. Take the gutter off
  that side's padding.

## A Feathers control looks unresponsive but works

Its hover is a five percent lift in lightness, which is easy to miss over busy
imagery. Widen `BUTTON_BG_HOVER` and friends on the `UiTheme` rather than
styling controls one at a time, so everything moves together.

## A glyph renders as a question mark

The bundled font has no icon coverage. Feathers can draw icons but only from
image assets this crate does not ship. Use words or plain ASCII.

## Something cannot be patched in BSN

Every scene takes `@`: `@label(text)`, `@{expr}`, and
`@Component { @prop: {expr} }` with the `@` on each prop too. A scene function
called without it is read as a template and fails with a `Template` or
`SceneEffect` bound. List entities are separated by `--`, without parentheses.
Patched components need `Default + Clone`, since a scene writes fields over
defaults. Components with private fields cannot be patched field by field —
supply them whole as `~{value}`.
