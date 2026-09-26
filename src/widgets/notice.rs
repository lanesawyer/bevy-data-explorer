//! A message about how something went, boxed and marked by what kind of news
//! it is.
//!
//! Every status line is one of these — a dataset that would not open, a
//! bookmark that saved, a sign-in under way — so they look alike wherever they
//! sit. Whoever owns one writes a [`Notice`] onto it and [`sync_notices`] draws
//! it; an empty message hides the box rather than leaving an empty one.

use bevy::prelude::*;
use bevy::text::LineHeight;

use super::{CORNER_PX, Icon, icon_text, patch_node, set_text, size, space, text};
use crate::app::theme::Palette;

/// What kind of news a message is, which picks its icon and color.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Tone {
    /// Something under way, or worth knowing.
    #[default]
    Info,
    Success,
    /// Nothing failed, or not outright, but it is worth reading before going on.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "nothing warns yet; a state's unreadable layers are the first candidate"
        )
    )]
    Warning,
    Error,
}

impl Tone {
    fn icon(self) -> Icon {
        match self {
            Tone::Info => Icon::Info,
            Tone::Success => Icon::CircleCheck,
            Tone::Warning => Icon::TriangleAlert,
            Tone::Error => Icon::CircleAlert,
        }
    }

    /// The theme's red on dark and red on light are not the same red, so the
    /// color comes from the palette rather than from here.
    pub fn color(self, palette: &Palette) -> Color {
        match self {
            Tone::Info => palette.info,
            Tone::Success => palette.success,
            Tone::Warning => palette.caution,
            Tone::Error => palette.problem,
        }
    }
}

/// What a notice says. Empty hides it.
#[derive(Component, Clone, Default, PartialEq, Debug)]
pub struct Notice {
    pub tone: Tone,
    pub message: String,
}

impl Notice {
    pub fn new(tone: Tone, message: impl Into<String>) -> Self {
        Notice {
            tone,
            message: message.into(),
        }
    }
}

#[derive(Component, Clone, Default)]
pub(super) struct NoticeIcon;

#[derive(Component, Clone, Default)]
pub(super) struct NoticeText;

/// The height of every line in a notice, icon and text alike. The icon is
/// larger than the text, and on their own line boxes the text sat high beside
/// it; given the same one, each is centered in it, and a message that wraps
/// keeps the icon level with its first line.
const LINE_PX: f32 = 18.0;

/// How far the text is lowered to look centered. Fira Sans draws its capitals
/// and ascenders high in any line box: measured on screen, their middle sat
/// 1.5px above the icon's. An offset rather than padding, so the box keeps
/// its height and the icon its place.
const TEXT_DROP_PX: f32 = 1.5;

/// A notice, hidden until something is written to it.
pub fn notice() -> impl Scene {
    bsn! {
        Notice
        Node {
            display: { Display::None },
            width: { Val::Percent(100.0) },
            align_items: { AlignItems::FlexStart },
            column_gap: { Val::Px(space::ROWS) },
            padding: { UiRect::axes(Val::Px(space::CONTROL_INSET), Val::Px(space::ITEM_INSET * 1.5)) },
            border: { UiRect::all(Val::Px(1.0)) },
            border_radius: { BorderRadius::all(Val::Px(CORNER_PX)) },
        }
        BackgroundColor
        BorderColor
        Children [
            (icon_text(Icon::Info) NoticeIcon TextColor template_value(LineHeight::Px(LINE_PX))),
            (
                text(String::new(), size::SECONDARY)
                NoticeText
                template_value(LineHeight::Px(LINE_PX))
                Node {
                    flex_grow: { 1.0_f32 },
                    min_width: { Val::Px(0.0) },
                    top: { Val::Px(TEXT_DROP_PX) },
                }
            ),
        ]
    }
}

/// Draw each notice from what it says, and again when the theme changes.
pub(super) fn sync_notices(
    palette: Res<Palette>,
    mut notices: Query<(
        Ref<Notice>,
        &Children,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut icons: Query<(&mut Text, &mut TextColor), (With<NoticeIcon>, Without<NoticeText>)>,
    mut texts: Query<&mut Text, (With<NoticeText>, Without<NoticeIcon>)>,
) {
    for (notice, children, node, mut background, mut border) in &mut notices {
        if !notice.is_changed() && !palette.is_changed() {
            continue;
        }
        patch_node(node, |node| {
            node.display = super::display(!notice.message.is_empty());
        });
        let color = notice.tone.color(&palette);
        background.set_if_neq(BackgroundColor(color.with_alpha(0.12)));
        border.set_if_neq(BorderColor::all(color.with_alpha(0.45)));
        for &child in children {
            if let Ok((glyph, mut glyph_color)) = icons.get_mut(child) {
                set_text(glyph, notice.tone.icon().glyph());
                glyph_color.set_if_neq(TextColor(color));
            }
            if let Ok(text) = texts.get_mut(child) {
                set_text(text, &notice.message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_has_its_own_icon_and_color() {
        let tones = [Tone::Info, Tone::Success, Tone::Warning, Tone::Error];
        for palette in [Palette::dark(), Palette::light()] {
            for (i, a) in tones.iter().enumerate() {
                for b in &tones[i + 1..] {
                    assert_ne!(a.icon(), b.icon());
                    assert_ne!(a.color(&palette), b.color(&palette));
                }
            }
        }
    }
}
