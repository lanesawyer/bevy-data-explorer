//! Per-panel status overlays.

use bevy::prelude::*;

use crate::panel::PanelKind;
use crate::pointcloud::PointStreamer;
use crate::slices::{SliceMode, SliceStreamer};
use crate::tiles::TileStreamer;

#[derive(Component)]
pub struct PanelText {
    kind: PanelKind,
    columns: usize,
    index: usize,
}

pub fn spawn_hud(commands: &mut Commands, panels: &[(PanelKind, usize)], columns: usize) {
    for (kind, index) in panels.iter().copied() {
        commands.spawn((
            Text::new(""),
            TextFont {
                font_size: bevy::text::FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.9, 0.95)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(100.0 * index as f32 / columns as f32),
                top: Val::Px(0.0),
                margin: UiRect {
                    left: Val::Px(10.0),
                    top: Val::Px(8.0),
                    ..default()
                },
                ..default()
            },
            PanelText {
                kind,
                columns,
                index,
            },
        ));
    }
}

pub fn update_hud(
    tiles: Option<Res<TileStreamer>>,
    points: Option<Res<PointStreamer>>,
    slices: Option<Res<SliceStreamer>>,
    panels: Query<(&Camera, &Projection, &crate::panel::Panel)>,
    mut texts: Query<(&mut Text, &PanelText)>,
) {
    for (mut text, panel_text) in &mut texts {
        let view = panels
            .iter()
            .find(|(_, _, p)| p.kind == panel_text.kind)
            .and_then(|(camera, projection, _)| match projection {
                Projection::Orthographic(ortho) => Some((camera, ortho)),
                _ => None,
            });
        let Some((camera, ortho)) = view else {
            continue;
        };
        let viewport = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
        let units_per_px = ortho.area.width() / viewport.x.max(1.0);
        let _ = panel_text.columns;
        let _ = panel_text.index;

        text.0 = match panel_text.kind {
            PanelKind::Image => match tiles.as_ref() {
                Some(streamer) => image_status(streamer, units_per_px),
                None => String::new(),
            },
            PanelKind::Points => match points.as_ref() {
                Some(streamer) => points_status(streamer, units_per_px),
                None => String::new(),
            },
            PanelKind::Slices => match slices.as_ref() {
                Some(streamer) => slices_status(streamer, units_per_px),
                None => String::new(),
            },
        };
    }
}

fn image_status(streamer: &TileStreamer, units_per_px: f32) -> String {
    let dataset = streamer.dataset();
    let level = &dataset.levels[streamer.active_level];

    let channels = streamer
        .channels
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mark = if c.active { '*' } else { ' ' };
            format!("{}{}:{}", mark, i + 1, c.label)
        })
        .collect::<Vec<_>>()
        .join("  ");

    let mut notes = String::new();
    if streamer.cancelled > 0 {
        notes.push_str(&format!(", {} cancelled", streamer.cancelled));
    }
    let failed = streamer.failed();
    if failed > 0 {
        notes.push_str(&format!(", {failed} failed"));
    }

    format!(
        "OME-Zarr — {}\n\
         level {}/{}  ({} x {} px, {:.4} {}/px)\n\
         zoom {:.4} {}/screen px\n\
         tiles {} cached ({} MB / {} MB), {} loading{}\n\
         channels  {}\n\
         drag pan · scroll zoom · R reset · 1-9 channels",
        dataset.name,
        streamer.active_level,
        dataset.levels.len() - 1,
        level.width,
        level.height,
        level.scale_x,
        dataset.unit,
        units_per_px,
        dataset.unit,
        streamer.loaded(),
        streamer.resident_bytes() / (1024 * 1024),
        streamer.budget_bytes / (1024 * 1024),
        streamer.in_flight,
        notes,
        channels,
    )
}

fn points_status(streamer: &PointStreamer, units_per_px: f32) -> String {
    let cloud = streamer.cloud();
    let colour = streamer
        .colour_column
        .as_ref()
        .and_then(|name| {
            cloud
                .attributes
                .iter()
                .find(|a| &a.name == name)
                .map(|a| a.description.clone())
        })
        .unwrap_or_else(|| "none".into());

    format!(
        "Scatterbrain — {} points\n\
         octree depth {} of {}\n\
         zoom {:.5} units/screen px\n\
         {} nodes loaded, {} loading\n\
         {} / {} points resident\n\
         colour by  {}",
        cloud.total_points(),
        streamer.deepest,
        cloud.max_depth(),
        units_per_px,
        streamer.loaded_nodes(),
        streamer.in_flight,
        streamer.resident_points,
        streamer.budget,
        colour,
    )
}

fn slices_status(streamer: &SliceStreamer, units_per_px: f32) -> String {
    let cloud = streamer.cloud();
    let colour = streamer
        .colour_column
        .as_ref()
        .and_then(|name| {
            cloud
                .attributes
                .iter()
                .find(|a| &a.name == name)
                .map(|a| a.description.clone())
        })
        .unwrap_or_else(|| "none".into());

    let showing = match streamer.mode {
        SliceMode::Grid => format!(
            "grid of {} across {} columns",
            cloud.slides.len(),
            streamer.columns()
        ),
        SliceMode::Single => {
            let slide = &cloud.slides[streamer.current];
            format!(
                "slice {} of {}  [{}]  {} points",
                slide.index + 1,
                cloud.slides.len(),
                slide.id,
                slide.total_points,
            )
        }
    };

    format!(
        "Sections — {} points in {} slices\n\
         {}\n\
         zoom {:.5} {}/screen px\n\
         {} nodes loaded, {} loading\n\
         {} / {} points resident\n\
         colour by  {}\n\
         G grid/single · arrows or [ ] step slices",
        cloud.total_points(),
        cloud.slides.len(),
        showing,
        units_per_px,
        cloud.unit,
        streamer.loaded_nodes(),
        streamer.in_flight,
        streamer.resident_points,
        streamer.budget,
        colour,
    )
}
