//! Channel controls in View configuration: for each channel the selected
//! source mixes into colour, whether it is shown and how bright.
//!
//! Nothing here knows it is driving an image. The rows read and write
//! [`SourceChannels`] on the selected source, the way the paging control reads
//! a stack, and appear only for a source that has channels to offer.
//!
//! Brightness is a percentage of how the dataset publishes the channel, rather
//! than a raw intensity: 100 is the dataset's own window whatever its data
//! type, and a raw 1377 out of 65535 would mean nothing to anyone.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy_feathers::containers::{subpane, subpane_body, subpane_header};
use bevy_feathers::controls::{FeathersCheckbox, FeathersToolButton};
use bevy_feathers::theme::ThemedText;
use bevy_ui_widgets::{Activate, SliderValue, ValueChange};

use crate::app::schedule::Stage;
use crate::source::channels::{MAX_GAIN, SourceChannels};
use crate::view::{BlocksFrameInput, SelectedPanel, ShowsSource};
use crate::widgets::{button_text, spawn_slider};

/// Brightness runs 0..400 on the slider, so its readout is a percentage.
const PERCENT: f32 = 100.0;

/// Holds the channel rows, hidden while the selection has no channels.
#[derive(Component, Clone, Default)]
pub struct ChannelSection;

/// Where the rows go, rebuilt whenever the selection's channels differ.
#[derive(Component, Clone, Default)]
pub struct ChannelRows;

/// Shows or hides one channel.
#[derive(Component, Clone, Default)]
pub struct ChannelCheckbox {
    pub channel: usize,
}

/// Puts every channel back as the dataset published it. Always in its place,
/// and disabled while there is nothing to put back.
#[derive(Component, Clone, Default)]
pub struct ChannelReset;

/// Sets one channel's brightness.
#[derive(Component, Clone, Default)]
pub struct ChannelSlider {
    pub channel: usize,
}

/// The section, for View configuration to put in its body.
pub fn spawn_channel_section(commands: &mut Commands) -> Entity {
    // A Feathers sub-pane: the header keeps its height whatever it holds, and
    // the reset sits at its far end, disabled rather than hidden until there is
    // something to put back, so touching a slider moves nothing.
    let title = commands
        .spawn_scene(bsn! { Text("Channels") ThemedText })
        .id();
    let reset = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text("reset")] }
            }
            BlocksFrameInput
            ChannelReset
            InteractionDisabled
        })
        .id();
    let header = commands
        .spawn_scene(bsn! { subpane_header() })
        .add_children(&[title, reset])
        .id();
    let rows = commands
        .spawn_scene(bsn! { subpane_body() ChannelRows })
        .id();
    commands
        .spawn_scene(bsn! {
            subpane()
            ChannelSection
            // Hidden until the selection has channels to show.
            Node { display: { Display::None } }
        })
        .add_children(&[header, rows])
        .id()
}

fn selected_channels<'a>(
    selected: &SelectedPanel,
    panels: &Query<&ShowsSource>,
    sources: &'a Query<&mut SourceChannels>,
) -> Option<(Entity, &'a SourceChannels)> {
    let source = selected.0.and_then(|panel| panels.get(panel).ok())?.0;
    sources.get(source).ok().map(|channels| (source, channels))
}

/// Rebuild the rows when the selection moves to a source with other channels.
///
/// Only then: respawning a Feathers checkbox draws its tick for a frame before
/// it is styled, so rebuilding on every change would flash them all.
pub fn rebuild_channel_rows(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&mut SourceChannels>,
    mut sections: Query<&mut Node, With<ChannelSection>>,
    rows: Query<(Entity, Option<&Children>), With<ChannelRows>>,
    mut built: Local<Option<(Entity, Vec<String>)>>,
) {
    let current = selected_channels(&selected, &panels, &sources);

    let display = if current.is_some_and(|(_, channels)| !channels.channels.is_empty()) {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut sections {
        if node.display != display {
            node.display = display;
        }
    }

    let fingerprint = current.map(|(source, channels)| {
        (
            source,
            channels
                .channels
                .iter()
                .map(|c| c.label.clone())
                .collect::<Vec<_>>(),
        )
    });
    if *built == fingerprint {
        return;
    }
    *built = fingerprint;

    let Ok((container, children)) = rows.single() else {
        return;
    };
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }
    let Some((_, channels)) = current else {
        return;
    };

    for (index, channel) in channels.channels.iter().enumerate() {
        let caption = format!("{} ({})", channel.label, index + 1);
        let [r, g, b] = channel.colour;
        // The channel's own colour, not a theme's: it is what the channel is
        // painted in, and means the same in either theme.
        let swatch = commands
            .spawn((
                Node {
                    width: Val::Px(10.0),
                    height: Val::Px(10.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(r, g, b)),
            ))
            .id();
        let checkbox = commands
            .spawn_scene(bsn! {
                @FeathersCheckbox {
                    @caption: { bsn_list![button_text(caption)] }
                }
                BlocksFrameInput
                ChannelCheckbox { channel: { index } }
            })
            .id();
        if channel.shown {
            commands.entity(checkbox).insert(Checked);
        }
        let heading = commands
            .spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .add_children(&[swatch, checkbox])
            .id();
        let slider = spawn_slider(
            &mut commands,
            channel.gain * PERCENT,
            (0.0, MAX_GAIN * PERCENT),
            0,
        );
        commands
            .entity(slider)
            .insert(ChannelSlider { channel: index });
        commands.entity(container).add_children(&[heading, slider]);
    }
}

/// Show or hide the channel whose box was ticked.
pub fn on_channel_toggled(
    change: On<ValueChange<bool>>,
    checkboxes: Query<&ChannelCheckbox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut SourceChannels>,
) {
    let Ok(checkbox) = checkboxes.get(change.source) else {
        return;
    };
    let Some(mut channels) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(channel) = channels.channels.get_mut(checkbox.channel)
        && channel.shown != change.value
    {
        channel.shown = change.value;
        info!(
            "{} the {} channel",
            if change.value { "showing" } else { "hiding" },
            channel.label
        );
    }
}

/// Put the selected source's channels back as the dataset published them.
pub fn on_channel_reset(
    activate: On<Activate>,
    buttons: Query<(), With<ChannelReset>>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut SourceChannels>,
) {
    if !buttons.contains(activate.entity) {
        return;
    }
    if let Some(mut channels) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    {
        channels.reset();
        info!("channels reset to how the dataset publishes them");
    }
}

/// Keep the controls and the channels in step, whichever moved.
///
/// Brightness goes both ways. A slider that moved since it was last seen was
/// dragged, so its channel follows; otherwise the channel may have been
/// reset, and the slider follows it. Always writing the slider's value back —
/// the simpler thing — would undo every reset the frame after it landed.
pub fn sync_channel_controls(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut SourceChannels>,
    sliders: Query<(Entity, &ChannelSlider, &SliderValue)>,
    boxes: Query<(Entity, &ChannelCheckbox, Has<Checked>)>,
    resets: Query<(Entity, Has<InteractionDisabled>), With<ChannelReset>>,
    mut seen: Local<HashMap<Entity, f32>>,
) {
    let Some(source) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0)
    else {
        return;
    };
    let Ok(mut channels) = sources.get_mut(source) else {
        return;
    };
    seen.retain(|slider, _| sliders.contains(*slider));

    for (entity, slider, value) in &sliders {
        let Some(gain) = channels.channels.get(slider.channel).map(|c| c.gain) else {
            continue;
        };
        let on_channel = gain * PERCENT;
        match seen.get(&entity) {
            Some(last) if (value.0 - last).abs() > f32::EPSILON => {
                // Read before writing, or every frame would mark the channels
                // changed.
                if (on_channel - value.0).abs() > f32::EPSILON {
                    channels.channels[slider.channel].gain = value.0 / PERCENT;
                }
                seen.insert(entity, value.0);
            }
            _ if (on_channel - value.0).abs() > f32::EPSILON => {
                commands.entity(entity).insert(SliderValue(on_channel));
                seen.insert(entity, on_channel);
            }
            _ => {
                seen.insert(entity, value.0);
            }
        }
    }

    for (entity, checkbox, checked) in &boxes {
        let Some(channel) = channels.channels.get(checkbox.channel) else {
            continue;
        };
        if channel.shown != checked {
            if channel.shown {
                commands.entity(entity).insert(Checked);
            } else {
                commands.entity(entity).remove::<Checked>();
            }
        }
    }

    let idle = !channels.changed_from_published();
    for (entity, disabled) in &resets {
        if idle && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !idle && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

/// The channel controls in View configuration.
pub struct ChannelControlsPlugin;

impl Plugin for ChannelControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_channel_toggled)
            .add_observer(on_channel_reset)
            .add_systems(Update, rebuild_channel_rows.in_set(Stage::ControlsBuild))
            .add_systems(Update, sync_channel_controls.in_set(Stage::ControlsPlace));
    }
}
