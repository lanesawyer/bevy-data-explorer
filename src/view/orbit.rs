//! Frames that look at their dataset in three dimensions.
//!
//! A frame goes 3D by carrying an [`Orbit`], and only over a source that
//! advertises a [`SourceVolume`] — so the header's `3d` button appears only
//! where it means something. The frame keeps its own camera throughout. Its
//! projection turns perspective and its render layer becomes the volume's, so
//! it draws the source's 3D form and none of the flat geometry the same source
//! streams for other frames; its viewport, its draw order and whether it is the
//! camera that clears the window are the grid's as before, which is what lets a
//! frame go 3D without the grid knowing.
//!
//! Everything that reads a frame's view as a 2D one — tile and node selection,
//! hover, pan and zoom — already asks for an orthographic projection and passes
//! over one that is not, so a 3D frame streams no tiles and answers no hover
//! rather than answering wrongly.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_feathers::controls::FeathersToolButton;
use bevy_ui_widgets::Activate;

use super::{Panel, View};
use crate::source::volume::SourceVolume;
use crate::source::{DataSource, ShowsSource};
use crate::widgets::{BlocksFrameInput, Icon, button_icon, set_display, set_text};

/// Radians turned per logical pixel dragged.
const TURN_PER_PX: f32 = 0.008;

/// How far above or below the volume the camera may go, short of straight
/// down, where "up" stops meaning anything and the view spins.
const MAX_PITCH: f32 = 1.5;

/// Field of view, vertically. Narrow enough that a volume's far side is not
/// shrunk much against its near side, which reads as a specimen rather than a
/// corridor.
const FOV: f32 = std::f32::consts::FRAC_PI_6;

/// A frame looking at its source in 3D, and what it showed in 2D before.
#[derive(Component, Clone, Copy, Debug)]
pub struct Orbit {
    /// The point turned about.
    pub target: Vec3,
    /// Turn about the vertical, and tilt above or below the volume, in radians.
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Where `R` puts the orbit back to, and how far out a zoom may go.
    home: Home,
    /// The render layer the volume is drawn on.
    layer: usize,
    /// The frame's 2D view, restored on the way out.
    pub flat: View,
}

#[derive(Clone, Copy, Debug)]
struct Home {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Orbit {
    /// An orbit framing the whole of `volume`, turned a little off its face so
    /// that going 3D looks like it.
    pub fn fit(volume: &SourceVolume, flat: View) -> Self {
        let home = Home {
            target: volume.center,
            yaw: -0.6,
            pitch: 0.35,
            // Far enough that the sphere round the volume fits the narrower
            // way of any frame shape the grid makes.
            distance: volume.radius() / (FOV * 0.5).sin() * 1.1,
        };
        Orbit {
            target: home.target,
            yaw: home.yaw,
            pitch: home.pitch,
            distance: home.distance,
            home,
            layer: volume.layer,
            flat,
        }
    }

    pub fn reset(&mut self) {
        self.target = self.home.target;
        self.yaw = self.home.yaw;
        self.pitch = self.home.pitch;
        self.distance = self.home.distance;
    }

    /// Where the camera sits, looking at the target.
    pub fn transform(&self) -> Transform {
        let offset = Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        ) * self.distance;
        Transform::from_translation(self.target + offset).looking_at(self.target, Vec3::Y)
    }

    /// Turn by a drag of `delta` logical pixels: sideways turns it about the
    /// vertical, and up and down tilts it, as if the pointer had hold of it.
    pub fn turn(&mut self, delta: Vec2) {
        self.yaw -= delta.x * TURN_PER_PX;
        self.pitch = (self.pitch + delta.y * TURN_PER_PX).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Slide the target across the screen by a drag of `delta` logical pixels,
    /// so the point under the pointer stays under it at the target's depth.
    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        let per_px = 2.0 * self.distance * (FOV * 0.5).tan() / viewport_height.max(1.0);
        let rotation = self.transform().rotation;
        self.target += rotation * Vec3::new(-delta.x, delta.y, 0.0) * per_px;
    }

    /// Move in or out by a wheel of `scroll` notches, the rate a flat frame
    /// zooms at.
    pub fn zoom(&mut self, scroll: f32) {
        let radius = self.home.distance;
        self.distance = (self.distance * 1.12_f32.powf(-scroll)).clamp(radius * 0.02, radius * 8.0);
    }

    /// Zoom as [`Self::zoom`] does, toward where `ray` — the one under the
    /// pointer — crosses the plane through the target facing the camera.
    ///
    /// The target moves the same fraction of the way there as the distance
    /// shrinks, which is what keeps that point under the pointer: it is how a
    /// flat frame zooms, and without it the only way in was toward the middle.
    pub fn zoom_towards(&mut self, scroll: f32, ray: Ray3d) {
        let before = self.distance;
        let forward = self.transform().forward().as_vec3();
        let facing = ray.direction.dot(forward);
        self.zoom(scroll);
        if facing <= f32::EPSILON {
            return;
        }
        let along = (self.target - ray.origin).dot(forward) / facing;
        let under = ray.get_point(along);
        self.target += (under - self.target) * (1.0 - self.distance / before);
    }

    pub fn projection(&self) -> PerspectiveProjection {
        PerspectiveProjection {
            fov: FOV,
            // Close enough to go right up to a slice, and the far plane only
            // culls, since Bevy's perspective depth is infinite.
            near: self.home.distance * 0.001,
            far: self.home.distance * 20.0,
            ..default()
        }
    }
}

/// Keep each orbiting frame's camera where its orbit says, drawing the volume.
pub fn apply_orbits(
    mut frames: Query<(&Orbit, &mut Transform, &mut Projection, &mut RenderLayers), With<Panel>>,
) {
    for (orbit, mut transform, mut projection, mut layers) in &mut frames {
        transform.set_if_neq(orbit.transform());
        let wanted = orbit.projection();
        // Read before writing: a projection taken mutably is recomputed.
        let current = match &*projection {
            Projection::Perspective(mine) => Some((mine.fov, mine.near, mine.far)),
            _ => None,
        };
        if current != Some((wanted.fov, wanted.near, wanted.far)) {
            *projection = Projection::Perspective(wanted);
        }
        layers.set_if_neq(RenderLayers::layer(orbit.layer));
    }
}

/// The header button that turns a frame between 2D and 3D.
#[derive(Component, Clone)]
pub struct PanelViewButton {
    panel: Entity,
}

impl Default for PanelViewButton {
    fn default() -> Self {
        PanelViewButton {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Add a frame's 3D button to its header, hidden until its source can be seen
/// that way.
pub(super) fn spawn_view_button(commands: &mut Commands, header: Entity, panel: Entity) {
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(Icon::Cube)] }
            }
            BlocksFrameInput
            Node { display: { Display::None } }
            PanelViewButton { panel: { panel } }
        })
        .id();
    commands.entity(header).add_child(button);
}

/// What the button shows: the view it would switch to, as the theme button
/// shows the theme it would switch to.
fn caption(orbiting: bool) -> Icon {
    if orbiting { Icon::Square } else { Icon::Cube }
}

/// Show each frame's 3D button only over a source with depth, and keep its
/// caption naming the other view.
pub fn sync_view_buttons(
    buttons: Query<(Entity, &PanelViewButton)>,
    mut nodes: Query<&mut Node>,
    frames: Query<(&ShowsSource, Has<Orbit>)>,
    volumes: Query<(), With<SourceVolume>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    for (entity, button) in &buttons {
        let Ok((shows, orbiting)) = frames.get(button.panel) else {
            continue;
        };
        set_display(&mut nodes, entity, volumes.contains(shows.0));
        let wanted = caption(orbiting).glyph();
        for child in children.iter_descendants(entity) {
            if let Ok(text) = texts.get_mut(child) {
                set_text(text, wanted);
            }
        }
    }
}

/// Turn a frame into 3D, or back.
pub fn on_view_toggled(
    activate: On<Activate>,
    mut commands: Commands,
    buttons: Query<&PanelViewButton>,
    frames: Query<(&ShowsSource, &Transform, &Projection, Option<&Orbit>)>,
    sources: Query<(&DataSource, Option<&SourceVolume>)>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Ok((shows, transform, projection, orbit)) = frames.get(button.panel) else {
        return;
    };
    let Ok((source, volume)) = sources.get(shows.0) else {
        return;
    };
    match (orbit, volume) {
        (Some(orbit), _) => {
            leave(&mut commands, button.panel, orbit, source.layer);
            info!("{}: back to 2D", source.name);
        }
        (None, Some(volume)) => {
            let Projection::Orthographic(ortho) = projection else {
                return;
            };
            let flat = View {
                center: transform.translation.truncate(),
                scale: ortho.scale,
            };
            commands
                .entity(button.panel)
                .insert(Orbit::fit(volume, flat));
            info!("{}: 3D", source.name);
        }
        (None, None) => {}
    }
}

/// Put a frame back to the 2D view it left, on its source's own layer.
fn leave(commands: &mut Commands, panel: Entity, orbit: &Orbit, layer: usize) {
    commands.entity(panel).remove::<Orbit>().insert((
        Transform::from_translation(orbit.flat.center.extend(1000.0)),
        Projection::Orthographic(OrthographicProjection {
            scale: orbit.flat.scale,
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(layer),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume() -> SourceVolume {
        SourceVolume {
            center: Vec3::new(7.0, -5.0, -7.1),
            size: Vec3::new(14.0, 10.5, 14.2),
            layer: 9,
        }
    }

    fn flat() -> View {
        View {
            center: Vec2::new(7.0, -5.0),
            scale: 0.02,
        }
    }

    #[test]
    fn a_fitted_orbit_keeps_the_whole_volume_in_view() {
        let volume = volume();
        let orbit = Orbit::fit(&volume, flat());
        let camera = orbit.transform();
        // Looking at the middle of the volume, from outside it.
        let forward = camera.forward().as_vec3();
        let to_center = (volume.center - camera.translation).normalize();
        assert!(forward.dot(to_center) > 0.9999);
        let distance = camera.translation.distance(volume.center);
        assert!(distance > volume.radius());
        // The sphere round it subtends no more than the field of view.
        assert!((volume.radius() / distance).asin() * 2.0 <= FOV);
    }

    #[test]
    fn turning_past_straight_down_stops_short_of_it() {
        // Straight down, "up" is undefined and the view spins about the axis.
        let mut orbit = Orbit::fit(&volume(), flat());
        orbit.turn(Vec2::new(0.0, 10_000.0));
        assert!(orbit.pitch <= MAX_PITCH);
        orbit.turn(Vec2::new(0.0, -20_000.0));
        assert!(orbit.pitch >= -MAX_PITCH);
        assert!(orbit.transform().translation.is_finite());
    }

    #[test]
    fn reset_undoes_every_gesture() {
        let mut orbit = Orbit::fit(&volume(), flat());
        let home = orbit.transform();
        orbit.turn(Vec2::new(120.0, -40.0));
        orbit.pan(Vec2::new(30.0, 30.0), 600.0);
        orbit.zoom(5.0);
        assert_ne!(orbit.transform(), home);
        orbit.reset();
        assert!(orbit.transform().translation.distance(home.translation) < 1e-4);
    }

    #[test]
    fn zoom_neither_passes_through_nor_loses_the_volume() {
        let mut orbit = Orbit::fit(&volume(), flat());
        orbit.zoom(1_000.0);
        assert!(orbit.distance > 0.0);
        orbit.zoom(-1_000.0);
        assert!(orbit.distance.is_finite());
    }

    #[test]
    fn panning_moves_the_target_across_the_screen_not_into_it() {
        let mut orbit = Orbit::fit(&volume(), flat());
        let before = orbit.target;
        let forward = orbit.transform().forward().as_vec3();
        orbit.pan(Vec2::new(50.0, -20.0), 600.0);
        let moved = orbit.target - before;
        assert!(moved.length() > 0.0);
        assert!(moved.dot(forward).abs() < 1e-4);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_where_it_is() {
        let mut orbit = Orbit::fit(&volume(), flat());
        let camera = orbit.transform();
        // A pointer off to one side: the ray through it from the camera.
        let aside = camera.translation
            + camera.forward().as_vec3() * orbit.distance
            + camera.right().as_vec3() * 2.0
            + camera.up().as_vec3() * 1.0;
        let under = aside;
        let ray = Ray3d::new(
            camera.translation,
            Dir3::new(under - camera.translation).unwrap(),
        );
        orbit.zoom_towards(3.0, ray);

        // Seen from where the camera is now, the same point lies straight
        // along the same direction the pointer's ray took.
        let after = orbit.transform();
        let now = (under - after.translation).normalize();
        assert!(now.dot(ray.direction.as_vec3()) > 0.9999);
        assert!(orbit.distance < orbit.home.distance);
    }

    #[test]
    fn the_button_names_the_view_it_switches_to() {
        assert_eq!(caption(false), Icon::Cube);
        assert_eq!(caption(true), Icon::Square);
    }
}
