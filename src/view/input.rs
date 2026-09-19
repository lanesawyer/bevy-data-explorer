//! Routing the pointer: which frame it is over, whether chrome is in the
//! way, and what it is pointing at.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use super::grid::{Drag, active_panel, panel_under_cursor, within_frames};
use super::{FrameArea, Panel, SelectedPanel, ShowsSource};
use crate::source::ViewLimits;
use crate::source::hover::HoverProbe;

/// Whether a text field currently owns the keyboard.
///
/// Read rather than each keyboard shortcut asking about focus itself, because
/// the shortcuts are spread across the grid and two format plugins, and they
/// run in stages either side of the controls. Settled once, in [`Stage::Focus`],
/// so every one of them answers the same way for a whole frame.
///
/// [`Stage::Focus`]: crate::app::schedule::Stage::Focus
#[derive(Resource, Default)]
pub struct TextEntryFocused(pub bool);

/// Record whether what has focus is something being typed into.
pub fn track_text_focus(
    focus: Res<InputFocus>,
    editable: Query<(), With<EditableText>>,
    mut captured: ResMut<TextEntryFocused>,
) {
    let wanted = focus.get().is_some_and(|entity| editable.contains(entity));
    if captured.0 != wanted {
        captured.0 = wanted;
    }
}

/// Put the selected frame's view back where it started.
///
/// A shortcut rather than a gesture, so it follows the selection like every
/// other key — the pointer decides what a drag or a wheel applies to, and it
/// used to decide this as well, which made `r` the one key that acted on a
/// frame nobody had chosen.
pub fn reset_selected_view(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<TextEntryFocused>,
    selected: Res<SelectedPanel>,
    mut panels: Query<
        (
            &mut Transform,
            &mut Projection,
            &ViewLimits,
            Option<&mut super::Orbit>,
        ),
        With<Panel>,
    >,
) {
    if typing.0 || !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    let Some(panel) = selected.0 else { return };
    let Ok((mut transform, mut projection, limits, orbit)) = panels.get_mut(panel) else {
        return;
    };
    // A 3D frame goes back to where it started turning, not out of 3D.
    if let Some(mut orbit) = orbit {
        orbit.reset();
        return;
    }
    let Projection::Orthographic(ortho) = projection.as_mut() else {
        return;
    };
    transform.translation = limits.centre.extend(transform.translation.z);
    ortho.scale = limits.fit_scale;
}

/// Page through the selected frame's stack of slices.
///
/// Acts on the selected frame rather than on every source that has a stack: two
/// specimens open side by side are paged one at a time, and the outline says
/// which one the keys are talking to.
///
/// Volumetric images and sectioned datasets both carry a stack, so arrows,
/// brackets and page keys page either: paging a volume and stepping a
/// specimen's sections are the same gesture, and a viewer where the same thing
/// is done two ways is a viewer with two things to learn.
pub fn page_slice_stack(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<TextEntryFocused>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut stacks: Query<&mut crate::source::stack::SliceStack>,
) {
    if typing.0 {
        return;
    }
    let mut delta = 0i64;
    for (key, step) in [
        (KeyCode::ArrowRight, 1),
        (KeyCode::ArrowLeft, -1),
        (KeyCode::PageDown, 1),
        (KeyCode::PageUp, -1),
        (KeyCode::BracketRight, 1),
        (KeyCode::BracketLeft, -1),
    ] {
        if keys.just_pressed(key) {
            delta += step;
        }
    }
    if delta == 0 {
        return;
    }

    let Some(source) = super::selected_source(&selected, &panels) else {
        return;
    };
    let Ok(stack) = stacks.get(source) else {
        return;
    };

    // Read before writing. Taking the stack mutably marks it changed whether or
    // not it moved, and a change means every tile is loaded again — so paging
    // into the end of a specimen would reload it on every keypress.
    let mut wanted = *stack;
    wanted.step(delta);
    if wanted == *stack {
        return;
    }
    if let Ok(mut stack) = stacks.get_mut(source) {
        *stack = wanted;
    }
}

/// `G` shows every slice of the selected frame's stack at once, or one at a
/// time, for a stack that can be laid out either way.
pub fn toggle_slice_grid(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<TextEntryFocused>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut grids: Query<&mut crate::source::stack::SliceGrid>,
) {
    if typing.0 || !keys.just_pressed(KeyCode::KeyG) {
        return;
    }
    if let Some(source) = super::selected_source(&selected, &panels)
        && let Ok(mut grid) = grids.get_mut(source)
    {
        grid.0 = !grid.0;
    }
}

/// Marks interactive chrome that swallows pointer input before a frame sees it.
///
/// Needed because chrome can overlap the grid — the sidebar's drag handle
/// straddles its own edge — so position alone cannot decide who gets the drag.
#[derive(Component, Clone, Default)]
pub struct BlocksFrameInput;

/// Whether the pointer is over chrome that takes input before a frame sees it.
///
/// Read from the picking hover state rather than from `Interaction`, because
/// the Feathers controls carry no `Interaction` for a hit test to find.
fn pointer_over_chrome(
    hover: &bevy::picking::hover::HoverMap,
    chrome: &Query<(), With<BlocksFrameInput>>,
    parents: &Query<&ChildOf>,
) -> bool {
    hover.values().flat_map(|hits| hits.keys()).any(|hovered| {
        chrome.get(*hovered).is_ok()
            || parents
                .iter_ancestors(*hovered)
                .any(|ancestor| chrome.get(ancestor).is_ok())
    })
}

/// Tell the sources under the pointer where the pointer is.
///
/// The grid knows the cursor and which frame it falls in; only a format plugin
/// knows what is there. This writes the one onto the source entity so the
/// plugin can answer with the other. Only the sources stacked in the frame
/// under the pointer carry a probe, so a plugin resolving hover need not work
/// out whether the pointer is really over one of its own frames.
pub fn probe_hover(
    mut commands: Commands,
    windows: Query<&Window>,
    area: Res<FrameArea>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &Panel, &ShowsSource)>,
    panel_entities: Query<(Entity, &Panel)>,
    hover: Res<bevy::picking::hover::HoverMap>,
    chrome: Query<(), With<BlocksFrameInput>>,
    parents: Query<&ChildOf>,
    probed: Query<Entity, With<HoverProbe>>,
    stacks: Query<&super::FrameLayers>,
    layer_cameras: Query<&ShowsSource, With<super::LayerOf>>,
) {
    let target = probe_target(
        &windows,
        &area,
        &panels,
        &panel_entities,
        &hover,
        &chrome,
        &parents,
    );

    let stack = target.map_or_else(Vec::new, |(source, probe)| {
        super::layers::stacked_sources(
            &ShowsSource(source),
            stacks.get(probe.panel).ok(),
            &layer_cameras,
        )
    });
    for entity in &probed {
        if !stack.contains(&entity) {
            commands.entity(entity).remove::<HoverProbe>();
        }
    }
    if let Some((_, probe)) = target {
        for source in stack {
            commands.entity(source).insert(probe);
        }
    }
}

fn probe_target(
    windows: &Query<&Window>,
    area: &FrameArea,
    panels: &Query<(&Camera, &GlobalTransform, &Projection, &Panel, &ShowsSource)>,
    panel_entities: &Query<(Entity, &Panel)>,
    hover: &bevy::picking::hover::HoverMap,
    chrome: &Query<(), With<BlocksFrameInput>>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, HoverProbe)> {
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    if pointer_over_chrome(hover, chrome, parents) {
        return None;
    }

    // Measured inside the grid, as the pan and zoom controls are, so chrome
    // docked beside it neither reports a hover nor shifts which frame the
    // pointer is over.
    let local = cursor - area.origin;
    if !within_frames(local, area.size) {
        return None;
    }

    let count = panels.iter().count();
    let index = panel_under_cursor(local, area.size, count);
    let (panel_entity, _) = panel_entities
        .iter()
        .find(|(_, panel)| panel.index == index)?;

    let (camera, global, projection, _, shows) = panels.get(panel_entity).ok()?;
    let Projection::Orthographic(ortho) = projection else {
        return None;
    };
    let world = camera.viewport_to_world_2d(global, cursor).ok()?;
    let viewport = camera.logical_viewport_size()?;

    Some((
        shows.0,
        HoverProbe {
            panel: panel_entity,
            world,
            units_per_px: ortho.area.width() / viewport.x.max(1.0),
        },
    ))
}

/// Scroll to zoom about the cursor and drag to pan, in whichever panel the
/// pointer is over.
pub fn panel_controls(
    mut wheel: MessageReader<MouseWheel>,
    mut panels: Query<(
        &Camera,
        &GlobalTransform,
        &mut Transform,
        &mut Projection,
        &Panel,
        &ViewLimits,
        Option<&mut super::Orbit>,
    )>,
    windows: Query<&Window>,
    area: Res<FrameArea>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    hover: Res<bevy::picking::hover::HoverMap>,
    chrome: Query<(), With<BlocksFrameInput>>,
    parents: Query<&ChildOf>,
    panel_entities: Query<(Entity, &Panel)>,
    mut selected: ResMut<SelectedPanel>,
    mut drag: Local<Option<Drag>>,
) {
    let Ok(window) = windows.single() else { return };
    // The right button drags too: it is how a 3D frame is slid about without
    // a middle button, which a trackpad does not have.
    const DRAGGING: [MouseButton; 3] = [MouseButton::Left, MouseButton::Middle, MouseButton::Right];
    let held = buttons.any_pressed(DRAGGING);

    let Some(cursor) = window.cursor_position() else {
        // The pointer left the window. Hold the gesture so it resumes if the
        // pointer comes back with the button still down.
        if !held {
            *drag = None;
        }
        wheel.clear();
        return;
    };

    // A click on a frame's own chrome must not also pan the frame. An existing
    // drag is left alone, so passing over a button mid-stroke does not end it.
    if drag.is_none() && pointer_over_chrome(&hover, &chrome, &parents) {
        wheel.clear();
        return;
    }

    let count = panels.iter().count();
    // Measured inside the grid, so chrome docked beside it neither receives
    // frame input nor shifts which frame the pointer is over.
    let local = cursor - area.origin;
    if drag.is_none() && !within_frames(local, area.size) {
        wheel.clear();
        return;
    }
    let window_size = area.size;

    if !held {
        *drag = None;
    } else if drag.is_none() && buttons.any_just_pressed(DRAGGING) {
        let index = panel_under_cursor(local, window_size, count);
        *drag = Some(Drag {
            panel: index,
            last: cursor,
        });
        selected.0 = panel_entities
            .iter()
            .find(|(_, panel)| panel.index == index)
            .map(|(entity, _)| entity);
    }

    let active = active_panel(*drag, local, window_size, count);

    let mut scroll = 0.0;
    for event in wheel.read() {
        scroll += match event.unit {
            MouseScrollUnit::Line => event.y,
            // Trackpads report pixels; scale them into comparable steps.
            MouseScrollUnit::Pixel => event.y / 50.0,
        };
    }

    for (camera, global, mut transform, mut projection, panel, limits, orbit) in &mut panels {
        if panel.index != active {
            continue;
        }
        // The same gestures, read as moving about a volume: a drag turns it,
        // any other drag slides it, and the wheel moves toward whatever is
        // under the pointer, as it does in a flat frame.
        if let Some(mut orbit) = orbit {
            if scroll != 0.0 {
                match camera.viewport_to_world(global, cursor) {
                    Ok(ray) => orbit.zoom_towards(scroll, ray),
                    Err(_) => orbit.zoom(scroll),
                }
            }
            if let Some(state) = *drag {
                let delta = cursor - state.last;
                if delta != Vec2::ZERO {
                    let sliding = buttons.any_pressed([MouseButton::Middle, MouseButton::Right])
                        || keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                    if sliding {
                        let height = camera.logical_viewport_size().map_or(1.0, |size| size.y);
                        orbit.pan(delta, height);
                    } else {
                        orbit.turn(delta);
                    }
                }
            }
            continue;
        }
        let Projection::Orthographic(ortho) = projection.as_mut() else {
            continue;
        };

        if scroll != 0.0 {
            let before = camera.viewport_to_world_2d(global, cursor).ok();
            let factor = 1.12_f32.powf(-scroll);
            ortho.scale = (ortho.scale * factor).clamp(limits.min_scale, limits.max_scale);

            // Pin the world point under the cursor. Bevy refreshes the
            // projection's `area` after this system, so the post-zoom mapping
            // is recomputed by hand from the new scale.
            if let (Some(before), Some(viewport)) = (before, camera.logical_viewport_size()) {
                let local = cursor - camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
                let ndc = (local - viewport * 0.5) * Vec2::new(1.0, -1.0);
                let after = transform.translation.truncate() + ndc * ortho.scale;
                let correction = before - after;
                transform.translation.x += correction.x;
                transform.translation.y += correction.y;
            }
        }

        if let Some(state) = *drag {
            let delta = cursor - state.last;
            transform.translation.x -= delta.x * ortho.scale;
            transform.translation.y += delta.y * ortho.scale;
        }
    }

    if let Some(state) = drag.as_mut() {
        state.last = cursor;
    }
}
