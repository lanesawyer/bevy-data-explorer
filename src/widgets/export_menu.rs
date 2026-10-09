//! A download button opening a menu that says what it exports and offers it
//! in each [`Format`].

use bevy::prelude::*;
use bevy_feathers::controls::FeathersButton;

use super::space;
use super::{BlocksFrameInput, Icon, Menu, button_text, size, spawn_icon_menu, text, text_dim};
use crate::app::export::Format;

/// What [`spawn_export_menu`] built.
pub struct ExportMenu {
    pub menu: Entity,
    /// The text under the title, for a caller whose offer changes.
    pub description: Entity,
    /// A button for each format, for the caller to mark with what it exports.
    pub formats: [(Format, Entity); 2],
}

/// A download button in `header` whose menu is titled `title` and explains
/// itself with `description`.
pub fn spawn_export_menu(
    commands: &mut Commands,
    header: Entity,
    title: &str,
    description: &str,
) -> ExportMenu {
    let (_, menu) = spawn_icon_menu(commands, header, Icon::Download);
    let mut format_button = |format: Format| {
        let entity = commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @caption: { bsn_list! {@button_text(format.label())} }
                }
                Node { flex_grow: { 1.0_f32 } }
                BlocksFrameInput
            })
            .id();
        (format, entity)
    };
    let formats = [format_button(Format::Csv), format_button(Format::Json)];
    let row = commands
        .spawn(Node {
            column_gap: Val::Px(space::CONTROLS),
            ..default()
        })
        .add_children(&formats.map(|(_, entity)| entity))
        .id();
    let title = commands
        .spawn_scene(text(title.to_string(), size::BODY))
        .id();
    let description = commands
        .spawn_scene(text_dim(description.to_string(), size::SMALL))
        .id();
    let content = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(space::ROWS),
            ..default()
        })
        .add_children(&[title, description, row])
        .id();
    commands.entity(menu).add_child(content);
    ExportMenu {
        menu,
        description,
        formats,
    }
}

/// Close whatever menu holds `button`, once it has done what it offered.
pub fn close_menu_holding(button: Entity, parents: &Query<&ChildOf>, menus: &mut Query<&mut Menu>) {
    for ancestor in parents.iter_ancestors(button) {
        if let Ok(mut menu) = menus.get_mut(ancestor) {
            menu.open = false;
        }
    }
}
