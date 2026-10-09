//! Colors picked for values in place of the ones their points are drawn in.
//!
//! Any color square standing for a value — beside it in the cell panel, or in
//! the list here — is a Feathers color input marked [`PickColor`]: pressed, it
//! drops down Feathers' picker, with its wheel, its RGB and HSL sliders and
//! the colors recently picked. There is no way back to a value's own color in
//! there; an override is dropped from the list here, which is where every one
//! of them is seen.
//!
//! The recent colors are Feathers' ([`ColorInputSettings`]), which it adds to
//! as a picker closes. They are kept in the preferences as well, so a palette
//! built up in one session is there in the next.
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
//! A channel's square is the same input, as a [`PickChannelColor`]. What is
//! picked for a channel is written onto the channel itself in
//! [`SourceChannels`], which already keeps what the dataset published to go
//! back to, so it is not listed here: the channel's row is where it is seen,
//! and the channels' reset button where it is undone.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_feathers::controls::{
    ColorInputSettings, ColorInputValue, FeathersColorInput, FeathersToolButton,
};
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ValueChange};

use crate::app::prefs::Preferences;
use crate::app::schedule::{Boot, Stage};
use crate::source::channels::SourceChannels;
use crate::source::properties::{CellProperties, ColorOverrides, Provenance, default_color};
use crate::ui::color_export::spawn_color_export_menu;
use crate::ui::sidebar::{SectionFor, SectionOrder, SidebarContent};
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, SectionLevel, button_icon, size, spawn_accordion, spawn_header_button,
};

/// Just below the cell properties, whose colors these are.
const SECTION_ORDER: u32 = 22;

/// Two colors this close are the same one, written down two ways: the input
/// holds what was dragged, in whichever space it was dragged in, and the
/// override what was kept.
const SAME_COLOR: f32 = 0.5 / 255.0;

/// A square standing for one value's color, picking it when pressed.
#[derive(Component, Clone, Default)]
#[require(BlocksFrameInput)]
pub struct PickColor {
    pub column: String,
    pub code: u16,
}

/// A channel's square, picking the color it is painted in.
#[derive(Component, Clone, Default)]
#[require(BlocksFrameInput)]
pub struct PickChannelColor {
    pub channel: usize,
}

/// A color square: Feathers' color input, for whatever is put beside it to
/// say what it picks.
pub fn color_square() -> impl Scene {
    bsn! {
        @FeathersColorInput
        Node { flex_shrink: { 0.0_f32 } }
    }
}

/// What a square picks a color for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// A value of a property, by column and code.
    Value { column: String, code: u16 },
    /// A channel of an image, by its place in the source's channels.
    Channel(usize),
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
}

/// What the square `entity` picks a color for, if it is one.
fn target_of(
    entity: Entity,
    values: &Query<&PickColor>,
    channels: &Query<&PickChannelColor>,
) -> Option<Target> {
    if let Ok(square) = values.get(entity) {
        return Some(Target::Value {
            column: square.column.clone(),
            code: square.code,
        });
    }
    channels
        .get(entity)
        .ok()
        .map(|square| Target::Channel(square.channel))
}

/// Whether `a` and `b` are the same color, whatever space each is held in.
fn same_color(a: Color, b: Color) -> bool {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    [
        a.red - b.red,
        a.green - b.green,
        a.blue - b.blue,
        a.alpha - b.alpha,
    ]
    .iter()
    .all(|gap| gap.abs() < SAME_COLOR)
}

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

/// How the list names a value: its column, and its label.
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
    spawn_color_export_menu(&mut commands, accordion.header, true);
    let reset = spawn_header_button(&mut commands, accordion.header, Icon::RotateCcw);
    commands.entity(reset).insert(ResetAllOverrides);
}

/// Write what a square's picker chose onto what the square stands for.
///
/// The input is told its own value back at once, as Feathers' own
/// `color_input_self_update` would, so a drag is drawn where it is rather
/// than a frame behind.
fn on_square_picked(
    change: On<ValueChange<Color>>,
    values: Query<&PickColor>,
    channels: Query<&PickChannelColor>,
    selected: SelectedSource,
    mut colors: Colors,
    mut commands: Commands,
) {
    let Some(target) = target_of(change.source, &values, &channels) else {
        return;
    };
    commands
        .entity(change.source)
        .insert(ColorInputValue(change.value));
    if let Some(source) = selected.entity() {
        colors.set(source, &target, change.value);
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
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    if let Some(mut overrides) = selected.get_mut(&mut sources) {
        overrides.clear();
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
                        @color_square()
                        PickColor { column: { column.clone() }, code: { code } }
                        --
                        Text({ label })
                        TextFont { font_size: { FontSize::Px(size::SMALL) } }
                        ThemeTextColor({ tokens::TEXT_MAIN })
                        Node { flex_grow: { 1.0_f32 }, min_width: { Val::ZERO } }
                        --
                        @FeathersToolButton {
                            @caption: { bsn_list! {@button_icon(Icon::X)} }
                        }
                        Node { flex_shrink: { 0.0_f32 } }
                        BlocksFrameInput
                        ResetOverride { column: { column }, code: { code } }
                    ]
                })
                .id()
        })
        .collect();
    commands.entity(body).add_children(&rows);
}

/// Show every square in the color what it stands for is drawn in.
///
/// Written only where it differs, by more than the rounding between color
/// spaces: every write sends Feathers back through the picker's controls, and
/// one a frame would fight a drag.
fn paint_squares(
    mut commands: Commands,
    selected: SelectedSource,
    colors: Colors,
    values: Query<&PickColor>,
    channels: Query<&PickChannelColor>,
    squares: Query<(Entity, &ColorInputValue)>,
) {
    let Some(source) = selected.entity() else {
        return;
    };
    for (square, value) in &squares {
        let Some(wanted) = target_of(square, &values, &channels)
            .and_then(|target| colors.current(source, &target))
        else {
            continue;
        };
        if !same_color(value.0, wanted) {
            commands.entity(square).insert(ColorInputValue(wanted));
        }
    }
}

/// Keep Feathers' recent colors in the preferences, and start it from them.
fn share_recent_colors(
    mut settings: ResMut<ColorInputSettings>,
    mut prefs: ResMut<Preferences>,
    mut started: Local<bool>,
) {
    if !*started {
        *started = true;
        settings.recent_colors = prefs.recent_colors().collect();
        return;
    }
    if settings.is_changed() {
        prefs.keep_recent_colors(&settings.recent_colors);
    }
}

pub struct ColorOverridesPlugin;

impl Plugin for ColorOverridesPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_square_picked)
            .add_observer(on_reset_override)
            .add_observer(on_reset_all)
            .add_systems(Update, rebuild_override_list.in_set(Stage::ControlsBuild))
            .add_systems(Update, paint_squares.in_set(Stage::ControlsPlace))
            .add_systems(Update, share_recent_colors.in_set(Stage::ControlsApply))
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

    /// Run `work` against the squares' view of `world`.
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
            colors.set(source, &value(), TEAL);
            assert_eq!(colors.current(source, &value()), Some(TEAL));
        });
    }

    #[test]
    fn a_channel_is_picked_for_the_same_way_as_a_value() {
        let (mut world, source) = world();
        let gfp = Target::Channel(1);
        with_colors(&mut world, |colors| {
            assert_eq!(
                colors.current(source, &gfp),
                Some(Color::srgb(0.0, 1.0, 0.0))
            );
            colors.set(source, &gfp, TEAL);
            assert!(same_color(colors.current(source, &gfp).unwrap(), TEAL));
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
    fn a_color_dragged_in_hsl_is_the_one_kept_in_rgb() {
        // The input holds what the picker dragged, the override what was
        // kept; writing one over the other every frame would fight the drag.
        let dragged = Color::hsl(180.0, 1.0, 0.25);
        assert!(same_color(dragged, Color::from(dragged.to_srgba())));
        assert!(!same_color(dragged, TEAL.with_alpha(0.5)));
        assert!(!same_color(RED, TEAL));
    }
}
