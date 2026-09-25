use bevy::prelude::*;
use bevy_feathers::controls::FeathersSlider;
use bevy_ui_widgets::{Slider, SliderPrecision, TrackClick};

use super::BlocksFrameInput;

/// Spawn a slider over `range`, showing its value to `decimals` places.
///
/// Feathers styles and drives it; the app only writes the value back, via
/// `bevy_ui_widgets::slider_self_update`.
///
/// `SliderPrecision` is added explicitly because Feathers' own scene omits it
/// while the system that moves the fill and rewrites the value text requires
/// it. Without it the slider drags but never redraws.
///
/// Feathers hands the track `TrackClick::Drag`, which treats a click anywhere
/// on the track as the start of a drag from wherever the value already was:
/// clicking halfway did not set it to halfway, it just took hold, and the value
/// had to be dragged the rest of the way by hand. `TrackClick::Snap` puts it
/// where it was clicked, which is what a track is for. Dragging from there
/// still works: the drag records the value at the moment it starts, which is
/// the snapped one.
pub fn spawn_slider(
    commands: &mut Commands,
    value: f32,
    range: (f32, f32),
    decimals: i32,
) -> Entity {
    commands
        .spawn_scene(bsn! {
            BlocksFrameInput
            @FeathersSlider {
                @value: { value },
                @min: { range.0 },
                @max: { range.1 }
            }
            // After the scene, so this patches the track behavior it set.
            Slider { track_click: { TrackClick::Snap } }
            SliderPrecision({ decimals })
        })
        .id()
}
