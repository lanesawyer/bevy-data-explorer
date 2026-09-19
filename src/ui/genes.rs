//! The genes section of the sidebar.
//!
//! Offered only by sources carrying a [`GeneSearch`], which a catalog gives
//! those whose service knows their genes and whose files hold expression. A
//! gene is found by typing the start of its symbol into the same search field
//! the cell properties and the dataset picker use, and added by picking it
//! from what that finds. Each gene added becomes a sub-section like a numeric
//! cell property's — a histogram with a range to filter by, and a button to
//! color by it along the gradient — built from the cell panel's own parts, so
//! the two behave alike without either knowing the other is there.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_feathers::controls::FeathersButton;
use bevy_feathers::display::label_dim;
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::app::theme::Palette;
use crate::source::ShowsSource;
use crate::source::genes::{GeneSearch, SearchState};
use crate::source::properties::CellProperties;
use crate::ui::cellpanel::range::spawn_range_control;
use crate::ui::cellpanel::{ClearPropertyButton, ColorByButton, SMALL_PX};
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::SelectedPanel;
use crate::widgets::{
    Accordion, BlocksFrameInput, Icon, SectionLevel, button_text, field_well, spawn_accordion,
    spawn_header_button, spawn_search_field,
};

/// Under the cell properties, whose numeric controls these are, and above the
/// bookmarks, which apply to everything on screen.
const SECTION_ORDER: u32 = 25;

/// The section itself, hidden for sources with no genes to offer.
#[derive(Component, Clone, Default)]
pub struct GenePanel;

/// The field a gene's symbol is typed into. On the inner text entity, which
/// holds the [`EditableText`].
#[derive(Component, Clone, Default)]
pub struct GeneQueryInput;

/// The line saying how the search is going.
#[derive(Component, Clone, Default)]
pub struct GeneStatus;

/// Where the genes a search found are listed.
#[derive(Component, Clone, Default)]
pub struct GeneResults;

/// Where the genes added are listed, one sub-section each.
#[derive(Component, Clone, Default)]
pub struct GeneList;

/// A gene a search found, by its place in the results.
#[derive(Component, Clone, Default)]
pub struct GeneResultButton {
    pub result: usize,
}

/// The button that drops an added gene, by its place among the properties.
#[derive(Component, Clone, Default)]
pub struct RemoveGeneButton {
    pub property: usize,
}

/// A gene's sub-section, by the gene's id, so whether it is open outlives a
/// rebuild.
#[derive(Component, Clone, Default)]
pub struct GeneSection {
    pub id: String,
}

pub fn spawn_gene_panel(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };

    let accordion = spawn_accordion(&mut commands, "Genes", true, SectionLevel::Pane);
    commands
        .entity(accordion.section)
        .insert(GenePanel)
        .insert(SectionOrder(SECTION_ORDER))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            display: Display::None,
            ..default()
        });
    commands.entity(parent).add_child(accordion.section);

    let search = spawn_search_field(&mut commands, "Search genes, such as Gad1");
    commands.entity(search.field).insert(GeneQueryInput);

    let body = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(4.0) },
            }
        })
        .id();
    // The field would not show against the pane's body on its own.
    let well = commands.spawn_scene(field_well()).id();
    commands.entity(well).add_child(search.entry);
    let status = commands
        .spawn_scene(bsn! {
            GeneStatus
            label_dim("")
            TextFont { font_size: { FontSize::Px(SMALL_PX) } }
            Node { display: { Display::None } }
        })
        .id();
    let results = commands
        .spawn_scene(bsn! {
            GeneResults
            Node {
                width: { Val::Percent(100.0) },
                flex_wrap: { FlexWrap::Wrap },
                column_gap: { Val::Px(4.0) },
                row_gap: { Val::Px(4.0) },
            }
        })
        .id();
    let list = commands
        .spawn_scene(bsn! {
            GeneList
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                // The gap a pane body gives the cell properties' sub-sections.
                row_gap: { Val::Px(6.0) },
                margin: { UiRect::top(Val::Px(4.0)) },
            }
        })
        .id();
    commands
        .entity(body)
        .add_children(&[well, status, results, list]);
    commands.entity(accordion.body).add_child(body);
}

/// The selected frame's source, if it offers genes.
fn selected_source(selected: &SelectedPanel, panels: &Query<&ShowsSource>) -> Option<Entity> {
    selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .map(|shows| shows.0)
}

/// Write what is typed into the selected source's search, and load a newly
/// selected source's query into the field so the last one's does not carry
/// over.
pub fn read_gene_query(
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut GeneSearch>,
    mut fields: Query<&mut EditableText, With<GeneQueryInput>>,
    mut last: Local<Option<Entity>>,
) {
    let source = selected_source(&selected, &panels).filter(|s| sources.contains(*s));
    let Ok(mut field) = fields.single_mut() else {
        return;
    };
    let Some(source) = source else {
        *last = None;
        return;
    };
    let Ok(mut search) = sources.get_mut(source) else {
        return;
    };
    if *last != Some(source) {
        *last = Some(source);
        if field.value().to_string() != search.query {
            field.editor_mut().set_text(&search.query);
        }
        return;
    }
    let typed = field.value().to_string();
    if typed != search.query {
        search.query = typed;
    }
}

/// Add the gene picked from the results.
pub fn on_gene_picked(
    activate: On<Activate>,
    buttons: Query<&GeneResultButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut GeneSearch>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut search) =
        selected_source(&selected, &panels).and_then(|source| sources.get_mut(source).ok())
    else {
        return;
    };
    if let Some(gene) = search.results.get(button.result).cloned() {
        search.add(gene);
    }
}

/// Drop a gene, its filter and, if it colored, its coloring.
pub fn on_remove_gene(
    activate: On<Activate>,
    buttons: Query<&RemoveGeneButton>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(mut properties) =
        selected_source(&selected, &panels).and_then(|source| sources.get_mut(source).ok())
    else {
        return;
    };
    if let Some(property) = properties.properties.get(button.property) {
        info!("removing gene {}", property.name);
    }
    properties.remove_gene(button.property);
}

/// List what the search found, rebuilt only when that changes.
pub fn rebuild_gene_results(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&GeneSearch>,
    list: Query<Entity, With<GeneResults>>,
    mut shown: Local<Option<(Entity, Vec<String>)>>,
) {
    let Ok(list) = list.single() else { return };
    let found = selected_source(&selected, &panels)
        .and_then(|source| sources.get(source).ok().map(|found| (source, found)));
    let fingerprint = found.map(|(source, search)| {
        (
            source,
            search.results.iter().map(|gene| gene.id.clone()).collect(),
        )
    });
    if *shown == fingerprint {
        return;
    }
    *shown = fingerprint;
    commands.entity(list).despawn_children();

    let Some((_, search)) = found else {
        return;
    };
    let buttons: Vec<Entity> = search
        .results
        .iter()
        .enumerate()
        .map(|(result, gene)| {
            let symbol = gene.symbol.clone();
            commands
                .spawn_scene(bsn! {
                    @FeathersButton {
                        @caption: { bsn_list![button_text(symbol)] }
                    }
                    BlocksFrameInput
                    GeneResultButton { result: { result } }
                })
                .id()
        })
        .collect();
    commands.entity(list).add_children(&buttons);
}

/// Build a sub-section for each gene added, rebuilt only when which genes
/// those are changes. Everything inside is kept current by the cell panel's
/// systems, which find its controls by the property they stand for.
pub fn rebuild_gene_list(
    mut commands: Commands,
    palette: Res<Palette>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties, With<GeneSearch>>,
    list: Query<Entity, With<GeneList>>,
    sections: Query<(&GeneSection, &Accordion)>,
    mut open: Local<HashMap<String, bool>>,
    mut shown: Local<Option<(Entity, Vec<String>)>>,
) {
    let Ok(list) = list.single() else { return };
    for (section, accordion) in &sections {
        open.insert(section.id.clone(), accordion.open);
    }

    let found = selected_source(&selected, &panels)
        .and_then(|source| sources.get(source).ok().map(|found| (source, found)));
    let fingerprint = found.map(|(source, properties)| {
        (
            source,
            properties
                .genes()
                .map(|(_, gene)| gene.id.clone())
                .collect(),
        )
    });
    if *shown == fingerprint {
        return;
    }
    *shown = fingerprint;
    commands.entity(list).despawn_children();

    let Some((_, properties)) = found else {
        return;
    };
    let mut sections = Vec::new();
    for (index, gene) in properties.genes() {
        // A gene just added is open, since its histogram is what was asked for.
        let was_open = open.get(&gene.id).copied().unwrap_or(true);
        let sub = spawn_accordion(&mut commands, &gene.name, was_open, SectionLevel::Group);
        commands.entity(sub.section).insert(GeneSection {
            id: gene.id.clone(),
        });

        let clear = spawn_header_button(&mut commands, sub.header, Icon::FilterX);
        commands
            .entity(clear)
            .insert(ClearPropertyButton { property: index });
        let color = spawn_header_button(&mut commands, sub.header, Icon::Palette);
        commands
            .entity(color)
            .insert(ColorByButton { property: index });
        let remove = spawn_header_button(&mut commands, sub.header, Icon::X);
        commands
            .entity(remove)
            .insert(RemoveGeneButton { property: index });

        if let Some(range) = gene.range() {
            let ramp = properties
                .ramp()
                .filter(|_| properties.color_by == Some(index));
            let control = spawn_range_control(&mut commands, index, range, ramp.as_ref(), &palette);
            commands.entity(sub.body).add_child(control);
        }
        sections.push(sub.section);
    }
    commands.entity(list).add_children(&sections);
}

/// Show the section only for a source offering genes, and say how its search
/// is going.
pub fn update_gene_panel(
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&GeneSearch>,
    mut section: Query<&mut Node, (With<GenePanel>, Without<GeneStatus>)>,
    mut status: Query<(&mut Text, &mut Node), (With<GeneStatus>, Without<GenePanel>)>,
) {
    let search = selected_source(&selected, &panels).and_then(|source| sources.get(source).ok());

    for mut node in &mut section {
        let wanted = if search.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }

    let message = search.map(status_of).unwrap_or_default();
    for (mut text, mut node) in &mut status {
        let wanted = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != wanted {
            node.display = wanted;
        }
        if text.0 != message {
            text.0.clone_from(&message);
        }
    }
}

/// The line under the field, or nothing when there is nothing to say.
fn status_of(search: &GeneSearch) -> String {
    if let Some(gene) = search.adding.first() {
        return format!("Adding {}...", gene.symbol);
    }
    match &search.state {
        SearchState::Searching => "Searching...".into(),
        SearchState::Failed(error) => format!("Could not reach the gene service: {error}"),
        SearchState::Idle if !search.query.trim().is_empty() && search.results.is_empty() => {
            format!("No genes start with \"{}\".", search.query.trim())
        }
        SearchState::Idle => String::new(),
    }
}

/// The sidebar section that finds genes and colors and filters by them.
pub struct GenePanelPlugin;

impl Plugin for GenePanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_gene_picked)
            .add_observer(on_remove_gene)
            .add_systems(Update, read_gene_query.in_set(Stage::ControlsRead))
            .add_systems(
                Update,
                (rebuild_gene_results, rebuild_gene_list)
                    .chain()
                    .in_set(Stage::ControlsBuild),
            )
            .add_systems(Update, update_gene_panel.in_set(Stage::ControlsPlace))
            .add_systems(Startup, spawn_gene_panel.in_set(Boot::DockContent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::genes::Gene;

    fn gene(symbol: &str) -> Gene {
        Gene {
            id: symbol.to_lowercase(),
            symbol: symbol.into(),
            index: 0,
            high: 1.0,
        }
    }

    #[test]
    fn an_empty_field_says_nothing() {
        assert_eq!(status_of(&GeneSearch::default()), "");
    }

    #[test]
    fn a_search_that_found_nothing_says_so() {
        let search = GeneSearch {
            query: " zzz ".into(),
            ..default()
        };
        assert_eq!(status_of(&search), "No genes start with \"zzz\".");
    }

    #[test]
    fn a_gene_being_added_is_named_before_anything_else() {
        let mut search = GeneSearch {
            state: SearchState::Searching,
            ..default()
        };
        search.add(gene("Gad1"));
        search.add(gene("Gad1"));
        assert_eq!(search.adding.len(), 1, "asked twice, added once");
        assert_eq!(status_of(&search), "Adding Gad1...");
    }
}
