---
name: debug-bevy-ui
description: 'Diagnose Bevy 0.19 UI, input and rendering faults in bevy-data-explorer — dead buttons, misplaced chrome, input reaching the wrong thing, blank or accumulating renders. Use when: a widget does nothing, UI lands in the wrong place, clicks pass through, or something renders wrongly.'
---

# Debug Bevy UI

Check these before reading further code. Each has already caused a bug here,
and most of them build cleanly and fail silently.

## A button does nothing

Which button is it?

- `bevy_ui::Button` drives `Interaction`, so `Changed<Interaction>` works.
- `bevy_ui_widgets` and **all Feathers controls** trigger an `Activate` entity
  event and carry **no `Interaction` component at all**. Polling for a changed
  interaction never fires. Use `app.add_observer(...)` on `On<Activate>` and
  read `activate.entity`.

If it fired once and then stopped, or flickered: `Interaction` reads as
`Pressed` on *every frame* the button is held. Toggling on the value rather
than the transition flips state for the length of the click. Filter with
`Changed<Interaction>`.

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

## Input reaches the frame behind the chrome

Two separate requirements:

- The chrome needs `BlocksFrameInput`, and if it holds no buttons it needs an
  `Interaction` of its own, or it is not detectable as chrome.
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

## A glyph renders as a question mark

The bundled font has no icon coverage. Feathers can draw icons but only from
image assets this crate does not ship. Use words or plain ASCII.

## Something cannot be patched in BSN

Scene components take `@Component { @prop: {expr} }`, with the `@` on both the
component and each prop. Patched components need `Default + Clone`, since a
scene writes fields over defaults. Components with private fields cannot be
patched field by field — supply them whole with `template_value(...)`.
