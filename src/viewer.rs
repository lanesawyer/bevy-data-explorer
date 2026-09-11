//! Camera controls and the status overlay.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use crate::dataset::Dataset;
use crate::tiles::TileStreamer;

#[derive(Component)]
pub struct MainCamera;

/// Zoom limits relative to the pyramid: how far past 1:1 on the finest level
/// the user may magnify, and how far past "whole image" they may pull back.
const MAX_MAGNIFICATION: f32 = 8.0;
const MAX_PULLBACK: f32 = 4.0;

#[derive(Resource)]
pub struct ViewLimits {
    pub min_scale: f32,
    pub max_scale: f32,
    pub fit_scale: f32,
    pub centre: Vec2,
}

pub fn setup_camera(mut commands: Commands, dataset: Res<DatasetHandle>, windows: Query<&Window>) {
    let (x0, y0, x1, y1) = dataset.0.world;
    let width = (x1 - x0).abs().max(f32::EPSILON);
    let height = (y1 - y0).abs().max(f32::EPSILON);

    // World y is negated so the image reads top-down.
    let centre = Vec2::new((x0 + x1) * 0.5, -(y0 + y1) * 0.5);

    let viewport = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let fit_scale = (width / viewport.x).max(height / viewport.y);

    let finest = dataset.0.levels[0].scale_x as f32;
    commands.insert_resource(ViewLimits {
        min_scale: finest / MAX_MAGNIFICATION,
        max_scale: fit_scale * MAX_PULLBACK,
        fit_scale,
        centre,
    });

    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: fit_scale,
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_translation(centre.extend(1000.0)),
        MainCamera,
    ));
}

/// Wrapper so the dataset can be handed to systems as a resource.
#[derive(Resource)]
pub struct DatasetHandle(pub std::sync::Arc<Dataset>);

/// Scroll to zoom about the cursor, drag to pan.
pub fn camera_controls(
    mut wheel: MessageReader<MouseWheel>,
    mut camera: Single<
        (&Camera, &GlobalTransform, &mut Transform, &mut Projection),
        With<MainCamera>,
    >,
    windows: Query<&Window>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    limits: Res<ViewLimits>,
    mut drag: Local<Option<Vec2>>,
) {
    let (camera, global, transform, projection) = &mut *camera;
    let Projection::Orthographic(ortho) = projection.as_mut() else {
        return;
    };
    let Ok(window) = windows.single() else { return };
    let cursor = window.cursor_position();

    if keys.just_pressed(KeyCode::KeyR) {
        transform.translation = limits.centre.extend(transform.translation.z);
        ortho.scale = limits.fit_scale;
        return;
    }

    let mut scroll = 0.0;
    for event in wheel.read() {
        scroll += match event.unit {
            MouseScrollUnit::Line => event.y,
            // Trackpads report pixels; scale them into comparable steps.
            MouseScrollUnit::Pixel => event.y / 50.0,
        };
    }

    if scroll != 0.0 {
        let before = cursor.and_then(|c| camera.viewport_to_world_2d(global, c).ok());

        let factor = 1.12_f32.powf(-scroll);
        ortho.scale = (ortho.scale * factor).clamp(limits.min_scale, limits.max_scale);

        // Keep the world point under the cursor pinned there. The projection's
        // `area` is refreshed by Bevy after this system, so recompute the
        // mapping by hand from the new scale instead of reading it back.
        if let (Some(before), Some(cursor)) = (before, cursor) {
            let viewport = camera
                .logical_viewport_size()
                .unwrap_or(Vec2::new(window.width(), window.height()));
            let ndc = (cursor - viewport * 0.5) * Vec2::new(1.0, -1.0);
            let after = transform.translation.truncate() + ndc * ortho.scale;
            let correction = before - after;
            transform.translation.x += correction.x;
            transform.translation.y += correction.y;
        }
    }

    if buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Middle) {
        *drag = cursor;
    }
    if buttons.just_released(MouseButton::Left) || buttons.just_released(MouseButton::Middle) {
        *drag = None;
    }
    if buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Middle) {
        if let (Some(previous), Some(current)) = (*drag, cursor) {
            let delta = current - previous;
            transform.translation.x -= delta.x * ortho.scale;
            transform.translation.y += delta.y * ortho.scale;
            *drag = Some(current);
        }
    }
}

#[derive(Component)]
pub struct StatusText;

pub fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.9, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(8.0),
            ..default()
        },
        StatusText,
    ));
}

pub fn update_hud(
    streamer: Res<TileStreamer>,
    camera: Single<(&Camera, &Projection), With<MainCamera>>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    let (camera, projection) = *camera;
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let dataset = streamer.dataset();
    let level = &dataset.levels[streamer.active_level];
    let viewport = camera.logical_viewport_size().unwrap_or(Vec2::ONE);
    let units_per_px = ortho.area.width() / viewport.x.max(1.0);

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

    let failed = streamer.failed();
    let mut notes = String::new();
    if streamer.cancelled > 0 {
        notes.push_str(&format!(", {} cancelled", streamer.cancelled));
    }
    if failed > 0 {
        notes.push_str(&format!(", {failed} failed"));
    }

    text.0 = format!(
        "{}\n\
         level {}/{}  ({} x {} px, {:.4} {}/px)\n\
         zoom {:.4} {}/screen px\n\
         tiles {} cached ({} MB / {} MB), {} loading{}\n\
         channels  {}\n\
         drag pan · scroll zoom · R reset · 1-9 toggle channel",
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
    );
}

/// Number keys toggle channels. Tiles bake the composite into RGBA, so the
/// visible ones are rebuilt; the shard decoders survive, which keeps the
/// refetch cheap.
pub fn toggle_channels(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut streamer: ResMut<TileStreamer>,
) {
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];

    let Some(index) = DIGITS
        .iter()
        .position(|key| keys.just_pressed(*key))
        .filter(|i| *i < streamer.channels.len())
    else {
        return;
    };

    streamer.channels[index].active = !streamer.channels[index].active;
    streamer.reset(&mut commands);
}
