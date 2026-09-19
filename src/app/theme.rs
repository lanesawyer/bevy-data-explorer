//! Which theme the app wears, and where it comes from.
//!
//! Bevy reports the desktop's preference on the window — [`WindowTheme`], with
//! a `WindowThemeChanged` event when it changes — so that much is hooked up
//! rather than invented. What it does not supply is a light theme: Feathers
//! ships `create_dark_theme` and nothing else, so the light one here is made by
//! turning the dark one over.
//!
//! Following the desktop stops the moment the button is pressed. A preference
//! read from elsewhere is a good default and a bad override: having the theme
//! change back under you because the desktop said so is worse than not
//! following it at all.

use bevy::prelude::*;
use bevy::window::{WindowTheme, WindowThemeChanged};
use bevy_feathers::theme::{ThemeProps, UiTheme};

use crate::app::schedule::Stage;

/// Widen the button state tokens.
///
/// Feathers' hover is a five percent lift in lightness, which is hard to see at
/// all over a frame's imagery and reads as a button that does not respond. The
/// tokens are widened rather than each button being styled by hand, so every
/// control in the app moves together.
pub fn dark() -> ThemeProps {
    use bevy_feathers::{dark_theme::create_dark_theme, palette, tokens};

    let mut theme = create_dark_theme();
    theme
        .color
        .insert(tokens::BUTTON_BG_HOVER, palette::GRAY_3.lighter(0.14));
    theme
        .color
        .insert(tokens::BUTTON_BG_PRESSED, palette::ACCENT.darker(0.12));
    theme.color.insert(
        tokens::BUTTON_PRIMARY_BG_HOVER,
        palette::ACCENT.lighter(0.08),
    );
    theme
}

/// A first cut at a light theme, turned over from the dark one.
///
/// Feathers supplies no light palette, and a theme is a set of relationships
/// rather than a list of colors: a near-neutral token's job is to sit a
/// certain distance from the background, and flipping its lightness in OKLCH
/// keeps that distance while swapping which side it is on.
///
/// Two kinds of token are left alone. A color that carries meaning — the
/// accent, the axis colors — means the same thing in either theme, and a hue
/// that shifted with the theme would stop being a signal. And text that sits on
/// one of those colors has to stay where it is, or it would turn dark over a
/// background that never moved.
///
/// This is a starting point to look at and adjust, not a designed palette.
pub fn light() -> ThemeProps {
    let mut theme = dark();
    for (name, color) in &mut theme.color {
        if !STAYS_PUT.contains(name) {
            *color = flip(*color);
        }
    }
    theme
}

/// Text that must not turn over, because what it is written on does not.
///
/// A button keeps its pale label — over a mid grey in the light theme, which is
/// what it already was over a dark one — and the primary button's label sits on
/// the accent, which never moves. A slider's readout is written over the filled
/// part of its bar, and that is the accent too.
///
/// Everything not named here turns over with its background. Leaving all white
/// text alone was the first cut of this, and it left a menu and a URL field
/// writing white on white.
const STAYS_PUT: [bevy_feathers::theme::ThemeToken; 6] = {
    use bevy_feathers::tokens::*;
    [
        BUTTON_TEXT,
        BUTTON_TEXT_DISABLED,
        BUTTON_PRIMARY_TEXT,
        BUTTON_PRIMARY_TEXT_DISABLED,
        SLIDER_TEXT,
        SLIDER_TEXT_DISABLED,
    ]
};

/// How saturated a color may be and still count as neutral chrome.
const NEUTRAL_CHROMA: f32 = 0.03;

fn flip(color: Color) -> Color {
    let oklch = Oklcha::from(color);
    if oklch.chroma > NEUTRAL_CHROMA {
        return color;
    }
    Color::from(Oklcha {
        lightness: 1.0 - oklch.lightness,
        ..oklch
    })
}

/// The colors this app names for itself, which Feathers has no token for.
///
/// Two ways out of here, from one definition. The ones on controls that are
/// spawned once go into the theme as tokens, and Feathers repaints them when
/// the theme changes like any of its own. The ones chosen while running — the
/// color of a histogram bar depends on whether the span admits it, a status
/// line on whether it went well — are read from the [`Palette`] resource at the
/// moment they are decided.
pub mod token {
    use bevy_feathers::theme::ThemeToken;

    /// The translucent panel a frame's header and status sit on.
    pub const OVERLAY_BG: ThemeToken = ThemeToken::new_static("explorer.overlay.bg");
    pub const OVERLAY_TEXT: ThemeToken = ThemeToken::new_static("explorer.overlay.text");
    pub const OVERLAY_DIM: ThemeToken = ThemeToken::new_static("explorer.overlay.dim");
    /// The rule drawn between frames.
    pub const DIVIDER: ThemeToken = ThemeToken::new_static("explorer.divider");
    /// The outline around the selected frame.
    pub const SELECTION: ThemeToken = ThemeToken::new_static("explorer.selection");
    /// The rail a numeric range is dragged along, and the ends that drag it.
    pub const TRACK: ThemeToken = ThemeToken::new_static("explorer.track");
    pub const THUMB: ThemeToken = ThemeToken::new_static("explorer.thumb");
    /// The bar that sweeps across a frame while it is fetching.
    pub const LOADING: ThemeToken = ThemeToken::new_static("explorer.loading");
}

/// Every color the app names that is not a Feathers token.
#[derive(Resource, Clone, Copy)]
pub struct Palette {
    /// What a frame clears to where it has drawn nothing. Read by the cameras,
    /// and by a saved picture, which keys it out to transparency.
    pub frame_bg: Color,
    pub overlay_bg: Color,
    pub overlay_text: Color,
    pub overlay_dim: Color,
    pub divider: Color,
    pub selection: Color,
    /// A control's rail, the part of it within the chosen span, and the ends
    /// that move it.
    pub track: Color,
    pub fill: Color,
    pub thumb: Color,
    /// A histogram bucket outside the chosen span: dimmed rather than hidden,
    /// so the shape of the whole distribution stays visible.
    pub bar_dim: Color,
    /// A line reporting progress, one reporting something worth noticing, and
    /// one reporting that something went wrong.
    pub progress: Color,
    pub caution: Color,
    pub problem: Color,
}

impl Palette {
    pub fn of(mode: &ThemeMode) -> Self {
        if mode.is_dark() {
            Self::dark()
        } else {
            Self::light()
        }
    }

    pub fn dark() -> Self {
        Palette {
            frame_bg: Color::srgb(0.04, 0.04, 0.06),
            // Dark and translucent, so the overlay reads over pale tissue and
            // over the black around it alike.
            overlay_bg: Color::srgba(0.04, 0.05, 0.07, 0.72),
            overlay_text: Color::srgb(0.86, 0.90, 0.96),
            overlay_dim: Color::srgb(0.78, 0.83, 0.90),
            divider: Color::srgb(0.25, 0.27, 0.32),
            selection: Color::srgb(0.38, 0.60, 0.90),
            track: Color::srgb(0.20, 0.22, 0.28),
            fill: Color::srgb(0.38, 0.60, 0.90),
            thumb: Color::srgb(0.85, 0.89, 0.95),
            bar_dim: Color::srgb(0.22, 0.25, 0.31),
            progress: Color::srgb(0.70, 0.76, 0.85),
            caution: Color::srgb(0.95, 0.76, 0.36),
            problem: Color::srgb(0.95, 0.48, 0.45),
        }
    }

    /// The light counterpart, by the same rule the theme itself follows: the
    /// near-neutrals turn over and the colors that mean something stay put.
    ///
    /// The overlay is the one that cannot simply flip. It is a panel over
    /// imagery rather than over the window, so in light mode it is a pale wash
    /// with dark text on it — a dark panel would read as a hole punched in the
    /// picture.
    pub fn light() -> Self {
        let dark = Palette::dark();
        Palette {
            frame_bg: Color::srgb(0.93, 0.93, 0.95),
            overlay_bg: Color::srgba(0.97, 0.97, 0.99, 0.78),
            overlay_text: flip(dark.overlay_text),
            overlay_dim: flip(dark.overlay_dim),
            divider: flip(dark.divider),
            selection: dark.selection,
            track: flip(dark.track),
            fill: dark.fill,
            thumb: flip(dark.thumb),
            bar_dim: flip(dark.bar_dim),
            progress: flip(dark.progress),
            // Amber and red mean the same thing in either theme, darkened to
            // hold their own against a pale background.
            caution: Color::srgb(0.60, 0.40, 0.05),
            problem: Color::srgb(0.72, 0.16, 0.14),
        }
    }

    /// Hand the colors that sit on controls to the theme, so that Feathers
    /// repaints them along with its own.
    fn into_theme(self, theme: &mut ThemeProps) {
        for (name, color) in [
            (token::OVERLAY_BG, self.overlay_bg),
            (token::OVERLAY_TEXT, self.overlay_text),
            (token::OVERLAY_DIM, self.overlay_dim),
            (token::DIVIDER, self.divider),
            (token::SELECTION, self.selection),
            (token::TRACK, self.track),
            (token::THUMB, self.thumb),
            (token::LOADING, self.fill),
        ] {
            theme.color.insert(name, color);
        }
    }
}

/// Which theme is being worn, and whether the desktop still gets a say.
#[derive(Resource)]
pub struct ThemeMode {
    pub mode: WindowTheme,
    /// Set once the button has been pressed, after which the desktop's
    /// preference is ignored.
    chosen: bool,
}

impl Default for ThemeMode {
    fn default() -> Self {
        ThemeMode {
            // Dark until the window says otherwise, which is what this app has
            // always been.
            mode: WindowTheme::Dark,
            chosen: false,
        }
    }
}

impl ThemeMode {
    pub fn is_dark(&self) -> bool {
        matches!(self.mode, WindowTheme::Dark)
    }

    /// Switch by hand, which is also what stops the desktop switching back.
    pub fn toggle(&mut self) {
        self.mode = if self.is_dark() {
            WindowTheme::Light
        } else {
            WindowTheme::Dark
        };
        self.chosen = true;
    }

    fn follow(&mut self, mode: WindowTheme) {
        if !self.chosen {
            self.mode = mode;
        }
    }
}

/// Take the desktop's preference as the starting theme.
///
/// The window carries it only if the platform reports one, so no answer means
/// the default stands rather than a guess being made.
fn seed_from_window(windows: Query<&Window>, mut mode: ResMut<ThemeMode>) {
    let Some(theme) = windows.iter().next().and_then(|window| window.window_theme) else {
        return;
    };
    mode.follow(theme);
}

/// Follow the desktop when it changes, until the button says otherwise.
fn follow_window_theme(
    mut changed: MessageReader<WindowThemeChanged>,
    mut mode: ResMut<ThemeMode>,
) {
    for event in changed.read() {
        mode.follow(event.theme);
    }
}

/// Put the chosen theme in place.
///
/// Feathers repaints everything carrying a theme token when [`UiTheme`]
/// changes, so swapping the resource is the whole of applying a theme — for
/// controls that take their colors from tokens. Colors written as literals
/// are not reached, which is why the frames stay dark.
fn apply_theme(mut commands: Commands, mode: Res<ThemeMode>, mut theme: ResMut<UiTheme>) {
    if !mode.is_changed() {
        return;
    }
    let palette = Palette::of(&mode);
    let mut props = if mode.is_dark() { dark() } else { light() };
    palette.into_theme(&mut props);
    theme.0 = props;
    commands.insert_resource(palette);
}

/// The theme, and where it is taken from.
pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        let mode = ThemeMode::default();
        let palette = Palette::of(&mode);
        let mut props = dark();
        palette.into_theme(&mut props);

        app.insert_resource(mode)
            .insert_resource(palette)
            .insert_resource(UiTheme(props))
            // With the controls, which is what a theme is made of, and after
            // the stage that reads the pointer so a press is answered in the
            // frame it happened.
            .add_systems(
                Update,
                (follow_window_theme, apply_theme)
                    .chain()
                    .in_set(Stage::ControlsApply),
            )
            .add_systems(
                Startup,
                seed_from_window.in_set(crate::app::schedule::Boot::Shell),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lightness(color: Color) -> f32 {
        Oklcha::from(color).lightness
    }

    #[test]
    fn the_light_theme_turns_the_chrome_over() {
        use bevy_feathers::tokens;
        let (dark, light) = (dark(), light());

        // The window goes from dark to light, and the text it carries with it.
        let bg = |theme: &ThemeProps| lightness(theme.color[&tokens::WINDOW_BG]);
        assert!(bg(&dark) < 0.5 && bg(&light) > 0.5);
        let text = |theme: &ThemeProps| lightness(theme.color[&tokens::TEXT_MAIN]);
        assert!(text(&dark) > 0.5 && text(&light) < 0.5);
    }

    #[test]
    fn text_keeps_its_distance_from_the_background() {
        use bevy_feathers::tokens;
        let light = light();
        let gap =
            lightness(light.color[&tokens::TEXT_MAIN]) - lightness(light.color[&tokens::WINDOW_BG]);
        assert!(gap.abs() > 0.3, "label text has to stay readable: {gap}");
    }

    #[test]
    fn colors_that_mean_something_do_not_move() {
        use bevy_feathers::{palette, tokens};
        let light = light();
        // The accent is the selection color; it says "this one" in either
        // theme, and a hue that moved with the theme would stop saying it.
        assert_eq!(light.color[&tokens::BUTTON_PRIMARY_BG], palette::ACCENT);
    }

    #[test]
    fn a_buttons_label_stays_pale_in_both_themes() {
        use bevy_feathers::tokens;
        // What the user sees first: a button that kept its dark background
        // needs to keep its pale text with it.
        let light = light();
        assert!(lightness(light.color[&tokens::BUTTON_TEXT]) > 0.95);
        assert_eq!(
            light.color[&tokens::BUTTON_PRIMARY_TEXT],
            dark().color[&tokens::BUTTON_PRIMARY_TEXT]
        );
    }

    #[test]
    fn text_on_a_background_that_moves_moves_with_it() {
        use bevy_feathers::tokens;
        // The other half of the same rule, and the bug it replaces: a menu and
        // a URL field whose backgrounds turned light kept white text on them.
        let light = light();
        for token in [tokens::MENUITEM_TEXT, tokens::TEXT_INPUT_TEXT] {
            let text = lightness(light.color[&token]);
            assert!(text < 0.5, "{token} stayed pale over a pale background");
        }
        assert!(lightness(light.color[&tokens::MENU_BG]) > 0.5);
        assert!(lightness(light.color[&tokens::TEXT_INPUT_BG]) > 0.5);
    }

    #[test]
    fn the_frames_turn_over_with_the_docks() {
        // The whole window changes, not only the chrome: a dark frame area
        // around a light dock is the seam this replaces.
        assert!(lightness(Palette::dark().frame_bg) < 0.3);
        assert!(lightness(Palette::light().frame_bg) > 0.7);
    }

    #[test]
    fn an_overlay_stays_readable_over_the_picture_it_covers() {
        // It is a panel over imagery rather than over the window, so its text
        // has to contrast with the panel in either theme.
        for palette in [Palette::dark(), Palette::light()] {
            let gap = lightness(palette.overlay_text) - lightness(palette.overlay_bg);
            assert!(gap.abs() > 0.3, "overlay text is not readable: {gap}");
        }
    }

    #[test]
    fn selection_means_the_same_thing_in_both_themes() {
        assert_eq!(Palette::dark().selection, Palette::light().selection);
        assert_eq!(Palette::dark().fill, Palette::light().fill);
    }

    #[test]
    fn a_problem_stands_out_against_either_background() {
        // Red on dark and red on light are not the same red.
        assert_ne!(Palette::dark().problem, Palette::light().problem);
        assert!(lightness(Palette::light().problem) < lightness(Palette::light().frame_bg));
        assert!(lightness(Palette::dark().problem) > lightness(Palette::dark().frame_bg));
    }

    #[test]
    fn the_palette_reaches_the_theme() {
        // Controls spawned once are repainted by Feathers, which can only do it
        // for colors the theme knows about.
        let palette = Palette::light();
        let mut props = light();
        palette.into_theme(&mut props);
        assert_eq!(props.color[&token::OVERLAY_BG], palette.overlay_bg);
        assert_eq!(props.color[&token::SELECTION], palette.selection);
    }

    #[test]
    fn the_desktop_is_followed_until_it_is_overruled() {
        let mut mode = ThemeMode::default();
        mode.follow(WindowTheme::Light);
        assert!(!mode.is_dark(), "no choice made yet, so the desktop wins");

        mode.toggle();
        assert!(mode.is_dark());
        mode.follow(WindowTheme::Light);
        assert!(mode.is_dark(), "a chosen theme is not taken back");
    }
}
