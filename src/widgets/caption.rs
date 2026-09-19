use bevy::prelude::*;
use bevy_feathers::display::label_dim;

/// A dim caption, for the secondary lines of a listing.
pub fn caption(commands: &mut Commands, text: impl Into<String>) -> Entity {
    let text = text.into();
    commands.spawn_scene(bsn! { label_dim(text) }).id()
}
