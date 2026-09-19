//! The section's menu, which picks which properties the panel lists.
//!
//! One checkbox per property the source advertises, ticked when that property
//! appears below. A dataset can offer more properties than the sidebar can
//! usefully hold, and the ones worth looking at differ per question, so which
//! of them are listed is the user's to choose.
//!
//! The property points are colored by cannot be hidden, so its box is drawn
//! unavailable rather than left to bounce back when the source refuses it.
//!
//! Hiding does not rebuild the section. Every property keeps its sub-section
//! and the hidden ones are laid out away, because respawning a Feathers
//! checkbox draws its mark for a frame before the styling system hides it,
//! which reads as the boxes inside flashing ticked.

use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy_feathers::controls::FeathersCheckbox;
use bevy_feathers::display::label;
use bevy_ui_widgets::ValueChange;

use crate::source::properties::CellProperties;
use crate::ui::cellpanel::{CellPanelMenu, PropertySection};
use crate::view::{BlocksFrameInput, SelectedPanel, ShowsSource};
use crate::widgets::button_text;

/// A checkbox in the menu, listing or hiding one property.
#[derive(Component, Clone, Default)]
pub struct ShowPropertyCheckbox {
    pub property: usize,
}

/// Marks what a rebuild of the menu replaces.
#[derive(Component, Clone, Default)]
pub struct MenuContent;

/// Fill the menu with one checkbox per property of the selected source.
///
/// Rebuilt only when the set of properties changes; which of them are ticked is
/// written onto the existing boxes by [`update_property_visibility`], for the
/// same reason the panel's own controls are.
pub fn rebuild_visibility_menu(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    menus: Query<Entity, With<CellPanelMenu>>,
    existing: Query<Entity, With<MenuContent>>,
    mut shown: Local<Option<(Entity, Vec<String>)>>,
) {
    let Ok(menu) = menus.single() else { return };

    let source = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok().map(|found| (shows.0, found)));

    let Some((entity, properties)) = source else {
        if shown.is_some() {
            *shown = None;
            for entity in &existing {
                commands.entity(entity).despawn();
            }
        }
        return;
    };

    let fingerprint: (Entity, Vec<String>) = (
        entity,
        properties
            .cell_properties()
            .map(|(_, property)| property.id.clone())
            .collect(),
    );
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let mut rows = vec![heading(&mut commands)];
    for (index, property) in properties.cell_properties() {
        let caption = property.name.clone();
        let row = commands
            .spawn_scene(bsn! {
                MenuContent
                @FeathersCheckbox {
                    @caption: { bsn_list![button_text(caption)] }
                }
                BlocksFrameInput
                ShowPropertyCheckbox { property: { index } }
            })
            .id();
        if property.shown {
            commands.entity(row).insert(Checked);
        }
        rows.push(row);
    }
    commands.entity(menu).add_children(&rows);
}

fn heading(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            MenuContent
            label("Show properties")
            TextFont { font_size: { FontSize::Px(13.0f32) } }
            Node { margin: { UiRect::bottom(Val::Px(4.0)) } }
        })
        .id()
}

/// List or hide the property whose box was ticked.
pub fn on_show_toggled(
    change: On<ValueChange<bool>>,
    checkboxes: Query<&ShowPropertyCheckbox>,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut sources: Query<&mut CellProperties>,
) {
    let Ok(checkbox) = checkboxes.get(change.source) else {
        return;
    };
    let Some(mut properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get_mut(shows.0).ok())
    else {
        return;
    };
    // The box's own `Checked` is left to the sync below, which reads it back
    // from the property rather than from what was clicked: the source refuses to
    // hide the one it is colored by.
    properties.set_shown(checkbox.property, change.value);
    if let Some(property) = properties.properties.get(checkbox.property) {
        info!(
            "{} {} in the panel",
            if property.shown { "showing" } else { "hiding" },
            property.name
        );
    }
}

/// Keep the menu's ticks and the sub-sections matching which properties are
/// listed, and the colored-by box unavailable.
pub fn update_property_visibility(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource>,
    sources: Query<&CellProperties>,
    boxes: Query<(
        Entity,
        &ShowPropertyCheckbox,
        Has<Checked>,
        Has<InteractionDisabled>,
    )>,
    mut sections: Query<(&PropertySection, &mut Node)>,
) {
    let Some(properties) = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| sources.get(shows.0).ok())
    else {
        return;
    };
    let is_shown = |index: usize| {
        properties
            .properties
            .get(index)
            .is_some_and(|property| property.shown)
    };

    for (entity, checkbox, checked, disabled) in &boxes {
        let shown = is_shown(checkbox.property);
        if shown != checked {
            if shown {
                commands.entity(entity).insert(Checked);
            } else {
                commands.entity(entity).remove::<Checked>();
            }
        }
        // Coloring is what holds a property on screen, so its box offers
        // nothing to click until something else is colored by.
        let locked = properties.color_by == Some(checkbox.property);
        if locked != disabled {
            if locked {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    }

    for (section, mut node) in &mut sections {
        let wanted = display(is_shown(section.property));
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

fn display(shown: bool) -> Display {
    if shown { Display::Flex } else { Display::None }
}
