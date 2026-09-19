use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_ui_widgets::Activate;

use super::{BlocksFrameInput, Icon, button_icon, button_text};

/// A button that opens `url` in the system's browser.
#[derive(Component, Clone, Default)]
pub struct ExternalLink {
    pub url: &'static str,
}

/// A link, drawn as a button with `icon` beside `text`. `Plain` shows no
/// background until hovered, which is what reads as a link in running text.
pub fn link_button(
    icon: Icon,
    text: &'static str,
    url: &'static str,
    variant: ButtonVariant,
) -> impl Scene {
    bsn! {
        @FeathersButton {
            @variant: { variant },
            @caption: { bsn_list![button_icon(icon), button_text(text)] }
        }
        Node { column_gap: { Val::Px(6.0) } }
        BlocksFrameInput
        ExternalLink { url: { url } }
    }
}

pub fn on_link_pressed(activate: On<Activate>, links: Query<&ExternalLink>) {
    let Ok(link) = links.get(activate.entity) else {
        return;
    };
    // Detached: a browser that is not already running would otherwise hold
    // the frame until it finished starting.
    match open::that_detached(link.url) {
        Ok(()) => info!("opened {}", link.url),
        Err(error) => warn!("could not open {}: {error}", link.url),
    }
}
