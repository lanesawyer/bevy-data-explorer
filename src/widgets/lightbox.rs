//! A picture shown as large as the window allows, over everything else.
//!
//! Any image in the UI can be made to open in it: give the entity carrying
//! its [`ImageNode`] an [`Enlargeable`], and a click on it opens the picture
//! here, scaled to fit the frames' share of the window without being cropped.
//! It is a [`Modal`] like help and settings, and closes the same ways.

use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy_feathers::cursor::EntityCursor;

use super::modal::PADDING_PX;
use super::{Modal, ModalScreen, set_modal_open, set_text, spawn_modal};

/// How far a picture is ever scaled up. A small plot stretched to fill a large
/// window is mostly blur.
const MAX_SCALE: f32 = 4.0;

/// Room kept for the title row above the picture.
const HEADER_PX: f32 = 48.0;

/// An image that opens in the lightbox when clicked, titled `title`.
#[derive(Component, Clone, Default)]
#[require(EntityCursor::System(SystemCursorIcon::ZoomIn))]
pub struct Enlargeable {
    pub title: String,
}

/// The lightbox's backdrop.
#[derive(Component, Clone, Default)]
pub struct Lightbox;

/// Its X.
#[derive(Component, Clone, Default)]
pub struct LightboxToggle;

impl Modal for Lightbox {
    type Toggle = LightboxToggle;
}

/// The picture in the lightbox, and the panel it sets the width of.
#[derive(Component)]
pub struct LightboxImage {
    panel: Entity,
    title: Entity,
}

pub fn spawn_lightbox(mut commands: Commands) {
    let parts = spawn_modal::<Lightbox>(&mut commands, "", 0.0);
    let image = commands
        .spawn((
            LightboxImage {
                panel: parts.panel,
                title: parts.title,
            },
            ImageNode::default(),
            Node {
                flex_shrink: 0.0,
                align_self: AlignSelf::Center,
                ..default()
            },
        ))
        .id();
    commands.entity(parts.panel).add_child(image);
}

/// Open a clicked [`Enlargeable`] image in the lightbox.
pub fn enlarge(
    click: On<Pointer<Click>>,
    pictures: Query<(&Enlargeable, &ImageNode), Without<LightboxImage>>,
    mut shown: Query<(&LightboxImage, &mut ImageNode)>,
    mut texts: Query<&mut Text>,
    mut screens: Query<&mut Node, With<Lightbox>>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok((picture, source)) = pictures.get(click.entity) else {
        return;
    };
    let Ok((lightbox, mut image)) = shown.single_mut() else {
        return;
    };
    image.image = source.image.clone();
    if let Ok(text) = texts.get_mut(lightbox.title) {
        set_text(text, &picture.title);
    }
    set_modal_open::<Lightbox>(&mut screens, Some(true));
}

/// Size the picture to the room the backdrop leaves it, and the panel to the
/// picture.
pub fn fit_lightbox(
    windows: Query<&Window>,
    screens: Query<&Node, (With<Lightbox>, With<ModalScreen>)>,
    lightboxes: Query<(Entity, &LightboxImage, &ImageNode)>,
    images: Res<Assets<Image>>,
    mut nodes: Query<&mut Node, Without<Lightbox>>,
) {
    let (Ok(window), Ok(screen)) = (windows.single(), screens.single()) else {
        return;
    };
    if screen.display == Display::None {
        return;
    }
    let Ok((entity, lightbox, image)) = lightboxes.single() else {
        return;
    };
    let Some(size) = images.get(&image.image).map(Image::size_f32) else {
        return;
    };
    // The backdrop is padded to the frames' share of the window, and the
    // panel may take nine tenths of what is left.
    let px = |val: Val| match val {
        Val::Px(px) => px,
        _ => 0.0,
    };
    let room = Vec2::new(
        window.width() - px(screen.padding.left) - px(screen.padding.right),
        window.height() - px(screen.padding.top) - px(screen.padding.bottom),
    ) * 0.9
        - Vec2::new(PADDING_PX * 2.0, PADDING_PX * 2.0 + HEADER_PX);
    let shown = size * fit_scale(size, room);

    if let Ok(mut node) = nodes.get_mut(entity) {
        let (width, height) = (Val::Px(shown.x), Val::Px(shown.y));
        if node.width != width || node.height != height {
            node.width = width;
            node.height = height;
        }
    }
    if let Ok(mut node) = nodes.get_mut(lightbox.panel) {
        let width = Val::Px(shown.x + PADDING_PX * 2.0);
        if node.width != width {
            node.width = width;
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
