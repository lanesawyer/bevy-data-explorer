use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, Pressed};
use bevy_feathers::controls::FeathersSlider;
use bevy_feathers::theme::UiTheme;
use bevy_feathers::tokens;
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

/// Paint every slider in the theme now in place.
///
/// Feathers draws a slider's bar as a gradient it colors itself, and only when
/// the slider is spawned, hovered, pressed or disabled — never when the theme
/// changes. The desktop's accent arrives a moment after the first sliders are
/// built, so they stayed Feathers' own blue until the pointer passed over
/// them, and a slider's track kept the old theme's gray on switching between
/// light and dark. The same tokens Feathers would choose, chosen again.
pub(super) fn repaint_sliders(
    theme: Res<UiTheme>,
    mut sliders: Query<
        (
            Has<InteractionDisabled>,
            Has<Pressed>,
            &Hovered,
            &mut BackgroundGradient,
        ),
        With<FeathersSlider>,
    >,
) {
    if !theme.is_changed() {
        return;
    }
    for (disabled, pressed, hovered, mut gradient) in &mut sliders {
        let (bar, track) = if disabled {
            (tokens::SLIDER_BAR_DISABLED, tokens::SLIDER_BG_DISABLED)
        } else if pressed {
            (tokens::SLIDER_BAR_PRESSED, tokens::SLIDER_BG_PRESSED)
        } else if hovered.0 {
            (tokens::SLIDER_BAR_HOVER, tokens::SLIDER_BG_HOVER)
        } else {
            (tokens::SLIDER_BAR, tokens::SLIDER_BG)
        };
        let (bar, track) = (theme.color(&bar), theme.color(&track));
        if let [Gradient::Linear(linear)] = &mut gradient.0[..]
            && let [first, second, third, fourth] = &mut linear.stops[..]
        {
            first.color = bar;
            second.color = bar;
            third.color = track;
            fourth.color = track;
        }
    }
}
