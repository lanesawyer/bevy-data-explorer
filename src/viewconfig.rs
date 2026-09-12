//! The view configuration section of the sidebar.
//!
//! Shows the settings that apply to whichever frame is selected, starting with
//! transparency. The section is built once; its contents re-read the selection
//! every frame, so it follows the frame you last touched rather than being
//! rebuilt when the selection moves.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_ui_widgets::SliderValue;

use crate::datasource::DataSource;
use crate::panel::{SelectedPanel, ShowsSource};
use crate::sidebar::SidebarContent;
use crate::widgets::{spawn_accordion, spawn_slider};

/// How opaque a source's geometry is drawn, on its own entity so that two
/// sources can be faded independently.
#[derive(Component, Clone, Copy)]
pub struct SourceOpacity(pub f32);

impl Default for SourceOpacity {
    fn default() -> Self {
        SourceOpacity(1.0)
    }
}

/// The label naming the selected dataset.
#[derive(Component, Clone, Default)]
pub struct SelectedName;

/// The slider driving the selected source's opacity.
#[derive(Component, Clone, Default)]
pub struct OpacitySlider;

/// The readout beside the opacity slider.
#[derive(Component, Clone, Default)]
pub struct OpacityReadout;

pub fn spawn_view_config(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let (accordion, body) = spawn_accordion(&mut commands, "View configuration", true);
    commands.entity(parent).add_child(accordion);

    let name = commands
        .spawn_scene(bsn! {
            SelectedName
            Text({ String::new() })
            TextFont { font_size: { bevy::text::FontSize::Px(12.0) } }
            TextColor({ Color::srgb(0.70, 0.76, 0.85) })
        })
        .id();

    let label = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                justify_content: { JustifyContent::SpaceBetween },
            }
            Children [
                (
                    Text({ "Transparency".to_string() })
                    TextFont { font_size: { bevy::text::FontSize::Px(12.0) } }
                    TextColor({ Color::srgb(0.82, 0.86, 0.92) })
                ),
                (
                    OpacityReadout
                    Text({ String::new() })
                    TextFont { font_size: { bevy::text::FontSize::Px(12.0) } }
                    TextColor({ Color::srgb(0.62, 0.68, 0.78) })
                ),
            ]
        })
        .id();

    let slider = spawn_slider(&mut commands, 1.0, (0.0, 1.0));
    commands.entity(slider).insert(OpacitySlider);
    commands.entity(body).add_children(&[name, label, slider]);
}

/// Point the slider at the selected source, and write its value back.
///
/// Runs both ways in one system so a change of selection loads that source's
/// opacity into the slider instead of the previous source's value leaking
/// across.
pub fn sync_opacity_slider(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<(&DataSource, Option<&mut SourceOpacity>)>,
    slider: Query<(Entity, &SliderValue), With<OpacitySlider>>,
    mut names: Query<&mut Text, (With<SelectedName>, Without<OpacityReadout>)>,
    mut readouts: Query<&mut Text, (With<OpacityReadout>, Without<SelectedName>)>,
    mut shown: Local<Option<Entity>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0);

    let Ok((slider_entity, value)) = slider.single() else {
        return;
    };
    let Some(source) = source else {
        for mut text in &mut names {
            text.0 = "no frame selected".into();
        }
        return;
    };
    let Ok((data, opacity)) = sources.get_mut(source) else {
        return;
    };

    for mut text in &mut names {
        if text.0 != data.name {
            text.0 = data.name.clone();
        }
    }

    match opacity {
        // Selection moved: load the newly selected source's own value into the
        // widget rather than letting the previous source's value leak across.
        Some(mut opacity) if *shown != Some(source) => {
            commands
                .entity(slider_entity)
                .insert(SliderValue(opacity.0));
            opacity.set_changed();
        }
        Some(mut opacity) => {
            if (opacity.0 - value.0).abs() > f32::EPSILON {
                opacity.0 = value.0;
            }
        }
        None => {
            commands.entity(source).insert(SourceOpacity(value.0));
        }
    }
    *shown = Some(source);

    for mut text in &mut readouts {
        let wanted = format!("{:.0}%", value.0 * 100.0);
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// The tint that fades geometry to `opacity`.
///
/// Fading dims the colour rather than lowering alpha, because alpha compounds
/// with overdraw. A dense point cloud stacks dozens of points on a pixel, and
/// `1 - (1 - a)^n` reaches 99% by eight layers, so an alpha of 0.5 left the
/// sections looking untouched and nothing appeared to happen until roughly 0.1.
/// Dimming fades a layer uniformly however many times it overdraws, and against
/// a dark background looks the same as a single transparent layer would.
///
/// The factor is applied in sRGB so the slider reads perceptually: halfway
/// along looks half as bright.
fn fade_tint(opacity: f32) -> Color {
    let f = opacity.clamp(0.0, 1.0);
    Color::srgb(f, f, f)
}

/// Fade a source's geometry to its opacity.
///
/// Works off the render layer rather than asking each format plugin to apply
/// it, so a new format is faded without writing any code for it.
pub fn apply_opacity(
    sources: Query<(&DataSource, &SourceOpacity), Changed<SourceOpacity>>,
    mut sprites: Query<(&RenderLayers, &mut Sprite)>,
    meshes: Query<(&RenderLayers, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (source, opacity) in &sources {
        let layer = RenderLayers::layer(source.layer);
        let tint = fade_tint(opacity.0);

        for (layers, mut sprite) in &mut sprites {
            if *layers == layer {
                sprite.color = tint;
            }
        }
        for (layers, material) in &meshes {
            if *layers != layer {
                continue;
            }
            if let Some(material) = materials.get_mut(&material.0).as_mut() {
                material.color = tint;
            }
        }
    }
}

/// Fade geometry that arrives after the opacity was last changed.
///
/// Tiles and octree nodes stream in continuously, so anything spawned since the
/// last change would otherwise appear at full opacity.
pub fn apply_opacity_to_new(
    sources: Query<(&DataSource, &SourceOpacity)>,
    mut sprites: Query<(&RenderLayers, &mut Sprite), Added<Sprite>>,
    meshes: Query<(&RenderLayers, &MeshMaterial2d<ColorMaterial>), Added<Mesh2d>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (source, opacity) in &sources {
        if opacity.0 >= 1.0 {
            continue;
        }
        let layer = RenderLayers::layer(source.layer);
        let tint = fade_tint(opacity.0);

        for (layers, mut sprite) in &mut sprites {
            if *layers == layer {
                sprite.color = tint;
            }
        }
        for (layers, material) in &meshes {
            if *layers != layer {
                continue;
            }
            if let Some(material) = materials.get_mut(&material.0).as_mut() {
                material.color = tint;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(colour: Color) -> f32 {
        colour.to_srgba().red
    }

    #[test]
    fn full_opacity_leaves_the_colour_untouched() {
        // The tint multiplies the source colour, so white is a no-op.
        assert_eq!(brightness(fade_tint(1.0)), 1.0);
        assert_eq!(fade_tint(1.0).alpha(), 1.0);
    }

    #[test]
    fn fading_dims_rather_than_going_transparent() {
        // Alpha stays at one at every setting: transparency compounds with
        // overdraw, which is the bug this replaced.
        for opacity in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(fade_tint(opacity).alpha(), 1.0);
        }
    }

    #[test]
    fn the_slider_reads_perceptually() {
        // Halfway along the slider should look about half as bright, which is
        // a factor of one half in sRGB rather than in linear light.
        assert!((brightness(fade_tint(0.5)) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn brightness_rises_with_opacity() {
        let mut previous = -1.0;
        for step in 0..=10 {
            let value = brightness(fade_tint(step as f32 / 10.0));
            assert!(value > previous, "step {step} did not brighten");
            previous = value;
        }
    }

    #[test]
    fn out_of_range_values_clamp() {
        assert_eq!(brightness(fade_tint(-1.0)), 0.0);
        assert_eq!(brightness(fade_tint(4.0)), 1.0);
    }
}
