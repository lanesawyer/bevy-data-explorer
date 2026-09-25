//! Colors picked for values in place of the ones their points are drawn in.
//!
//! Any color square standing for a value — beside it in the cell panel, or in
//! the list here — is a [`PickColor`], and pressing one opens a single picker
//! anchored under it: a hue and saturation plane, a lightness slider, the
//! colors recently picked to choose from again, and a way back to the value's
//! own color.
//!
//! A color becomes a recent one when the picker is done with it — closed, or
//! moved to another value — rather than while it is being dragged, so the row
//! holds what was settled on and not every shade passed through on the way.
//! They are kept in the preferences, so a palette built up in one session is
//! there in the next.
//!
//! What was picked is a [`ColorOverrides`] on the source, by column and code,
//! so it outlives a service replacing the properties and follows the coloring
//! wherever it moves: a color picked for a region waits until the points are
//! colored by region again.
//!
//! The section lists every override on the selected source, each with its
//! square to pick again and a button to drop it, and only appears once there
//! is one.

use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy_feathers::controls::{
    ColorChannel, ColorPlaneValue, ColorSwatchValue, FeathersButton, FeathersColorPlane,
    FeathersColorSlider, FeathersColorSwatch, FeathersToolButton, SliderBaseColor,
};
use bevy_feathers::cursor::EntityCursor;
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, Button, SliderValue, ValueChange};

use crate::app::prefs::Preferences;
use crate::app::schedule::{Boot, Stage};
use crate::source::properties::{CellProperties, ColorOverrides, Provenance, default_color};
use crate::ui::cell_panel::SWATCH_PX;
use crate::ui::sidebar::{SectionFor, SectionOrder, SidebarContent};
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, Menu, MenuAnchor, SectionLevel, button_icon, button_text, display,
    patch_node, set_text, size, spawn_accordion, spawn_header_button, spawn_popup,
};

/// Just below the cell properties, whose colors these are.
const SECTION_ORDER: u32 = 22;

/// Height of the hue and saturation plane, as the filtered-points picker has.
const PLANE_PX: f32 = 100.0;

/// Below this saturation a color has no hue to speak of, and reading one back
/// from it would throw the plane's thumb to the left edge.
const GRAY: f32 = 1e-3;

/// A square standing for one value's color, which opens the picker on it.
#[derive(Component, Clone, Default)]
#[require(
    Button,
    BlocksFrameInput,
    BackgroundColor,
    EntityCursor = EntityCursor::System(SystemCursorIcon::Pointer)
)]
pub struct PickColor {
    pub column: String,
    pub code: u16,
}

/// A dot in the middle of a value's square, saying its color was picked
/// rather than its own. Only the cell panel's squares carry one: everything in
/// the list here is picked.
#[derive(Component, Clone, Default)]
#[require(Pickable::IGNORE, BackgroundColor)]
#[require(Node = Node {
    width: Val::Px(MARK_PX),
    height: Val::Px(MARK_PX),
    border_radius: BorderRadius::all(Val::Px(MARK_PX / 2.0)),
    display: Display::None,
    ..default()
})]
pub struct OverrideMark;

/// Small enough to leave the color around it readable.
const MARK_PX: f32 = 4.0;

/// Above this lightness a mark is drawn dark, and below it light, so it shows
/// on whatever was picked.
const LIGHT: f32 = 0.6;

/// The value the picker is open on, on which source.
#[derive(Resource, Default)]
pub struct Picking {
    source: Option<Entity>,
    column: String,
    code: u16,
    /// Whether a color has been picked since the picker was last done with
    /// one, and so is worth remembering once it is.
    picked: bool,
}

#[derive(Component, Clone, Default)]
pub struct ColorPicker;

#[derive(Component, Clone, Default)]
pub struct PickerTitle;

#[derive(Component, Clone, Default)]
pub struct PickerSwatch;

/// Hue across, saturation down.
#[derive(Component, Clone, Default)]
pub struct PickerPlane;

#[derive(Component, Clone, Default)]
pub struct PickerLightness;

/// The row of colors recently picked, to choose from again.
#[derive(Component, Clone, Default)]
pub struct PickerUsed;

/// One color recently picked.
#[derive(Component, Clone)]
#[require(
    Button,
    BlocksFrameInput,
    EntityCursor = EntityCursor::System(SystemCursorIcon::Pointer)
)]
pub struct UsedColor(Color);

/// Puts the value back to its own color.
#[derive(Component, Clone, Default)]
pub struct PickerReset;

#[derive(Component, Clone, Default)]
pub struct OverridesBody;

/// Marks what a rebuild of the list replaces.
#[derive(Component, Clone, Default)]
pub struct OverrideRow;

/// Drops one override.
#[derive(Component, Clone, Default)]
pub struct ResetOverride {
    column: String,
    code: u16,
}

/// Drops every override on the selected source.
#[derive(Component, Clone, Default)]
pub struct ResetAllOverrides;

/// The color a value is drawn in: what was picked for it, or else its own.
fn current(
    properties: &CellProperties,
    overrides: &ColorOverrides,
    column: &str,
    code: u16,
) -> Color {
    overrides.get(column, code).unwrap_or_else(|| {
        properties
            .value_in(column, code)
            .map_or_else(|| default_color(code), |(_, value)| value.swatch())
    })
}

/// How the list and the picker name a value: its column, and its label.
fn describe(properties: &CellProperties, column: &str, code: u16) -> String {
    match properties.value_in(column, code) {
        Some((name, value)) => format!("{name}: {}", value.label),
        None => format!("{column}: code {code}"),
    }
}

fn spawn_color_overrides(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Color overrides", true, SectionLevel::Pane);
    commands.entity(accordion.section).insert((
        SectionOrder(SECTION_ORDER),
        SectionFor(|source| {
            source
                .get::<ColorOverrides>()
                .is_some_and(|overrides| !overrides.is_empty())
        }),
    ));
    commands.entity(parent).add_child(accordion.section);
    commands.entity(accordion.body).insert(OverridesBody);
    let reset = spawn_header_button(&mut commands, accordion.header, Icon::RotateCcw);
    commands.entity(reset).insert(ResetAllOverrides);

    let picker = spawn_popup(&mut commands);
    commands.entity(picker).insert((
        ColorPicker,
        MenuAnchor {
            button: Entity::PLACEHOLDER,
        },
    ));
    let parts = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                (
                    Text("")
                    TextFont { font_size: { FontSize::Px(size::SECONDARY) } }
                    ThemeTextColor({ tokens::TEXT_MAIN })
                    PickerTitle
                ),
                (
                    @FeathersColorSwatch
                    PickerSwatch
                ),
                (
                    @FeathersColorPlane
                    FeathersColorPlane::HueSaturation
                    Node { height: { Val::Px(PLANE_PX) } }
                    BlocksFrameInput
                    PickerPlane
                ),
                (
                    @FeathersColorSlider {
                        @channel: { ColorChannel::HslLightness }
                    }
                    BlocksFrameInput
                    PickerLightness
                ),
                (
                    Node {
                        flex_wrap: { FlexWrap::Wrap },
                        column_gap: { Val::Px(space::CONTROLS) },
                        row_gap: { Val::Px(space::CONTROLS) },
                        display: { Display::None },
                    }
                    PickerUsed
                ),
                (
                    @FeathersButton {
                        @caption: { bsn_list![
                            button_icon(Icon::RotateCcw),
                            button_text("Use its own color"),
                        ] }
                    }
                    Node { column_gap: { Val::Px(space::ICON_LABEL) } }
                    BlocksFrameInput
                    PickerReset
                ),
            ]
        })
        .id();
    commands.entity(picker).add_child(parts);
}

/// Open the picker under the square pressed, on the value it stands for; or
/// close it, if it was already open there.
fn on_pick_color(
    activate: On<Activate>,
    squares: Query<&PickColor>,
    selected: SelectedSource,
    mut picking: ResMut<Picking>,
    mut pickers: Query<(&mut Menu, &mut MenuAnchor), With<ColorPicker>>,
) {
    let Ok(square) = squares.get(activate.entity) else {
        return;
    };
    let Ok((mut menu, mut anchor)) = pickers.single_mut() else {
        return;
    };
    if menu.open && anchor.button == activate.entity {
        menu.open = false;
        return;
    }
    // What was picked for the value before stays marked, so moving on from it
    // remembers it as closing the picker would.
    picking.source = selected.entity();
    picking.column = square.column.clone();
    picking.code = square.code;
    anchor.button = activate.entity;
    menu.open = true;
}

/// Change the color of the value being picked for, starting from the one it
/// is drawn in. Held as HSL so a gray keeps the hue it was dragged to.
fn recolor(
    picking: &mut Picking,
    sources: &mut Query<(&CellProperties, &mut ColorOverrides)>,
    change: impl FnOnce(&mut Hsla),
) {
    let Some((properties, mut overrides)) = picking
        .source
        .and_then(|source| sources.get_mut(source).ok())
    else {
        return;
    };
    let mut color = Hsla::from(current(
        properties,
        &overrides,
        &picking.column,
        picking.code,
    ));
    change(&mut color);
    let color = Color::Hsla(color);
    if overrides.get(&picking.column, picking.code) != Some(color) {
        overrides.set(&picking.column, picking.code, color);
    }
    picking.picked = true;
}

fn on_plane(
    change: On<ValueChange<Vec2>>,
    planes: Query<(), With<PickerPlane>>,
    mut picking: ResMut<Picking>,
    mut sources: Query<(&CellProperties, &mut ColorOverrides)>,
) {
    if planes.get(change.source).is_err() {
        return;
    }
    let value = change.value;
    recolor(&mut picking, &mut sources, |color| {
        color.hue = value.x * 360.0;
        color.saturation = 1.0 - value.y;
    });
}

fn on_lightness(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<PickerLightness>>,
    mut picking: ResMut<Picking>,
    mut sources: Query<(&CellProperties, &mut ColorOverrides)>,
) {
    if sliders.get(change.source).is_err() {
        return;
    }
    let value = change.value;
    recolor(&mut picking, &mut sources, |color| color.lightness = value);
}

fn on_used_color(
    activate: On<Activate>,
    chips: Query<&UsedColor>,
    mut picking: ResMut<Picking>,
    mut sources: Query<(&CellProperties, &mut ColorOverrides)>,
) {
    let Ok(UsedColor(used)) = chips.get(activate.entity) else {
        return;
    };
    recolor(&mut picking, &mut sources, |color| {
        *color = Hsla::from(*used);
    });
}

fn on_picker_reset(
    activate: On<Activate>,
    buttons: Query<(), With<PickerReset>>,
    picking: Res<Picking>,
    mut sources: Query<&mut ColorOverrides>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    if let Some(mut overrides) = picking
        .source
        .and_then(|source| sources.get_mut(source).ok())
    {
        overrides.remove(&picking.column, picking.code);
    }
}

fn on_reset_override(
    activate: On<Activate>,
    buttons: Query<&ResetOverride>,
    selected: SelectedSource,
    mut sources: Query<&mut ColorOverrides>,
) {
    let Ok(reset) = buttons.get(activate.entity) else {
        return;
    };
    if let Some(mut overrides) = selected.get_mut(&mut sources) {
        overrides.remove(&reset.column, reset.code);
    }
}

fn on_reset_all(
    activate: On<Activate>,
    buttons: Query<(), With<ResetAllOverrides>>,
    selected: SelectedSource,
    mut sources: Query<&mut ColorOverrides>,
    mut pickers: Query<&mut Menu, With<ColorPicker>>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    if let Some(mut overrides) = selected.get_mut(&mut sources) {
        overrides.clear();
    }
    // Its square has just gone with the list.
    for mut menu in &mut pickers {
        menu.open = false;
    }
}

/// List the selected source's overrides, when which ones there are changes.
fn rebuild_override_list(
    mut commands: Commands,
    selected: SelectedSource,
    sources: Query<(&CellProperties, &ColorOverrides)>,
    body: Query<Entity, With<OverridesBody>>,
    existing: Query<Entity, With<OverrideRow>>,
    mut shown: Local<Option<(Entity, Vec<(String, u16)>, Provenance)>>,
) {
    let Ok(body) = body.single() else { return };
    let Some((source, (properties, overrides))) = selected
        .entity()
        .and_then(|source| sources.get(source).ok().map(|found| (source, found)))
    else {
        *shown = None;
        return;
    };
    // The labels are read from the properties, which a service can replace
    // after an override was restored against the files' placeholders.
    let fingerprint = (
        source,
        overrides
            .iter()
            .map(|(column, code, _)| (column.to_string(), code))
            .collect(),
        properties.provenance.clone(),
    );
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for row in &existing {
        commands.entity(row).despawn();
    }
    let rows: Vec<Entity> = overrides
        .iter()
        .map(|(column, code, _)| {
            let label = describe(properties, column, code);
            let column = column.to_string();
            commands
                .spawn_scene(bsn! {
                    OverrideRow
                    Node {
                        width: { Val::Percent(100.0) },
                        align_items: { AlignItems::Center },
                        column_gap: { Val::Px(space::CONTROLS) },
                    }
                    Children [
                        (
                            Node {
                                width: { Val::Px(SWATCH_PX) },
                                height: { Val::Px(SWATCH_PX) },
                                flex_shrink: { 0.0_f32 },
                                border_radius: { BorderRadius::all(Val::Px(2.0)) },
                            }
                            PickColor { column: { column.clone() }, code: { code } }
                        ),
                        (
                            Text({ label })
                            TextFont { font_size: { FontSize::Px(size::SMALL) } }
                            ThemeTextColor({ tokens::TEXT_MAIN })
                            Node { flex_grow: { 1.0_f32 }, min_width: { Val::ZERO } }
                        ),
                        (
                            @FeathersToolButton {
                                @caption: { bsn_list![button_icon(Icon::X)] }
                            }
                            Node { flex_shrink: { 0.0_f32 } }
                            BlocksFrameInput
                            ResetOverride { column: { column }, code: { code } }
                        ),
                    ]
                })
                .id()
        })
        .collect();
    commands.entity(body).add_children(&rows);
}

/// Remember the color a value was left in once the picker is done with it:
/// closed, or moved on to another value.
fn remember_picked(
    mut picking: ResMut<Picking>,
    pickers: Query<&Menu, With<ColorPicker>>,
    sources: Query<&ColorOverrides>,
    mut prefs: ResMut<Preferences>,
    mut last: Local<Option<(Entity, String, u16)>>,
) {
    let open = pickers.single().is_ok_and(|menu| menu.open);
    let now = picking
        .source
        .filter(|_| open)
        .map(|source| (source, picking.column.clone(), picking.code));
    if *last == now {
        return;
    }
    if let Some((source, column, code)) = last.take()
        && picking.picked
        && let Some(color) = sources
            .get(source)
            .ok()
            .and_then(|overrides| overrides.get(&column, code))
    {
        prefs.remember_color(color);
    }
    picking.picked = false;
    *last = now;
}

/// Offer the colors recently picked to pick again. Rebuilt only when they
/// change.
fn rebuild_used_colors(
    mut commands: Commands,
    prefs: Res<Preferences>,
    mut rows: Query<(Entity, &mut Node), With<PickerUsed>>,
    mut shown: Local<Option<Vec<[f32; 3]>>>,
) {
    let Ok((row, node)) = rows.single_mut() else {
        return;
    };
    if shown.as_ref() == Some(&prefs.recent_colors) {
        return;
    }
    patch_node(node, |node| {
        node.display = display(!prefs.recent_colors.is_empty());
    });
    commands.entity(row).despawn_related::<Children>();
    let chips: Vec<Entity> = prefs
        .recent_colors()
        .map(|color| {
            commands
                .spawn((
                    Node {
                        width: Val::Px(SWATCH_PX * 1.5),
                        height: Val::Px(SWATCH_PX * 1.5),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(color),
                    UsedColor(color),
                ))
                .id()
        })
        .collect();
    commands.entity(row).add_children(&chips);
    *shown = Some(prefs.recent_colors.clone());
}

/// Paint every square in the color its value is drawn in, and mark the ones
/// whose color was picked.
fn paint_swatches(
    selected: SelectedSource,
    sources: Query<(&CellProperties, &ColorOverrides)>,
    mut squares: Query<(&PickColor, &mut BackgroundColor), Without<OverrideMark>>,
    mut marks: Query<(&ChildOf, &mut Node, &mut BackgroundColor), With<OverrideMark>>,
) {
    let Some((properties, overrides)) = selected.get(&sources) else {
        return;
    };
    for (square, mut background) in &mut squares {
        let wanted = current(properties, overrides, &square.column, square.code);
        background.set_if_neq(BackgroundColor(wanted));
    }
    for (parent, node, mut background) in &mut marks {
        let Ok(square) = squares.get(parent.parent()) else {
            continue;
        };
        let picked = overrides.get(&square.0.column, square.0.code);
        patch_node(node, |node| node.display = display(picked.is_some()));
        if let Some(color) = picked {
            let mark = if Hsla::from(color).lightness > LIGHT {
                Color::BLACK
            } else {
                Color::WHITE
            };
            background.set_if_neq(BackgroundColor(mark));
        }
    }
}

/// Show the picker as the value it is open on stands, and close it when that
/// value's source is no longer the one selected.
#[expect(
    clippy::too_many_arguments,
    reason = "each part of the picker is its own query"
)]
fn sync_picker(
    mut commands: Commands,
    selected: SelectedSource,
    picking: Res<Picking>,
    sources: Query<(&CellProperties, &ColorOverrides)>,
    mut pickers: Query<&mut Menu, With<ColorPicker>>,
    mut titles: Query<&mut Text, With<PickerTitle>>,
    mut swatches: Query<&mut ColorSwatchValue, With<PickerSwatch>>,
    mut planes: Query<&mut ColorPlaneValue, With<PickerPlane>>,
    mut sliders: Query<(Entity, &SliderValue, &mut SliderBaseColor), With<PickerLightness>>,
    mut resets: Query<&mut Node, With<PickerReset>>,
) {
    let Ok(mut menu) = pickers.single_mut() else {
        return;
    };
    if !menu.open {
        return;
    }
    let found = picking
        .source
        .filter(|source| selected.entity() == Some(*source))
        .and_then(|source| sources.get(source).ok());
    let Some((properties, overrides)) = found else {
        menu.open = false;
        return;
    };
    let color = current(properties, overrides, &picking.column, picking.code);
    let hsla = Hsla::from(color);

    for text in &mut titles {
        set_text(text, &describe(properties, &picking.column, picking.code));
    }
    for mut swatch in &mut swatches {
        if swatch.0 != color {
            swatch.0 = color;
        }
    }
    for mut plane in &mut planes {
        let across = if hsla.saturation < GRAY {
            plane.0.x
        } else {
            hsla.hue / 360.0
        };
        let wanted = Vec3::new(across, 1.0 - hsla.saturation, hsla.lightness);
        if plane.0 != wanted {
            plane.0 = wanted;
        }
    }
    for (entity, value, mut base) in &mut sliders {
        if base.0 != color {
            base.0 = color;
        }
        if (value.0 - hsla.lightness).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(hsla.lightness));
        }
    }
    let overridden = overrides.get(&picking.column, picking.code).is_some();
    for node in &mut resets {
        patch_node(node, |node| node.display = display(overridden));
    }
}

pub struct ColorOverridesPlugin;

impl Plugin for ColorOverridesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picking>()
            .add_observer(on_pick_color)
            .add_observer(on_plane)
            .add_observer(on_lightness)
            .add_observer(on_used_color)
            .add_observer(on_picker_reset)
            .add_observer(on_reset_override)
            .add_observer(on_reset_all)
            .add_systems(
                Update,
                (rebuild_override_list, rebuild_used_colors)
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(
                Update,
                (paint_swatches, sync_picker)
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Update, remember_picked.in_set(Stage::ControlsApply))
            .add_systems(Startup, spawn_color_overrides.in_set(Boot::DockContent));
    }
}
