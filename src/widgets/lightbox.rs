//! A picture shown as large as the window allows, over everything else.
//!
//! Any image in the UI can be made to open in it: give the entity carrying
//! its [`ImageNode`] an [`Enlargeable`], and a click on it opens the picture
//! here, scaled to fit the window without being cropped.
//! It is a [`Modal`] like help and settings, and closes the same ways.

use bevy::picking::cursor::EntityCursor;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;

use super::{Modal, ModalParts, spawn_modal};

/// How far a picture is ever scaled up. A small plot stretched to fill a large
/// window is mostly blur.
const MAX_SCALE: f32 = 4.0;

/// The share of the window a picture may take: what Feathers' backdrop
/// leaves the dialog across, and its body's cap down.
const ROOM: Vec2 = Vec2::new(0.9, 0.8);

/// Room kept around the picture for the dialog's own edges and padding.
const CHROME_PX: Vec2 = Vec2::new(26.0, 12.0);

/// An image that opens in the lightbox when clicked, titled `title`.
#[derive(Component, Clone, Default)]
#[require(EntityCursor::System(SystemCursorIcon::ZoomIn))]
pub struct Enlargeable {
    pub title: String,
}

/// The lightbox, carrying the picture it shows.
///
/// The picture is spawned with it and moved into its body once that exists,
/// since a handle cannot be carried through the dialog's scene.
#[derive(Component, Clone)]
pub struct Lightbox {
    picture: Entity,
}

impl Default for Lightbox {
    fn default() -> Self {
        Lightbox {
            picture: Entity::PLACEHOLDER,
        }
    }
}

/// Nothing opens the lightbox but a picture, so nothing carries this.
#[derive(Component, Clone, Default)]
pub struct LightboxToggle;

impl Modal for Lightbox {
    type Toggle = LightboxToggle;
    const TITLE: &'static str = "";
    const WIDTH: Val = Val::Auto;
}

/// The picture in the lightbox.
#[derive(Component)]
pub struct LightboxImage;

/// Open a clicked [`Enlargeable`] image in the lightbox.
pub fn enlarge(
    click: On<PointerClick>,
    pictures: Query<(&Enlargeable, &ImageNode), Without<LightboxImage>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok((picture, source)) = pictures.get(click.entity) else {
        return;
    };
    let shown = commands
        .spawn((
            LightboxImage,
            ImageNode::new(source.image.clone()),
            Node {
                flex_shrink: 0.0,
                align_self: AlignSelf::Center,
                ..default()
            },
        ))
        .id();
    let lightbox = Lightbox { picture: shown };
    spawn_modal(&mut commands, lightbox, picture.title.clone(), Val::Auto);
}

/// Put the picture in the lightbox as it opens.
pub fn fill_lightbox(
    add: On<Add<ModalParts>>,
    lightboxes: Query<(&Lightbox, &ModalParts)>,
    mut commands: Commands,
) {
    if let Ok((lightbox, parts)) = lightboxes.get(add.entity) {
        commands.entity(parts.body).add_child(lightbox.picture);
    }
}

/// Size the picture to the room the window leaves it. The dialog is as wide
/// as what it holds, so it follows.
pub fn fit_lightbox(
    windows: Query<&Window>,
    mut pictures: Query<(&ImageNode, &mut Node), With<LightboxImage>>,
    images: Res<Assets<Image>>,
) {
    let Ok(window) = windows.single() else { return };
    for (image, mut node) in &mut pictures {
        let Some(size) = images.get(&image.image).map(Image::size_f32) else {
            continue;
        };
        let room = window.size() * ROOM - CHROME_PX;
        let shown = size * fit_scale(size, room);
        let (width, height) = (Val::Px(shown.x), Val::Px(shown.y));
        if node.width != width || node.height != height {
            node.width = width;
            node.height = height;
        }
    }
}

/// How much a picture `size` is scaled to fit inside `room` whole.
fn fit_scale(size: Vec2, room: Vec2) -> f32 {
    if size.x <= 0.0 || size.y <= 0.0 {
        return 1.0;
    }
    (room / size).min_element().clamp(0.1, MAX_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_fits_whole_by_its_tighter_side() {
        // A wide picture in a square room is held by its width.
        assert_eq!(
            fit_scale(Vec2::new(400.0, 300.0), Vec2::new(800.0, 800.0)),
            2.0
        );
        assert_eq!(
            fit_scale(Vec2::new(150.0, 300.0), Vec2::new(900.0, 600.0)),
            2.0
        );
    }

    #[test]
    fn a_small_picture_is_not_blown_up_past_the_limit() {
        assert_eq!(
            fit_scale(Vec2::new(10.0, 10.0), Vec2::new(2000.0, 2000.0)),
            MAX_SCALE
        );
    }
}
