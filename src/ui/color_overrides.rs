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
//!
//! A channel's square opens the same picker, as a [`PickChannelColor`]. What
//! is picked for a channel is written onto the channel itself in
//! [`SourceChannels`], which already keeps what the dataset published to go
//! back to, so it is not listed here: the channel's row is where it is seen.

use bevy::ecs::system::SystemParam;
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
use crate::source::channels::SourceChannels;
use crate::source::properties::{CellProperties, ColorOverrides, Provenance, default_color};
use crate::ui::cell_panel::SWATCH_PX;
use crate::ui::color_export::spawn_export_menu;
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

/// A channel's square, which opens the picker on the color it is painted in.
#[derive(Component, Clone, Default)]
#[require(
    Button,
    BlocksFrameInput,
    BackgroundColor,
    EntityCursor = EntityCursor::System(SystemCursorIcon::Pointer)
)]
pub struct PickChannelColor {
    pub channel: usize,
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

/// What the picker is choosing a color for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// A value of a property, by column and code.
    Value { column: String, code: u16 },
    /// A channel of an image, by its place in the source's channels.
    Channel(usize),
}

/// What the picker is open on, on which source.
#[derive(Resource, Default)]
pub struct Picking {
    source: Option<Entity>,
    target: Option<Target>,
    /// Whether a color has been picked since the picker was last done with
    /// one, and so is worth remembering once it is.
    picked: bool,
}

/// Everything a color can be picked for, read and written the same way
/// whichever it is.
#[derive(SystemParam)]
struct Colors<'w, 's> {
    values: Query<'w, 's, (&'static CellProperties, &'static mut ColorOverrides)>,
    channels: Query<'w, 's, &'static mut SourceChannels>,
}

impl Colors<'_, '_> {
    /// The color `target` is drawn in now.
    fn current(&self, source: Entity, target: &Target) -> Option<Color> {
        match target {
            Target::Value { column, code } => {
                let (properties, overrides) = self.values.get(source).ok()?;
                Some(current(properties, overrides, column, *code))
            }
            Target::Channel(index) => {
                let [r, g, b] = self.channels.get(source).ok()?.channels.get(*index)?.color;
                Some(Color::srgb(r, g, b))
            }
        }
    }

    /// Whether `target` is drawn in a color picked for it rather than its own.
    fn picked(&self, source: Entity, target: &Target) -> bool {
        match target {
            Target::Value { column, code } => self
                .values
                .get(source)
                .is_ok_and(|(_, overrides)| overrides.get(column, *code).is_some()),
            Target::Channel(index) => self.channels.get(source).is_ok_and(|channels| {
                channels.channels.get(*index).map(|it| it.color)
                    != channels.published(*index).map(|it| it.color)
            }),
        }
    }

    fn set(&mut self, source: Entity, target: &Target, color: Color) {
        match target {
            Target::Value { column, code } => {
                if let Ok((_, mut overrides)) = self.values.get_mut(source)
                    && overrides.get(column, *code) != Some(color)
                {
                    overrides.set(column, *code, color);
                }
            }
            Target::Channel(index) => {
                let srgba = color.to_srgba();
                let wanted = [srgba.red, srgba.green, srgba.blue];
                if let Ok(mut channels) = self.channels.get_mut(source)
                    && channels
                        .channels
                        .get(*index)
                        .is_some_and(|it| it.color != wanted)
                {
                    channels.channels[*index].color = wanted;
                }
            }
        }
    }

    /// Put `target` back in its own color.
    fn reset(&mut self, source: Entity, target: &Target) {
        match target {
            Target::Value { column, code } => {
                if let Ok((_, mut overrides)) = self.values.get_mut(source) {
                    overrides.remove(column, *code);
                }
            }
            Target::Channel(index) => {
                if let Ok(mut channels) = self.channels.get_mut(source)
                    && let Some(own) = channels.published(*index).map(|it| it.color)
                {
                    channels.channels[*index].color = own;
                }
            }
        }
    }

    /// How the picker's title names `target`.
    fn describe(&self, source: Entity, target: &Target) -> String {
        match target {
            Target::Value { column, code } => self.values.get(source).map_or_else(
                |_| format!("{column}: code {code}"),
                |(properties, _)| describe(properties, column, *code),
            ),
            Target::Channel(index) => self
                .channels
                .get(source)
                .ok()
                .and_then(|channels| channels.channels.get(*index))
                .map_or_else(|| format!("channel {}", index + 1), |it| it.label.clone()),
        }
    }
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
    spawn_export_menu(&mut commands, accordion.header, true);
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

/// Open the picker under the square pressed, on what it stands for; or close
/// it, if it was already open there.
fn open_picker(
    square: Entity,
    target: Target,
    source: Option<Entity>,
    picking: &mut Picking,
    pickers: &mut Query<(&mut Menu, &mut MenuAnchor), With<ColorPicker>>,
) {
    let Ok((mut menu, mut anchor)) = pickers.single_mut() else {
        return;
    };
    if menu.open && anchor.button == square {
        menu.open = false;
        return;
    }
    // What was picked before stays marked, so moving on from it remembers it
    // as closing the picker would.
    picking.source = source;
    picking.target = Some(target);
    anchor.button = square;
    menu.open = true;
}

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
    let target = Target::Value {
        column: square.column.clone(),
        code: square.code,
    };
    open_picker(
        activate.entity,
        target,
        selected.entity(),
        &mut picking,
        &mut pickers,
    );
}

fn on_pick_channel_color(
    activate: On<Activate>,
    squares: Query<&PickChannelColor>,
    selected: SelectedSource,
    mut picking: ResMut<Picking>,
    mut pickers: Query<(&mut Menu, &mut MenuAnchor), With<ColorPicker>>,
) {
    let Ok(square) = squares.get(activate.entity) else {
        return;
    };
    let target = Target::Channel(square.channel);
    open_picker(
        activate.entity,
        target,
        selected.entity(),
        &mut picking,
        &mut pickers,
    );
}

/// Change the color being picked, starting from the one it is drawn in. Held
/// as HSL so a gray keeps the hue it was dragged to.
fn recolor(picking: &mut Picking, colors: &mut Colors, change: impl FnOnce(&mut Hsla)) {
    let (Some(source), Some(target)) = (picking.source, picking.target.clone()) else {
        return;
    };
    let Some(color) = colors.current(source, &target) else {
        return;
    };
    let mut color = Hsla::from(color);
    change(&mut color);
    colors.set(source, &target, Color::Hsla(color));
    picking.picked = true;
}

fn on_plane(
    change: On<ValueChange<Vec2>>,
    planes: Query<(), With<PickerPlane>>,
    mut picking: ResMut<Picking>,
    mut colors: Colors,
) {
    if planes.get(change.source).is_err() {
        return;
    }
    let value = change.value;
    recolor(&mut picking, &mut colors, |color| {
        color.hue = value.x * 360.0;
        color.saturation = 1.0 - value.y;
    });
}

fn on_lightness(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<PickerLightness>>,
    mut picking: ResMut<Picking>,
    mut colors: Colors,
) {
    if sliders.get(change.source).is_err() {
        return;
    }
    let value = change.value;
    recolor(&mut picking, &mut colors, |color| color.lightness = value);
}

fn on_used_color(
    activate: On<Activate>,
    chips: Query<&UsedColor>,
    mut picking: ResMut<Picking>,
    mut colors: Colors,
) {
    let Ok(UsedColor(used)) = chips.get(activate.entity) else {
        return;
    };
    recolor(&mut picking, &mut colors, |color| {
        *color = Hsla::from(*used);
    });
}

fn on_picker_reset(
    activate: On<Activate>,
    buttons: Query<(), With<PickerReset>>,
    picking: Res<Picking>,
    mut colors: Colors,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    if let (Some(source), Some(target)) = (picking.source, &picking.target) {
        colors.reset(source, target);
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

/// Remember the color something was left in once the picker is done with it:
/// closed, or moved on to something else.
fn remember_picked(
    mut picking: ResMut<Picking>,
    pickers: Query<&Menu, With<ColorPicker>>,
    colors: Colors,
    mut prefs: ResMut<Preferences>,
    mut last: Local<Option<(Entity, Target)>>,
) {
    let open = pickers.single().is_ok_and(|menu| menu.open);
    let now = picking.source.filter(|_| open).zip(picking.target.clone());
    if *last == now {
        return;
    }
    if let Some((source, target)) = last.take()
        && picking.picked
        && colors.picked(source, &target)
        && let Some(color) = colors.current(source, &target)
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

/// Paint every channel's square in the color the channel is painted in.
fn paint_channel_swatches(
    selected: SelectedSource,
    sources: Query<&SourceChannels>,
    mut squares: Query<(&PickChannelColor, &mut BackgroundColor)>,
) {
    let Some(channels) = selected.get(&sources) else {
        return;
    };
    for (square, mut background) in &mut squares {
        if let Some(channel) = channels.channels.get(square.channel) {
            let [r, g, b] = channel.color;
            background.set_if_neq(BackgroundColor(Color::srgb(r, g, b)));
        }
    }
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
            background.set_if_neq(BackgroundColor(mark_color(color)));
        }
    }
}

/// The dot on a picked square: dark on a light color and light on a dark one,
/// so it shows whatever was picked.
fn mark_color(picked: Color) -> Color {
    if Hsla::from(picked).lightness > LIGHT {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// Where the hue and saturation plane's thumb sits for `color`, with its
/// lightness alongside. A gray has no hue to read back, so it keeps the one
/// the thumb is `across` at rather than jumping to the left edge.
fn plane_position(color: Hsla, across: f32) -> Vec3 {
    let across = if color.saturation < GRAY {
        across
    } else {
        color.hue / 360.0
    };
    Vec3::new(across, 1.0 - color.saturation, color.lightness)
}

/// Show the picker as what it is open on stands, and close it when that
/// belongs to a source no longer selected.
#[expect(
    clippy::too_many_arguments,
    reason = "each part of the picker is its own query"
)]
fn sync_picker(
    mut commands: Commands,
    selected: SelectedSource,
    picking: Res<Picking>,
    colors: Colors,
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
        .zip(picking.target.as_ref())
        .and_then(|(source, target)| Some((source, target, colors.current(source, target)?)));
    let Some((source, target, color)) = found else {
        menu.open = false;
        return;
    };
    let hsla = Hsla::from(color);

    for text in &mut titles {
        set_text(text, &colors.describe(source, target));
    }
    for mut swatch in &mut swatches {
        if swatch.0 != color {
            swatch.0 = color;
        }
    }
    for mut plane in &mut planes {
        let wanted = plane_position(hsla, plane.0.x);
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
    let overridden = colors.picked(source, target);
    for node in &mut resets {
        patch_node(node, |node| node.display = display(overridden));
    }
}

pub struct ColorOverridesPlugin;

impl Plugin for ColorOverridesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picking>()
            .add_observer(on_pick_color)
            .add_observer(on_pick_channel_color)
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
                (paint_swatches, paint_channel_swatches, sync_picker)
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Update, remember_picked.in_set(Stage::ControlsApply))
            .add_systems(Startup, spawn_color_overrides.in_set(Boot::DockContent));
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::SystemState;

    use super::*;
    use crate::source::channels::ChannelSetting;
    use crate::source::properties::{CellProperty, PropertyKind, PropertyValue};

    const RED: Color = Color::srgb(1.0, 0.0, 0.0);
    const TEAL: Color = Color::srgb(0.0, 0.5, 0.5);

    /// A source with one categorical column, whose code 1 the publisher
    /// colors red, and an image's two channels.
    fn world() -> (World, Entity) {
        let mut world = World::new();
        let properties = CellProperties::ready(vec![CellProperty {
            id: "CLASS".into(),
            name: "Class".into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Categorical(vec![PropertyValue {
                code: 1,
                label: "Astrocyte".into(),
                reference: None,
                color: Some(RED),
                count: None,
                selected: false,
            }]),
        }]);
        let channels = SourceChannels::new(vec![
            ChannelSetting::new("DAPI", [0.0, 0.0, 1.0], true),
            ChannelSetting::new("GFP", [0.0, 1.0, 0.0], true),
        ]);
        let source = world
            .spawn((properties, ColorOverrides::default(), channels))
            .id();
        (world, source)
    }

    fn value() -> Target {
        Target::Value {
            column: "CLASS".into(),
            code: 1,
        }
    }

    /// Run `work` against the picker's view of `world`.
    fn with_colors<T>(world: &mut World, work: impl FnOnce(&mut Colors) -> T) -> T {
        let mut state = SystemState::<Colors>::new(world);
        let mut colors = state.get_mut(world).unwrap();
        let out = work(&mut colors);
        state.apply(world);
        out
    }

    #[test]
    fn a_value_is_drawn_in_its_own_color_until_one_is_picked() {
        let (mut world, source) = world();
        with_colors(&mut world, |colors| {
            assert_eq!(colors.current(source, &value()), Some(RED));
            assert!(!colors.picked(source, &value()));
            colors.set(source, &value(), TEAL);
            assert_eq!(colors.current(source, &value()), Some(TEAL));
            assert!(colors.picked(source, &value()));
        });
    }

    #[test]
    fn a_reset_value_goes_back_to_its_own_color() {
        let (mut world, source) = world();
        with_colors(&mut world, |colors| {
            colors.set(source, &value(), TEAL);
            colors.reset(source, &value());
            assert_eq!(colors.current(source, &value()), Some(RED));
            assert!(!colors.picked(source, &value()));
        });
    }

    #[test]
    fn a_channel_is_picked_for_and_reset_the_same_way_as_a_value() {
        let (mut world, source) = world();
        let gfp = Target::Channel(1);
        with_colors(&mut world, |colors| {
            assert_eq!(
                colors.current(source, &gfp),
                Some(Color::srgb(0.0, 1.0, 0.0))
            );
            colors.set(source, &gfp, TEAL);
            assert!(colors.picked(source, &gfp));
            colors.reset(source, &gfp);
            assert!(!colors.picked(source, &gfp), "back to how it was published");
            assert_eq!(
                colors.current(source, &gfp),
                Some(Color::srgb(0.0, 1.0, 0.0))
            );
        });
    }

    #[test]
    fn the_picker_names_a_value_by_its_column_and_label_and_a_channel_by_its_label() {
        let (mut world, source) = world();
        with_colors(&mut world, |colors| {
            assert_eq!(colors.describe(source, &value()), "Class: Astrocyte");
            assert_eq!(colors.describe(source, &Target::Channel(0)), "DAPI");
            // What the source does not hold is still named, by where it is.
            assert_eq!(colors.describe(source, &Target::Channel(5)), "channel 6");
            let unknown = Target::Value {
                column: "CLASS".into(),
                code: 9,
            };
            assert_eq!(colors.describe(source, &unknown), "CLASS: code 9");
        });
    }

    #[test]
    fn nothing_is_drawn_for_what_the_source_does_not_hold() {
        let (mut world, source) = world();
        with_colors(&mut world, |colors| {
            assert_eq!(colors.current(source, &Target::Channel(5)), None);
        });
    }

    #[test]
    fn a_gray_keeps_the_hue_its_thumb_was_left_at() {
        let gray = Hsla::hsl(0.0, 0.0, 0.5);
        assert_eq!(plane_position(gray, 0.4).x, 0.4);
        let teal = Hsla::hsl(180.0, 1.0, 0.25);
        assert_eq!(plane_position(teal, 0.4), Vec3::new(0.5, 0.0, 0.25));
    }

    #[test]
    fn a_picked_mark_shows_on_light_and_dark_colors_alike() {
        assert_eq!(mark_color(Color::srgb(1.0, 1.0, 0.8)), Color::BLACK);
        assert_eq!(mark_color(Color::srgb(0.1, 0.1, 0.3)), Color::WHITE);
    }
}
