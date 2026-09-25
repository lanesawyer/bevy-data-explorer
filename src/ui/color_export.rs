//! Exporting the colors values are drawn in.
//!
//! Two ways in, each a menu under a download button in its section's header
//! that says what it exports and offers it as CSV or JSON: the cell properties
//! section exports every value whose color is known — the publisher's, or one
//! picked for it, hidden properties included — and the color overrides section
//! exports only what was picked. A value colored by the stand-in palette alone
//! is left out of the first, since that color says nothing about the value.
//!
//! Each row names the value the way the panel does: its column's name as the
//! category, its label, and the publisher's own id for it where there is one.

use bevy::prelude::*;
use bevy_feathers::controls::FeathersButton;
use bevy_ui_widgets::Activate;

use crate::app::export::{Exports, Format, Table};
use crate::bookmark::store::file_stem;
use crate::source::DataSource;
use crate::source::properties::{CellProperties, ColorOverrides, PropertyKind, PropertyValue};
use crate::view::SelectedSource;
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, Menu, button_text, size, spawn_icon_menu, text, text_dim,
};

/// A button exporting the selected source's colors.
#[derive(Component, Clone, Default)]
pub struct ExportColors {
    pub overrides_only: bool,
    pub format: Format,
}

/// A download button in `header` opening a menu that says what it exports,
/// and offers it in each format.
pub fn spawn_export_menu(commands: &mut Commands, header: Entity, overrides_only: bool) {
    let (_, menu) = spawn_icon_menu(commands, header, Icon::Download, false);
    let (title, description) = if overrides_only {
        (
            "Export color overrides",
            "The colors picked for values on this dataset, and nothing else.",
        )
    } else {
        (
            "Export colors",
            "Every value with a color of its own or one picked for it, across \
             all properties, hidden ones included. Picked colors win.",
        )
    };
    let button = |format: Format| {
        bsn! {
            @FeathersButton {
                @caption: { bsn_list![button_text(format.label())] }
            }
            Node { flex_grow: { 1.0_f32 } }
            BlocksFrameInput
            ExportColors { overrides_only: { overrides_only }, format: { format } }
        }
    };
    let content = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                text(title, size::BODY),
                text_dim(description, size::SMALL),
                (
                    Node { column_gap: { Val::Px(space::CONTROLS) } }
                    Children [
                        {button(Format::Csv)},
                        {button(Format::Json)},
                    ]
                ),
            ]
        })
        .id();
    commands.entity(menu).add_child(content);
}

const COLUMNS: [&str; 4] = ["category", "displayName", "refId", "color"];

fn hex(color: Color) -> String {
    let [r, g, b, _] = color.to_srgba().to_u8_array();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn row(category: &str, label: &str, reference: Option<&str>, color: Color) -> Vec<Option<String>> {
    vec![
        Some(category.to_string()),
        Some(label.to_string()),
        reference.map(str::to_string),
        Some(hex(color)),
    ]
}

/// Every categorical column and tree level, by id, with what it is called and
/// the values it holds.
fn columns(properties: &CellProperties) -> Vec<(&str, &str, Vec<&PropertyValue>)> {
    let mut columns = Vec::new();
    for (_, property) in properties.cell_properties() {
        match &property.kind {
            PropertyKind::Categorical(values) => {
                columns.push((
                    property.id.as_str(),
                    property.name.as_str(),
                    values.iter().collect(),
                ));
            }
            PropertyKind::Tree(tree) => {
                for (index, level) in tree.levels.iter().enumerate() {
                    columns.push((
                        level.id.as_str(),
                        level.name.as_str(),
                        tree.level_values(index).collect(),
                    ));
                }
            }
            PropertyKind::Numeric(_) => {}
        }
    }
    columns
}

/// Every value with a known color, in the panel's order.
fn known_colors(
    properties: &CellProperties,
    overrides: &ColorOverrides,
) -> Vec<Vec<Option<String>>> {
    let mut rows = Vec::new();
    for (column, name, values) in columns(properties) {
        for value in values {
            let known = value.color.is_some() || overrides.get(column, value.code).is_some();
            if known {
                rows.push(row(
                    name,
                    &value.label,
                    value.reference.as_deref(),
                    overrides.swatch(column, value),
                ));
            }
        }
    }
    rows
}

/// Every override, named where the properties can name it.
fn overridden(properties: &CellProperties, overrides: &ColorOverrides) -> Vec<Vec<Option<String>>> {
    overrides
        .iter()
        .map(
            |(column, code, color)| match properties.value_in(column, code) {
                Some((name, value)) => row(name, &value.label, value.reference.as_deref(), color),
                None => row(column, &format!("code {code}"), None, color),
            },
        )
        .collect()
}

fn on_export_colors(
    activate: On<Activate>,
    buttons: Query<&ExportColors>,
    selected: SelectedSource,
    sources: Query<(&DataSource, &CellProperties, &ColorOverrides)>,
    parents: Query<&ChildOf>,
    mut menus: Query<&mut Menu>,
    mut exports: ResMut<Exports>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    for ancestor in parents.iter_ancestors(activate.entity) {
        if let Ok(mut menu) = menus.get_mut(ancestor) {
            menu.open = false;
        }
    }
    let Some((source, properties, overrides)) = selected.get(&sources) else {
        return;
    };
    let (rows, what) = if button.overrides_only {
        (overridden(properties, overrides), "color-overrides")
    } else {
        (known_colors(properties, overrides), "colors")
    };
    if rows.is_empty() {
        info!("no colors to export for {}", source.name);
        return;
    }
    exports.save(
        Table {
            title: "Export colors".into(),
            stem: format!("{}-{what}", file_stem(&source.name)),
            columns: COLUMNS.to_vec(),
            rows,
        },
        button.format,
    );
}

pub struct ColorExportPlugin;

impl Plugin for ColorExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_export_colors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::properties::CellProperty;

    fn value(code: u16, color: Option<Color>, reference: Option<&str>) -> PropertyValue {
        PropertyValue {
            code,
            label: format!("v{code}"),
            reference: reference.map(str::to_string),
            color,
            count: None,
            selected: false,
        }
    }

    fn properties() -> CellProperties {
        CellProperties::ready(vec![CellProperty {
            id: "class".into(),
            name: "Class".into(),
            shown: true,
            kind: PropertyKind::Categorical(vec![
                value(0, Some(Color::srgb_u8(0x4c, 0x15, 0x49)), Some("L1398")),
                value(1, None, None),
                value(2, None, None),
            ]),
            gene: None,
        }])
    }

    #[test]
    fn known_colors_skip_the_stand_in_palette() {
        let mut overrides = ColorOverrides::default();
        overrides.set("class", 2, Color::WHITE);
        let rows = known_colors(&properties(), &overrides);
        assert_eq!(
            rows,
            vec![
                row(
                    "Class",
                    "v0",
                    Some("L1398"),
                    Color::srgb_u8(0x4c, 0x15, 0x49)
                ),
                row("Class", "v2", None, Color::WHITE),
            ]
        );
        assert_eq!(rows[0][3].as_deref(), Some("#4c1549"));
    }

    #[test]
    fn overrides_name_what_they_can() {
        let mut overrides = ColorOverrides::default();
        overrides.set("class", 0, Color::BLACK);
        overrides.set("gone", 7, Color::BLACK);
        assert_eq!(
            overridden(&properties(), &overrides),
            vec![
                row("Class", "v0", Some("L1398"), Color::BLACK),
                row("gone", "code 7", None, Color::BLACK),
            ]
        );
    }
}
