//! The view configuration section of the sidebar.
//!
//! Shows the settings that apply to whichever frame is selected, starting with
//! transparency. The section is built once; its contents re-read the selection
//! every frame, so it follows the frame you last touched rather than being
//! rebuilt when the selection moves.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_feathers::theme::ThemeTextColor;
use bevy_ui_widgets::Activate;
use bevy_ui_widgets::SliderValue;

use crate::app::schedule::{Boot, Stage};
use crate::formats::EXAMPLES;
use crate::render::points::{
    DEFAULT_POINT_PX, HIGHLIGHT_NONE, MAX_POINT_PX, MIN_POINT_PX, PointMaterial, SourceHighlight,
    SourcePointSize,
};
use crate::source::DataSource;
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::{SelectedPanel, ShowsSource};
use crate::widgets::{button_text, caption, spawn_accordion, spawn_menu, spawn_slider};

/// The opacity slider runs 0..100, so its built-in readout is a percentage.
const PERCENT: f32 = 100.0;

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

/// The slider driving the selected source's point size.
#[derive(Component, Clone, Default)]
pub struct PointSizeSlider;

/// The row holding the point size control, hidden for sources without one.
#[derive(Component, Clone, Default)]
pub struct PointSizeRow;

/// A control that acts on the selected frame, and so has nothing to act on
/// while none is selected.
///
/// The point size row carries its own rule — a source may have no point size to
/// set even when it is selected — and that rule already covers there being no
/// selection at all, so it is not marked with this.
#[derive(Component, Clone, Default)]
pub struct FrameControl;

/// Above the per-dataset sections: it acts on the selected frame whatever
/// that frame is showing.
const SECTION_ORDER: u32 = 10;

pub fn spawn_view_config(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "View configuration", true);
    commands
        .entity(accordion.section)
        .insert(SectionOrder(SECTION_ORDER));
    commands.entity(parent).add_child(accordion.section);
    let menu = spawn_menu(&mut commands, accordion.header);
    commands.entity(menu).insert(LayoutMenu);

    // The frame rows are rebuilt wholesale, so they get a container of their
    // own: the custom dataset field below them holds what has been typed into
    // it, and would lose it every time a frame was opened or closed.
    let rows = commands
        .spawn_scene(bsn! {
            LayoutRows
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(2.0) },
            }
        })
        .id();
    let custom = crate::ui::addsource::spawn_custom_section(&mut commands);
    commands.entity(menu).add_children(&[rows, custom]);

    let body = accordion.body;

    let name = commands
        .spawn_scene(bsn! {
            SelectedName
            Text({ String::new() })
            TextFont { font_size: { bevy::text::FontSize::Px(12.0) } }
            ThemeTextColor({ bevy_feathers::tokens::TEXT_DIM })
        })
        .id();

    let transparency = commands
        .spawn_scene(bsn! {
            FrameControl
            label("Transparency")
            InheritableFont { font_size: { 12.0f32 } }
        })
        .id();

    // Percent rather than a fraction, so the slider's own readout is a whole
    // number that means something without a separate caption beside it.
    let slider = spawn_slider(&mut commands, 100.0, (0.0, PERCENT), 0);
    commands
        .entity(slider)
        .insert((OpacitySlider, FrameControl));

    let size_label = commands
        .spawn_scene(bsn! {
            PointSizeRow
            label("Point size")
            InheritableFont { font_size: { 12.0f32 } }
        })
        .id();
    let size_slider = spawn_slider(
        &mut commands,
        DEFAULT_POINT_PX,
        (MIN_POINT_PX, MAX_POINT_PX),
        1,
    );
    commands
        .entity(size_slider)
        .insert((PointSizeSlider, PointSizeRow));

    commands
        .entity(body)
        .add_children(&[name, transparency, slider, size_label, size_slider]);
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
    mut names: Query<&mut Text, With<SelectedName>>,
    mut controls: Query<&mut Node, With<FrameControl>>,
    mut shown: Local<Option<Entity>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0);

    // A control with nothing to act on is worse than no control: it invites a
    // drag that changes nothing. With no frame selected the section says so and
    // shows nothing else.
    let wanted = if source.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut controls {
        if node.display != wanted {
            node.display = wanted;
        }
    }

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
                .insert(SliderValue(opacity.0 * PERCENT));
            opacity.set_changed();
        }
        Some(mut opacity) => {
            let wanted = value.0 / PERCENT;
            if (opacity.0 - wanted).abs() > f32::EPSILON {
                opacity.0 = wanted;
            }
        }
        None => {
            commands
                .entity(source)
                .insert(SourceOpacity(value.0 / PERCENT));
        }
    }
    *shown = Some(source);
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

/// The sidebar section that acts on the selected frame.
pub struct ViewConfigPlugin;

impl Plugin for ViewConfigPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_layout_button)
            .add_observer(on_add_visualization)
            .add_systems(Update, rebuild_layout_menu.in_set(Stage::ControlsBuild))
            .add_systems(
                Update,
                (sync_opacity_slider, sync_point_size)
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(
                Update,
                (
                    apply_opacity,
                    apply_opacity_to_new,
                    apply_point_settings,
                    apply_point_settings_to_new,
                )
                    .chain()
                    .in_set(Stage::ControlsApply),
            )
            .add_systems(Startup, spawn_view_config.in_set(Boot::DockContent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(colour: Color) -> f32 {
        colour.to_srgba().red
    }

    /// The slider works in percent while opacity is a fraction, so the two
    /// conversions have to agree or the value drifts every time the selection
    /// changes.
    #[test]
    fn percent_and_opacity_round_trip() {
        for opacity in [0.0f32, 0.25, 0.46, 0.5, 1.0] {
            let shown = opacity * PERCENT;
            assert!((shown / PERCENT - opacity).abs() < 1e-6);
        }
        assert_eq!(100.0 / PERCENT, 1.0);
        assert_eq!(0.0 / PERCENT, 0.0);
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

/// The menu that edits which visualizations are on screen.
#[derive(Component, Clone, Default)]
pub struct LayoutMenu;

/// Anything the menu rebuilds, so a rebuild can clear what it made.
#[derive(Component, Clone, Default)]
pub struct LayoutContent;

/// The part of the menu that is rebuilt, which is everything above the custom
/// dataset field.
#[derive(Component, Clone, Default)]
pub struct LayoutRows;

/// What a button in the layout menu does.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum LayoutAction {
    #[default]
    Clone,
    Remove,
}

#[derive(Component, Clone)]
pub struct LayoutButton {
    pub panel: Entity,
    pub action: LayoutAction,
}

impl Default for LayoutButton {
    fn default() -> Self {
        LayoutButton {
            panel: Entity::PLACEHOLDER,
            action: LayoutAction::Clone,
        }
    }
}

/// A row offering to open a frame onto a dataset.
#[derive(Component, Clone)]
pub struct AddVisualization {
    pub source: Entity,
}

impl Default for AddVisualization {
    fn default() -> Self {
        AddVisualization {
            source: Entity::PLACEHOLDER,
        }
    }
}

/// Rebuild the menu when the set of frames changes.
///
/// Rebuilding wholesale rather than reconciling row by row is fine at this
/// size: the grid holds at most eight frames, and the list only changes when
/// one is added or removed.
pub fn rebuild_layout_menu(
    mut commands: Commands,
    menus: Query<Entity, With<LayoutRows>>,
    panels: Query<(Entity, &crate::view::Panel, &ShowsSource)>,
    sources: Query<(Entity, &DataSource)>,
    content: Query<Entity, With<LayoutContent>>,
    load: Res<crate::ui::addsource::CustomLoad>,
    mut previous: Local<Option<Vec<(Entity, Entity)>>>,
    mut listed: Local<usize>,
) {
    let Ok(menu) = menus.single() else { return };

    let mut frames: Vec<(usize, Entity, Entity)> = panels
        .iter()
        .map(|(entity, panel, shows)| (panel.index, entity, shows.0))
        .collect();
    frames.sort_by_key(|(index, _, _)| *index);
    let current: Vec<(Entity, Entity)> = frames
        .iter()
        .map(|(_, panel, source)| (*panel, *source))
        .collect();

    // The datasets the app knows an address for are offered too, and one of
    // them being opened changes what its row says, so that count is part of
    // what the menu is built from.
    let opened = EXAMPLES
        .iter()
        .filter(|example| load.has_opened(example.url))
        .count();
    if previous.as_ref() == Some(&current) && *listed == opened {
        return;
    }
    *previous = Some(current.clone());
    *listed = opened;

    for entity in &content {
        commands.entity(entity).despawn();
    }

    let mut children = vec![heading(&mut commands, "Edit layout", 15.0, 0.0)];

    for (panel, source) in &current {
        let Ok((_, data)) = sources.get(*source) else {
            continue;
        };
        children.push(frame_row(&mut commands, *panel, data));
    }

    children.push(heading(&mut commands, "Add visualization", 12.0, 8.0));
    let full = current.len() >= crate::view::MAX_PANELS;
    for (source, data) in &sources {
        children.push(add_row(&mut commands, source, data, full));
    }

    // Everything else the app knows the address of, so the grid can be filled
    // with the lot without anyone having to paste a URL. A dataset opened this
    // way becomes a source like any other and moves up into the list above.
    children.push(heading(&mut commands, "Open a dataset", 12.0, 8.0));
    for (index, example) in EXAMPLES.iter().enumerate() {
        children.push(example_row(
            &mut commands,
            index,
            example,
            full || load.has_opened(example.url),
        ));
    }

    commands.entity(menu).add_children(&children);
}

fn heading(commands: &mut Commands, text: &str, size: f32, gap: f32) -> Entity {
    let text = text.to_string();
    commands
        .spawn_scene(bsn! {
            LayoutContent
            label(text)
            InheritableFont { font_size: { size as f32 } }
            Node { margin: { UiRect::top(Val::Px(gap)) } }
        })
        .id()
}

/// Name, description and headline figure for one dataset.
fn summary(commands: &mut Commands, data: &DataSource) -> Entity {
    let name = data.name.clone();
    let row = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                flex_grow: { 1.0_f32 },
                row_gap: { Val::Px(1.0) },
            }
            Children [(
                label(name)
                InheritableFont { font_size: { 13.0f32 } }
            )]
        })
        .id();

    // Provenance and headline figure, dimmed the way Feathers dims captions.
    let detail = caption(commands, data.detail.clone());
    let stat = caption(commands, data.stat.clone());
    commands.entity(row).add_children(&[detail, stat]);
    row
}

fn frame_row(commands: &mut Commands, panel: Entity, data: &DataSource) -> Entity {
    let row = commands
        .spawn_scene(bsn! {
            LayoutContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::vertical(Val::Px(6.0)) },
            }
        })
        .id();

    let details = summary(commands, data);
    let clone = action_button(commands, panel, LayoutAction::Clone, "Clone");
    // Every frame offers to close, the last one included: the window it leaves
    // behind offers the examples again.
    let remove = action_button(commands, panel, LayoutAction::Remove, "Close");
    commands.entity(row).add_children(&[details, clone, remove]);
    row
}

/// A dataset the app knows the address of but has not opened: its name, what
/// kind it is, and a button that fetches it.
///
/// The button carries the same component the empty window's examples do, so
/// both go through one observer and one load — a dataset opened from either
/// place arrives by exactly the same path.
fn example_row(
    commands: &mut Commands,
    index: usize,
    example: &crate::formats::Example,
    taken: bool,
) -> Entity {
    let row = commands
        .spawn_scene(bsn! {
            LayoutContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::vertical(Val::Px(4.0)) },
            }
        })
        .id();

    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text("+")] }
            }
            crate::view::BlocksFrameInput
            crate::ui::welcome::ExampleButton { example: { index } }
        })
        .id();
    if taken {
        // Already open, or the grid is full: fetching it again would cost the
        // whole download to end up with the same dataset twice.
        commands.entity(button).insert(InteractionDisabled);
    }

    let name = example.name.to_string();
    let kind = example.kind.to_string();
    let details = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                flex_grow: { 1.0_f32 },
                row_gap: { Val::Px(1.0) },
            }
            Children [
                (
                    label(name)
                    InheritableFont { font_size: { 13.0f32 } }
                ),
                (
                    label_dim(kind)
                    InheritableFont { font_size: { 11.0f32 } }
                ),
            ]
        })
        .id();

    commands.entity(row).add_children(&[button, details]);
    row
}

fn add_row(commands: &mut Commands, source: Entity, data: &DataSource, full: bool) -> Entity {
    let row = commands
        .spawn_scene(bsn! {
            LayoutContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::vertical(Val::Px(4.0)) },
            }
        })
        .id();

    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text("+")] }
            }
            crate::view::BlocksFrameInput
            AddVisualization { source: { source } }
        })
        .id();
    if full {
        // The grid is full, so there is nowhere for another frame to go.
        commands.entity(button).insert(InteractionDisabled);
    }

    let details = summary(commands, data);
    commands.entity(row).add_children(&[button, details]);
    row
}

fn action_button(
    commands: &mut Commands,
    panel: Entity,
    action: LayoutAction,
    glyph: &str,
) -> Entity {
    let glyph = glyph.to_string();
    commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text(glyph)] }
            }
            crate::view::BlocksFrameInput
            LayoutButton { panel: { panel }, action: { action } }
        })
        .id()
}

/// Clone or close a frame from the menu.
///
/// Feathers controls report a press by triggering [`Activate`] on themselves
/// rather than by carrying an `Interaction`, so these are observers rather than
/// systems polling for a changed interaction.
///
/// Both raise the same requests as a frame's own corner buttons, so the two
/// routes cannot drift apart.
pub fn on_layout_button(
    activate: On<Activate>,
    buttons: Query<&LayoutButton>,
    mut requests: MessageWriter<crate::view::PanelRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    requests.write(match button.action {
        LayoutAction::Clone => crate::view::PanelRequest::Duplicate(button.panel),
        LayoutAction::Remove => crate::view::PanelRequest::Close(button.panel),
    });
}

/// Open a new frame onto a dataset.
pub fn on_add_visualization(
    activate: On<Activate>,
    rows: Query<&AddVisualization>,
    mut requests: MessageWriter<crate::view::PanelRequest>,
) {
    let Ok(add) = rows.get(activate.entity) else {
        return;
    };
    requests.write(crate::view::PanelRequest::Open(add.source));
}

/// Point the size slider at the selected source, and write its value back.
///
/// The control is hidden for sources that do not draw points, rather than
/// shown doing nothing.
pub fn sync_point_size(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut SourcePointSize>,
    slider: Query<(Entity, &SliderValue), With<PointSizeSlider>>,
    mut rows: Query<&mut Node, With<PointSizeRow>>,
    mut shown: Local<Option<Entity>>,
) {
    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0);

    let Ok((slider_entity, value)) = slider.single() else {
        return;
    };

    let sized = source.filter(|source| sources.get(*source).is_ok());
    for mut node in &mut rows {
        let wanted = if sized.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Some(source) = sized else {
        *shown = None;
        return;
    };
    let Ok(mut size) = sources.get_mut(source) else {
        return;
    };

    if *shown != Some(source) {
        // Selection moved: load this source's own size rather than carrying
        // the previous source's across.
        commands.entity(slider_entity).insert(SliderValue(size.0));
        size.set_changed();
    } else if (size.0 - value.0).abs() > f32::EPSILON {
        size.0 = value.0;
    }
    *shown = Some(source);
}

/// Push a source's point size, fade and highlight into the materials drawing it.
#[expect(
    clippy::type_complexity,
    reason = "one query over four source settings"
)]
pub fn apply_point_settings(
    sources: Query<
        (
            &DataSource,
            &SourcePointSize,
            Option<&SourceOpacity>,
            Option<&SourceHighlight>,
        ),
        Or<(
            Changed<SourcePointSize>,
            Changed<SourceOpacity>,
            Changed<SourceHighlight>,
        )>,
    >,
    meshes: Query<(&RenderLayers, &MeshMaterial2d<PointMaterial>)>,
    mut materials: ResMut<Assets<PointMaterial>>,
) {
    for (source, size, opacity, highlight) in &sources {
        let layer = RenderLayers::layer(source.layer);
        let tint = fade_tint(opacity.map_or(1.0, |o| o.0)).to_linear();
        let highlight = highlight.map_or(HIGHLIGHT_NONE, SourceHighlight::uniform);

        for (layers, material) in &meshes {
            if *layers != layer {
                continue;
            }
            if let Some(material) = materials.get_mut(&material.0).as_mut() {
                material.settings.size = size.0;
                material.settings.highlight = highlight;
                material.settings.tint = Vec4::new(tint.red, tint.green, tint.blue, tint.alpha);
            }
        }
    }
}

/// Apply the settings to point geometry that arrives after they were last set.
#[expect(
    clippy::type_complexity,
    reason = "one query over four source settings"
)]
pub fn apply_point_settings_to_new(
    sources: Query<(
        &DataSource,
        &SourcePointSize,
        Option<&SourceOpacity>,
        Option<&SourceHighlight>,
    )>,
    meshes: Query<(&RenderLayers, &MeshMaterial2d<PointMaterial>), Added<Mesh2d>>,
    mut materials: ResMut<Assets<PointMaterial>>,
) {
    for (source, size, opacity, highlight) in &sources {
        let layer = RenderLayers::layer(source.layer);
        let tint = fade_tint(opacity.map_or(1.0, |o| o.0)).to_linear();
        let highlight = highlight.map_or(HIGHLIGHT_NONE, SourceHighlight::uniform);

        for (layers, material) in &meshes {
            if *layers != layer {
                continue;
            }
            if let Some(material) = materials.get_mut(&material.0).as_mut() {
                material.settings.size = size.0;
                material.settings.highlight = highlight;
                material.settings.tint = Vec4::new(tint.red, tint.green, tint.blue, tint.alpha);
            }
        }
    }
}
