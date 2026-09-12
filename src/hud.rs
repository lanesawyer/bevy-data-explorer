//! Per-frame overlays.
//!
//! Each frame carries a header in its top corner: the dataset's name, a button
//! that opens the inspector on it, and a menu for pointing the frame at a
//! different dataset. The status its plugin reports sits underneath.
//!
//! The overlay knows nothing about any particular format. It reads the name and
//! status off whichever source entity a panel points at, and adds the lines
//! only a panel can know — its own zoom, which differs between two frames
//! showing the same source.
//!
//! Everything sits on a translucent panel, because it is drawn over imagery
//! that is bright in places and black in others.

use bevy::prelude::*;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label;
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::Activate;

use crate::datasource::{DataSource, SourceStatus};
use crate::panel::{BlocksFrameInput, Panel, PanelRequest, ShowsSource};
use crate::widgets::spawn_menu;

/// The translucent panel a frame's header and status sit on.
#[derive(Component, Clone)]
pub struct PanelHeader {
    panel: Entity,
}

impl Default for PanelHeader {
    fn default() -> Self {
        PanelHeader {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// The dataset's name, at the start of a frame's header.
#[derive(Component, Clone, Default)]
pub struct PanelTitle;

/// Opens the inspector on the frame it belongs to.
#[derive(Component, Clone)]
pub struct PanelInfoButton {
    panel: Entity,
}

impl Default for PanelInfoButton {
    fn default() -> Self {
        PanelInfoButton {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// The menu that points a frame at a different dataset.
#[derive(Component, Clone)]
pub struct SourceMenu {
    panel: Entity,
}

impl Default for SourceMenu {
    fn default() -> Self {
        SourceMenu {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// One dataset offered by a frame's menu.
#[derive(Component, Clone)]
pub struct SourceChoice {
    panel: Entity,
    source: Entity,
}

impl Default for SourceChoice {
    fn default() -> Self {
        SourceChoice {
            panel: Entity::PLACEHOLDER,
            source: Entity::PLACEHOLDER,
        }
    }
}

/// A status overlay bound to one panel. Bound by entity rather than by source
/// so that duplicated panels each get their own and report their own zoom.
#[derive(Component, Clone)]
pub struct PanelText {
    panel: Entity,
}

impl Default for PanelText {
    fn default() -> Self {
        // Scenes patch over defaults; the real panel is written on top.
        PanelText {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Keep one overlay per panel, and drop the overlays of panels that have gone
/// away.
pub fn sync_hud(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    headers: Query<(Entity, &PanelHeader)>,
) {
    // The box owns the header row and the status, so despawning it takes the
    // whole overlay with it.
    for (entity, header) in &headers {
        if panels.get(header.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }

    for panel in &panels {
        if headers.iter().any(|(_, header)| header.panel == panel) {
            continue;
        }
        spawn_overlay(&mut commands, panel);
    }
}

/// Build one frame's overlay: a header row over the status it reports.
fn spawn_overlay(commands: &mut Commands, panel: Entity) {
    let box_ = commands
        .spawn_scene(bsn! {
            PanelHeader { panel: { panel } }
            Node {
                position_type: { PositionType::Absolute },
                flex_direction: { FlexDirection::Column },
                row_gap: { Val::Px(2.0) },
                padding: { UiRect::axes(Val::Px(8.0), Val::Px(6.0)) },
                border_radius: { BorderRadius::all(Val::Px(5.0)) },
            }
            // Dark and translucent, so the overlay reads over pale tissue and
            // over the black around it alike.
            BackgroundColor({ Color::srgba(0.04, 0.05, 0.07, 0.72) })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let header = commands
        .spawn_scene(bsn! {
            Node {
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(4.0) },
            }
        })
        .id();

    let title = commands
        .spawn_scene(bsn! {
            PanelTitle
            label("")
            InheritableFont { font_size: { 14.0f32 } }
            Node { margin: { UiRect::right(Val::Px(2.0)) } }
        })
        .id();

    let info = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![label("i")] }
            }
            BlocksFrameInput
            PanelInfoButton { panel: { panel } }
        })
        .id();

    commands.entity(header).add_children(&[title, info]);
    let menu = spawn_menu(commands, header);
    commands.entity(menu).insert(SourceMenu { panel });

    let status = commands
        .spawn_scene(bsn! {
            PanelText { panel: { panel } }
            Text
            TextFont { font_size: { bevy::text::FontSize::Px(12.0) } }
            TextColor({ Color::srgb(0.78, 0.83, 0.90) })
        })
        .id();

    commands.entity(box_).add_children(&[header, status]);
}

/// Keep each overlay over its panel's cell.
pub fn position_hud(
    area: Res<crate::panel::FrameArea>,
    panels: Query<&Panel>,
    mut texts: Query<(&PanelHeader, &mut Node)>,
) {
    let (columns, rows) = crate::panel::grid_for(panels.iter().count());
    let cell = Vec2::new(area.size.x / columns as f32, area.size.y / rows as f32);

    for (text, mut node) in &mut texts {
        let Ok(panel) = panels.get(text.panel) else {
            continue;
        };
        let (col, row) = (panel.index % columns, panel.index / columns);
        node.left = Val::Px(area.origin.x + cell.x * col as f32 + 10.0);
        node.top = Val::Px(area.origin.y + cell.y * row as f32 + 8.0);
        // Clear of the frame's own buttons in the opposite corner.
        node.max_width = Val::Px((cell.x - 90.0).max(120.0));
    }
}

pub fn update_hud(
    panels: Query<(&Camera, &Projection, &ShowsSource)>,
    sources: Query<(&DataSource, &SourceStatus)>,
    mut texts: Query<(&mut Text, &PanelText)>,
    titles: Query<(Entity, &ChildOf), With<PanelTitle>>,
    headers: Query<&PanelHeader>,
    parents: Query<&ChildOf>,
    mut title_texts: Query<&mut Text, Without<PanelText>>,
) {
    for (mut text, panel_text) in &mut texts {
        let Ok((camera, projection, shows)) = panels.get(panel_text.panel) else {
            continue;
        };
        let Ok((source, status)) = sources.get(shows.0) else {
            continue;
        };
        let Projection::Orthographic(ortho) = projection else {
            continue;
        };
        let viewport = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
        let units_per_px = ortho.area.width() / viewport.x.max(1.0);

        // The name has moved up into the header, so it is no longer repeated
        // here.
        text.0 = format!(
            "{}\nzoom {:.5} {}/screen px",
            status.0, units_per_px, source.unit,
        );
    }

    for (entity, _) in &titles {
        let Some(panel) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| headers.get(ancestor).ok())
            .map(|header| header.panel)
        else {
            continue;
        };
        let Ok((_, _, shows)) = panels.get(panel) else {
            continue;
        };
        let Ok((source, _)) = sources.get(shows.0) else {
            continue;
        };
        if let Ok(mut text) = title_texts.get_mut(entity)
            && text.0 != source.name
        {
            text.0 = source.name.clone();
        }
    }
}

/// Open the inspector on a frame.
pub fn on_info_pressed(
    activate: On<Activate>,
    buttons: Query<&PanelInfoButton>,
    mut requests: MessageWriter<PanelRequest>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        requests.write(PanelRequest::Inspect(button.panel));
    }
}

/// Point a frame at the dataset chosen from its menu.
pub fn on_source_chosen(
    activate: On<Activate>,
    choices: Query<&SourceChoice>,
    mut requests: MessageWriter<PanelRequest>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        requests.write(PanelRequest::Show {
            panel: choice.panel,
            source: choice.source,
        });
    }
}

/// Fill each frame's menu with the datasets it could show.
///
/// Rebuilt when the set of sources changes, or when a frame is pointed
/// somewhere else, so the current one stays marked.
pub fn rebuild_source_menus(
    mut commands: Commands,
    menus: Query<(Entity, &SourceMenu)>,
    panels: Query<&ShowsSource>,
    sources: Query<(Entity, &DataSource)>,
    existing: Query<Entity, With<SourceChoice>>,
    mut shown: Local<Option<(Vec<(Entity, Entity)>, usize)>>,
) {
    // What each frame is showing, plus how many datasets there are to offer.
    // Either changing is what the rows have to reflect.
    let mut current: Vec<(Entity, Entity)> = menus
        .iter()
        .filter_map(|(_, menu)| {
            panels
                .get(menu.panel)
                .ok()
                .map(|shows| (menu.panel, shows.0))
        })
        .collect();
    current.sort_unstable();
    let fingerprint = (current, sources.iter().count());

    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let mut listed: Vec<(Entity, &DataSource)> = sources.iter().collect();
    // Registration order, which is the order the frames were opened in.
    listed.sort_by_key(|(_, source)| source.layer);

    for (menu_entity, menu) in &menus {
        let showing = panels.get(menu.panel).map(|shows| shows.0).ok();
        let rows: Vec<Entity> = listed
            .iter()
            .map(|(entity, source)| {
                let current = showing == Some(*entity);
                // A leading mark rather than a separate control, so the row
                // stays one target however long the name is.
                let caption = format!("{} {}", if current { "*" } else { " " }, source.name);
                commands
                    .spawn_scene(bsn! {
                        @FeathersToolButton {
                            @caption: { bsn_list![label(caption)] }
                        }
                        BlocksFrameInput
                        SourceChoice { panel: { menu.panel }, source: { *entity } }
                        Node { width: { Val::Percent(100.0) } }
                    })
                    .id()
            })
            .collect();
        commands.entity(menu_entity).add_children(&rows);
    }
}
