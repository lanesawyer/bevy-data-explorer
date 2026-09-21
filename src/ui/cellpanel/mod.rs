//! The cell properties section of the sidebar.
//!
//! Offered only by sources that advertise [`CellProperties`], so an image never
//! shows it. Each property becomes a sub-section: the button in its header
//! colors points by that property, and the checkboxes inside filter points
//! down to the values still ticked.
//!
//! Which properties are listed is chosen from the section's own menu, in
//! `visibility`; a dataset offers more of them than are worth reading at once.
//!
//! The section is rebuilt whenever the selected source's properties change, so
//! a lookup that fills them in later — over HTTP, from whatever service knows
//! the labels — needs no cooperation from this module.
//!
//! Each value shows, once a service has counted them, how many cells hold it,
//! and — only under the property the points are actually colored by — the
//! color they are drawn in. A square beside a property nobody is coloring by
//! would name a color that is nowhere on screen.
//!
//! Every other property's values get a bar instead, beside the count and
//! divided between the colors its cells are actually drawn in, largest share
//! first: a legend read the other way round, saying what a donor, a region or
//! a class is made of in the colors already on screen. The service counts the crossing alongside the counts
//! themselves, so the bars follow the filters and empty the moment the
//! coloring moves to another column.
//!
//! Genes are properties too, but are listed in their own section
//! (`ui::genes`), which builds its controls from the same parts.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersCheckbox};
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, ValueChange};

pub mod range;
pub mod tree;
pub mod values;
pub mod visibility;

use crate::app::schedule::{Boot, Stage};
use crate::catalog::cells::{self, Described};
use crate::source::properties::{
    CellColumns, CellProperties, CellProperty, PropertyKind, PropertyState, PropertyValue,
    Provenance,
};
use crate::source::{DataSource, ShowsSource, compact_count};
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::SelectedPanel;
use crate::widgets::{
    Accordion, BlocksFrameInput, Icon, SectionLevel, button_icon, button_text, size,
    spawn_accordion, spawn_header_button, spawn_icon_menu, spawn_menu, spawn_skeleton, text_dim,
};

/// The section itself, hidden for sources with no properties to show.
#[derive(Component, Clone, Default)]
pub struct CellPanel;

/// The section's own menu, for controls that act on all of its properties.
#[derive(Component, Clone, Default)]
pub struct CellPanelMenu;

/// Marks what a rebuild replaces.
///
/// Only the roots of the rebuilt subtrees carry this. Despawning is recursive,
/// so marking something nested inside another marked entity means trying to
/// despawn it twice, which Bevy reports as touching a dead entity.
#[derive(Component, Clone, Default)]
pub struct CellPanelContent;

/// The button that colors points by a property.
#[derive(Component, Clone, Default)]
pub struct ColorByButton {
    pub property: usize,
}

/// The button that drops one property's filters.
#[derive(Component, Clone, Default)]
pub struct ClearPropertyButton {
    pub property: usize,
}

/// The button that asks a failed service again.
#[derive(Component, Clone, Default)]
pub struct RetryButton;

/// The button that drops every filter, on the section's own header.
#[derive(Component, Clone, Default)]
pub struct ClearAllButton;

/// A checkbox admitting one value of one property.
#[derive(Component, Clone, Default)]
pub struct ValueCheckbox {
    pub property: usize,
    pub value: usize,
}

/// The count beside one value, filled in once a service has counted it.
#[derive(Component, Clone, Default)]
pub struct ValueCount {
    pub property: usize,
    pub value: usize,
}

/// Which column and code one value's row stands for.
///
/// The parts of a row that depend on what the points are colored by — its
/// color square and its mix bar — are kept in step from this, without
/// knowing which property or level of the panel they sit under. A tree's
/// levels are columns of their own, so a node carries the level it is on.
#[derive(Component, Clone, Default)]
pub struct ValueColumn {
    pub column: String,
    pub code: u16,
}

/// The color square beside one value, shown only while its points are
/// actually drawn in that color.
#[derive(Component, Clone, Default)]
pub struct ValueSwatch;

/// The bar under one value, divided between the colors the cells holding
/// that value are drawn in.
///
/// Empty until a service has counted the crossing, and empty again the moment
/// the coloring moves to another column. `drawn` is what its segments were
/// last built from, so counts landing rebuild it and a frame where nothing
/// changed does not.
#[derive(Component, Clone, Default)]
pub struct MixBar {
    pub drawn: Option<u64>,
}

/// The most colors one bar is divided into. Past this the largest are kept
/// and the rest are gathered into the remainder: a bar cut into more slices
/// than it has pixels is a smear.
const MAX_SEGMENTS: usize = 16;

/// The least of a bar one color is given a slice of. Below this it would be
/// a fraction of a pixel, and it joins the remainder instead.
const MIN_SEGMENT: f32 = 0.005;

/// How tall the mix bars are, and how wide. Wide enough to read a couple of
/// dozen slices in, narrow enough to leave a sidebar's labels their room.
const BAR_PX: f32 = 6.0;
const BAR_WIDTH_PX: f32 = 56.0;

/// The slot a count is written right-aligned into, so a row with a long one
/// does not push its bar out of line with the rest.
const COUNT_WIDTH_PX: f32 = 34.0;

/// Below the view configuration, which applies to every source.
const SECTION_ORDER: u32 = 20;

/// The most values listed under one property at once. A whole-brain taxonomy
/// has thousands of clusters, and a checkbox apiece makes the sidebar crawl;
/// its coarser levels, or a search, are the way in.
///
/// The table filters hold themselves to the same figure, for the same reason.
pub const MAX_VALUE_ROWS: usize = 300;

/// Placeholder rows shown while the properties are on their way, about as
/// tall as the sub-sections that replace them.
const SKELETON_ROWS: usize = 4;
const SKELETON_ROW_PX: f32 = 26.0;

pub fn spawn_cell_panel(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Cell properties", true, SectionLevel::Pane);
    commands
        .entity(accordion.section)
        .insert(CellPanel)
        .insert(SectionOrder(SECTION_ORDER))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            display: Display::None,
            ..default()
        });
    commands.entity(parent).add_child(accordion.section);
    commands.entity(accordion.body).insert(CellPanelBody);

    // Added before the menu so it sits to its left, and hides itself when
    // there is nothing to clear.
    let clear = spawn_header_button(&mut commands, accordion.header, Icon::FilterX);
    commands.entity(clear).insert(ClearAllButton);

    let menu = spawn_menu(&mut commands, accordion.header);
    commands.entity(menu).insert(CellPanelMenu);
}

/// The body the sub-sections are built into.
#[derive(Component, Clone, Default)]
pub struct CellPanelBody;

/// Rebuild the sub-sections when the selected source's properties change.
pub fn rebuild_cell_panel(
    mut commands: Commands,
    palette: Res<crate::app::theme::Palette>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<(
        &DataSource,
        &CellProperties,
        Has<CellColumns>,
        Has<Described>,
    )>,
    body: Query<Entity, With<CellPanelBody>>,
    mut section: Query<&mut Node, With<CellPanel>>,
    existing: Query<Entity, With<CellPanelContent>>,
    open: Res<OpenSections>,
    mut shown: Local<Option<(Entity, Vec<String>, Provenance, PropertyState, bool)>>,
) {
    let Ok(body) = body.single() else { return };

    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok().map(|found| (shows.0, found)));

    for mut node in &mut section {
        let wanted = if source.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let Some((entity, (_, properties, has_columns, described))) = source else {
        *shown = None;
        return;
    };

    // Still to be read, or read from the files while a service is asked for
    // the real labels. What the files say would be shown for a moment and then
    // swapped out, so placeholders stand in until the answer is final.
    let loading = properties.state == PropertyState::Pending || (has_columns && !described);

    // Rebuilt only when something visible changed: which source, what it is
    // colored by, and which values are ticked.
    // Only what the sections are built from. Everything a property's controls
    // display — ticks, the color choice, range ends — is written onto the
    // existing entities instead, because rebuilding respawns every checkbox and
    // Feathers draws a checkbox's mark before its styling system has had a
    // frame to hide it, which reads as every box flashing ticked.
    //
    // Where the labels came from is part of it: a service's answer can name
    // the same columns as the placeholders it replaces, with other labels.
    let fingerprint = (
        entity,
        properties
            .cell_properties()
            .map(|(_, property)| property.id.clone())
            .collect(),
        properties.provenance.clone(),
        properties.state.clone(),
        loading,
    );
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    if let PropertyState::Failed(error) = &properties.state {
        let failed = spawn_failed(&mut commands, error);
        commands.entity(body).add_child(failed);
        return;
    }
    if loading {
        let skeleton = spawn_skeleton(&mut commands, SKELETON_ROWS, SKELETON_ROW_PX);
        commands.entity(skeleton).insert(CellPanelContent);
        commands.entity(body).add_child(skeleton);
        return;
    }
    if properties.cell_properties().next().is_none() {
        let message = commands
            .spawn_scene(bsn! {
                CellPanelContent
                text_dim("No properties for this dataset.", size::SMALL)
            })
            .id();
        commands.entity(body).add_child(message);
        return;
    }

    let mut sections = Vec::with_capacity(properties.properties.len());
    for (index, property) in properties.cell_properties() {
        let coloring = properties.color_by == Some(index);
        // Sub-sections start closed, since a property can have many values and
        // all of them open at once would bury the rest of the sidebar. One the
        // user opened stays open across a rebuild.
        let was_open = open.0.get(&index).copied().unwrap_or(false);
        let sub = spawn_accordion(
            &mut commands,
            &property.name,
            was_open,
            // A group rather than a pane: these sit inside the section above
            // them, and a second pane header would not say so.
            SectionLevel::Group,
        );
        commands
            .entity(sub.section)
            .insert((CellPanelContent, PropertySection { property: index }));

        // Clearing sits to the left of the color control, as it does on the
        // section's own header. It hides itself when there is nothing to clear.
        let clear = spawn_header_button(&mut commands, sub.header, Icon::FilterX);
        commands
            .entity(clear)
            .insert(ClearPropertyButton { property: index });

        // A tree colors by one of its levels, so its button opens a menu of
        // them.
        let button = if let Some(tree) = property.tree() {
            let (button, menu) = spawn_icon_menu(&mut commands, sub.header, Icon::Palette, true);
            // The popup is a root of its own, not under the section, so a
            // rebuild has to be told to take it too.
            commands.entity(menu).insert(CellPanelContent);
            tree::fill_color_menu(&mut commands, menu, index, tree, coloring);
            button
        } else {
            spawn_header_button(&mut commands, sub.header, Icon::Palette)
        };
        commands
            .entity(button)
            .insert(ColorByButton { property: index });
        if coloring {
            commands.entity(button).insert(ButtonVariant::Primary);
        }

        let rows = match &property.kind {
            PropertyKind::Categorical(_) | PropertyKind::Tree(_) => {
                values::spawn_values(&mut commands, index, property)
            }
            PropertyKind::Numeric(range) => {
                vec![range::spawn_range_control(
                    &mut commands,
                    index,
                    range,
                    properties.ramp().as_ref().filter(|_| coloring),
                    &palette,
                )]
            }
        };
        commands.entity(sub.body).add_children(&rows);
        sections.push(sub.section);
    }
    commands.entity(body).add_children(&sections);
}

/// One value's row: its checkbox, captioned with the color its points are
/// drawn in and its label, then hard against the right edge the bar dividing
/// its cells between the colors on screen and the count of them.
///
/// `checkbox` and `count` mark the two for whatever keeps them in sync;
/// `column` says which column and code the row stands for, which is what the
/// square and the bar are kept in step from. Both start hidden: the square is
/// shown by [`update_property_controls`] only while this column is the one
/// coloring the points, and the bar by [`update_mix_bars`] only while it is
/// not and a service has counted the crossing.
pub fn spawn_value_row(
    commands: &mut Commands,
    value: &PropertyValue,
    checked: bool,
    checkbox: impl Bundle,
    count: impl Bundle,
    column: ValueColumn,
) -> Entity {
    let caption = value.label.clone();
    let color = value.swatch();
    let counted = value.count.map(compact_count).unwrap_or_default();
    let boxed = commands
        .spawn_scene(bsn! {
            @FeathersCheckbox {
                @caption: { bsn_list![
                    (
                        Node {
                            width: { Val::Px(10.0) },
                            height: { Val::Px(10.0) },
                            flex_shrink: { 0.0_f32 },
                            border_radius: { BorderRadius::all(Val::Px(2.0)) },
                            display: { Display::None },
                        }
                        // The value's own color, not a theme's: it is what
                        // its points are painted in.
                        BackgroundColor({ color })
                        ValueSwatch
                        ValueColumn {
                            column: { column.column.clone() },
                            code: { column.code },
                        }
                    ),
                    button_text(caption),
                ] }
            }
            Node { flex_shrink: { 1.0_f32 }, min_width: { Val::ZERO } }
            BlocksFrameInput
        })
        .insert(checkbox)
        .id();
    if checked {
        commands.entity(boxed).insert(Checked);
    }
    let counted = commands
        .spawn_scene(bsn! {
            Text({ counted })
            TextFont { font_size: { FontSize::Px(size::SMALL) } }
            ThemeTextColor({ tokens::TEXT_DIM })
            TextLayout { justify: { Justify::Right } }
            Node { flex_shrink: { 0.0_f32 }, min_width: { Val::Px(COUNT_WIDTH_PX) } }
        })
        .insert(count)
        .id();
    let bar = commands
        .spawn((
            Node {
                width: Val::Px(BAR_WIDTH_PX),
                height: Val::Px(BAR_PX),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(BAR_PX / 2.0)),
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            MixBar::default(),
            column,
        ))
        .id();
    // The bar travels with the count against the right edge, so the bars of
    // a list line up under one another however long the labels are.
    let tail = commands
        .spawn(Node {
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .add_children(&[bar, counted])
        .id();
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .add_children(&[boxed, tail])
        .id()
}

/// How one value's cells divide between the colors on screen: a weight and a
/// color per slice, largest first, so what a value is mostly made of is the
/// first thing the bar says.
///
/// Slices too thin to see are gathered into a remainder at the end, in
/// `rest`, as are all but the largest [`MAX_SEGMENTS`] — a bar with a
/// thousand clusters in it is a smear, and the shape is in its biggest parts.
fn segments(mix: &[(u16, u64)], colors: &HashMap<u16, Color>, rest: Color) -> Vec<(f32, Color)> {
    let total: u64 = mix.iter().map(|(_, count)| *count).sum();
    if total == 0 {
        return Vec::new();
    }
    let fraction = |count: u64| count as f32 / total as f32;
    let mut kept: Vec<(u64, Color)> = mix
        .iter()
        .filter(|(_, count)| fraction(*count) >= MIN_SEGMENT)
        .filter_map(|(code, count)| colors.get(code).map(|color| (*count, *color)))
        .collect();
    // Largest first, ties broken by color so that counting the same answer
    // twice does not shuffle a bar's slices.
    kept.sort_unstable_by_key(|(count, color)| {
        (std::cmp::Reverse(*count), color.to_srgba().to_u8_array())
    });
    kept.truncate(MAX_SEGMENTS);

    let shown: u64 = kept.iter().map(|(count, _)| *count).sum();
    let mut slices: Vec<(f32, Color)> = kept
        .into_iter()
        .map(|(count, color)| (fraction(count), color))
        .collect();
    if fraction(total - shown) >= MIN_SEGMENT {
        slices.push((fraction(total - shown), rest));
    }
    slices
}

/// The note under a branch cut short at [`MAX_VALUE_ROWS`].
pub fn spawn_more_note(commands: &mut Commands, more: usize) -> Entity {
    let note = format!("and {more} more; search to find them");
    commands
        .spawn_scene(bsn! {
            text_dim(note, size::SMALL)
        })
        .id()
}

/// Color points by the property whose header button was pressed. A tree's
/// button only opens its menu of levels; choosing one colors.
pub fn on_color_by(
    activate: On<Activate>,
    buttons: Query<&ColorByButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if !properties
        .properties
        .get(button.property)
        .is_some_and(|property| property.tree().is_none())
    {
        return;
    }
    properties.color_by = Some(button.property);
    if let Some(property) = properties.properties.get(button.property) {
        info!("coloring by {}", property.name);
    }
}

/// Admit or exclude one value of one property.
pub fn on_value_toggled(
    change: On<ValueChange<bool>>,
    checkboxes: Query<&ValueCheckbox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(checkbox) = checkboxes.get(change.source) else {
        return;
    };

    // The box's own `Checked` is deliberately not touched here. Changing the
    // property rebuilds the section, which respawns the box with its state
    // taken from the property — and writing to the old entity would land on
    // one that had just been despawned.
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(value) = properties
        .properties
        .get_mut(checkbox.property)
        .and_then(|property| match &mut property.kind {
            PropertyKind::Categorical(values) => values.get_mut(checkbox.value),
            _ => None,
        })
    {
        value.selected = change.value;
        info!(
            "{} {}",
            if change.value {
                "filtering to"
            } else {
                "no longer filtering to"
            },
            value.label
        );
    }
}

/// Which sub-sections are open, by property.
///
/// Rebuilding despawns the sub-sections, so ticking a checkbox would otherwise
/// close the very section being used. The flag outlives the accordion here.
#[derive(Resource, Default)]
pub struct OpenSections(pub HashMap<usize, bool>);

/// Marks a sub-section with the property it stands for.
#[derive(Component, Clone, Default)]
pub struct PropertySection {
    pub property: usize,
}

pub fn record_open_sections(
    mut open: ResMut<OpenSections>,
    sections: Query<(&PropertySection, &Accordion)>,
) {
    for (section, accordion) in &sections {
        open.0.insert(section.property, accordion.open);
    }
}

/// Drop one property's filters.
pub fn on_clear_property(
    activate: On<Activate>,
    buttons: Query<&ClearPropertyButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    if let Some(property) = properties.properties.get_mut(button.property) {
        property.clear();
        info!("cleared the filters on {}", property.name);
    }
}

/// Why the properties could not be loaded, and a way to ask again.
fn spawn_failed(commands: &mut Commands, error: &str) -> Entity {
    let message = format!("Could not load properties from {error}");
    commands
        .spawn_scene(bsn! {
            CellPanelContent
            Node {
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::Start },
                row_gap: { Val::Px(6.0) },
            }
            Children [
                (
                    text_dim(message, size::SMALL)
                ),
                (
                    @FeathersButton {
                        @caption: { bsn_list![button_icon(Icon::RotateCcw), button_text("Retry")] }
                    }
                    Node { column_gap: { Val::Px(6.0) } }
                    BlocksFrameInput
                    RetryButton
                ),
            ]
        })
        .id()
}

/// Ask the selected source's service about its cells again.
pub fn on_retry(
    activate: On<Activate>,
    buttons: Query<&RetryButton>,
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some((source, mut properties)) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok().map(|found| (shows.0, found)))
    else {
        return;
    };
    info!("retrying the cell properties");
    cells::retry(&mut commands, source, &mut properties);
}

/// Drop every filter on the selected source.
pub fn on_clear_all(
    activate: On<Activate>,
    buttons: Query<&ClearAllButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    let cleared = properties.applied();
    properties.clear_all();
    info!(
        "cleared {cleared} {}",
        if cleared == 1 { "filter" } else { "filters" }
    );
}

/// Show the clear controls only when they have something to clear.
///
/// Both are the icon alone. A count used to be written into the section's
/// button, but into every `Text` under it — the icon's glyph among them, which
/// then drew the words in the icon font as a row of unrelated icons.
pub fn update_clear_buttons(
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    mut per_property: Query<(&ClearPropertyButton, &mut Node), Without<ClearAllButton>>,
    mut clear_all: Query<&mut Node, (With<ClearAllButton>, Without<ClearPropertyButton>)>,
) {
    let properties = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok());

    for (button, mut node) in &mut per_property {
        let applied = properties
            .and_then(|properties| properties.properties.get(button.property))
            .map_or(0, CellProperty::applied);
        let wanted = if applied > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let total = properties.map_or(0, CellProperties::applied);
    for mut node in &mut clear_all {
        let wanted = if total > 0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Keep each checkbox and color control matching its property, without
/// respawning them.
///
/// Ticking a box used to rebuild the whole section. Beyond being wasteful, a
/// fresh Feathers checkbox draws its mark until the styling system runs a frame
/// later, so a rebuild made every box flash ticked before settling.
pub fn update_property_controls(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    boxes: Query<(Entity, &ValueCheckbox, Has<Checked>)>,
    mut colors: Query<(&ColorByButton, &mut ButtonVariant)>,
    mut counts: Query<(&ValueCount, &mut Text)>,
    mut swatches: Query<(&ValueColumn, &mut Node), With<ValueSwatch>>,
) {
    let Some(properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
    else {
        return;
    };

    for (entity, checkbox, checked) in &boxes {
        let selected = properties
            .properties
            .get(checkbox.property)
            .and_then(|property| property.values().get(checkbox.value))
            .is_some_and(|value| value.selected);
        if selected == checked {
            continue;
        }
        if selected {
            commands.entity(entity).insert(Checked);
        } else {
            commands.entity(entity).remove::<Checked>();
        }
    }

    // Counts arrive seconds after the labels, and are written in rather than
    // rebuilt for, for the same reason the ticks are.
    for (count, mut text) in &mut counts {
        let wanted = properties
            .properties
            .get(count.property)
            .and_then(|property| property.values().get(count.value))
            .and_then(|value| value.count)
            .map(compact_count)
            .unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }

    // A value's color square means nothing unless its points are drawn in it,
    // so only the column doing the coloring shows squares — on a tree, only
    // the level it colors by, and nowhere at all under a gradient.
    for (column, mut node) in &mut swatches {
        let wanted = if properties.mix_column() == Some(column.column.as_str()) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    // The one property coloring the points is the one whose palette is lit.
    for (button, mut variant) in &mut colors {
        let wanted = if properties.color_by == Some(button.property) {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        variant.set_if_neq(wanted);
    }
}
/// Divide each value's bar between the colors its cells are drawn in.
///
/// The counts these come from land seconds after the labels and again
/// whenever a filter or the coloring changes, so the segments are rebuilt
/// when they differ and left alone otherwise: a row can hold sixteen of them
/// and a property can hold three hundred rows.
pub fn update_mix_bars(
    mut commands: Commands,
    palette: Res<crate::app::theme::Palette>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    mut bars: Query<(Entity, &ValueColumn, &mut MixBar, &mut Node)>,
) {
    let Some(properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
    else {
        return;
    };

    // What each of the coloring's codes is painted, which is what a slice of
    // a bar is drawn in.
    let against = properties.mix_column();
    let colors: HashMap<u16, Color> = properties
        .color_by
        .and_then(|index| properties.properties.get(index))
        .and_then(CellProperty::color_column)
        .map(|(_, values)| {
            values
                .iter()
                .map(|value| (value.code, value.swatch()))
                .collect()
        })
        .unwrap_or_default();

    for (entity, column, mut bar, mut node) in &mut bars {
        let mix = properties.mixes.of(against, &column.column, column.code);
        let drawn = Some(signature(against, mix));
        if bar.drawn == drawn {
            continue;
        }
        bar.drawn = drawn;
        commands.entity(entity).despawn_related::<Children>();

        let slices = segments(mix, &colors, palette.fill);
        let wanted = if slices.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != wanted {
            node.display = wanted;
        }
        let spawned: Vec<Entity> = slices
            .into_iter()
            .map(|(weight, color)| {
                commands
                    .spawn((
                        Node {
                            flex_grow: weight,
                            flex_basis: Val::Px(0.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(color),
                    ))
                    .id()
            })
            .collect();
        commands.entity(entity).add_children(&spawned);
    }
}

/// What a bar was drawn from, so an answer that says the same thing as the
/// last one does not respawn its slices.
fn signature(against: Option<&str>, mix: &[(u16, u64)]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    against.hash(&mut hasher);
    mix.hash(&mut hasher);
    hasher.finish()
}

/// The sidebar section that filters and colors a point cloud by its cell
/// properties.
pub struct CellPanelPlugin;

impl Plugin for CellPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenSections>()
            .init_resource::<tree::OpenBranches>()
            .add_observer(on_color_by)
            .add_observer(tree::on_toggle)
            .add_observer(tree::on_node_toggled)
            .add_observer(tree::on_color_level)
            .add_observer(on_value_toggled)
            .add_observer(on_clear_property)
            .add_observer(on_clear_all)
            .add_observer(on_retry)
            .add_observer(visibility::on_show_toggled)
            .add_systems(
                Update,
                (record_open_sections, range::drag_range_handles)
                    .chain()
                    .in_set(Stage::ControlsRead),
            )
            .add_systems(
                Update,
                (
                    rebuild_cell_panel,
                    values::sync_value_lists,
                    visibility::rebuild_visibility_menu,
                    tree::sync_branches,
                )
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(
                Update,
                (
                    update_property_controls,
                    update_mix_bars,
                    visibility::update_property_visibility,
                    range::update_range_controls,
                    tree::update_tree_controls,
                    tree::unveil,
                    update_clear_buttons,
                )
                    .chain()
                    .in_set(Stage::ControlsPlace),
            )
            .add_systems(Startup, spawn_cell_panel.in_set(Boot::DockContent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REST: Color = Color::BLACK;

    /// What the coloring paints codes 0..n.
    fn palette(codes: usize) -> HashMap<u16, Color> {
        (0..codes as u16)
            .map(|code| (code, crate::source::properties::default_color(code)))
            .collect()
    }

    #[test]
    fn a_bar_divides_in_proportion_to_the_counts() {
        let colors = palette(3);
        let slices = segments(&[(0, 30), (1, 10)], &colors, REST);
        let weights: Vec<f32> = slices.iter().map(|(weight, _)| *weight).collect();
        assert_eq!(weights, [0.75, 0.25]);
        assert_eq!(slices[0].1, colors[&0]);
    }

    #[test]
    fn a_bar_puts_its_largest_color_first() {
        let colors = palette(3);
        let slices = segments(&[(2, 10), (0, 30), (1, 20)], &colors, REST);
        let drawn: Vec<Color> = slices.iter().map(|(_, color)| *color).collect();
        assert_eq!(drawn, [colors[&0], colors[&1], colors[&2]]);
    }

    #[test]
    fn a_counted_nothing_has_no_bar() {
        assert!(segments(&[], &palette(3), REST).is_empty());
        assert!(segments(&[(0, 0)], &palette(3), REST).is_empty());
    }

    #[test]
    fn slivers_are_gathered_into_the_remainder() {
        let slices = segments(&[(0, 1000), (1, 1)], &palette(2), REST);
        assert_eq!(slices.len(), 1, "a thousandth is under a pixel");
        assert_eq!(slices[0].1, palette(2)[&0]);
    }

    #[test]
    fn a_bar_keeps_the_largest_colors_and_lumps_the_rest() {
        let mix: Vec<(u16, u64)> = (0..40u16)
            .map(|code| (code, 100 - u64::from(code)))
            .collect();
        let slices = segments(&mix, &palette(40), REST);
        assert_eq!(slices.len(), MAX_SEGMENTS + 1, "the rest is one slice");
        assert_eq!(slices[MAX_SEGMENTS].1, REST);
        let total: f32 = slices.iter().map(|(weight, _)| weight).sum();
        assert!((total - 1.0).abs() < 1e-4, "the bar is full: {total}");
    }

    #[test]
    fn a_color_the_panel_does_not_know_falls_into_the_remainder() {
        let slices = segments(&[(0, 50), (99, 50)], &palette(1), REST);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[1].1, REST);
        assert_eq!(slices[1].0, 0.5);
    }
}
