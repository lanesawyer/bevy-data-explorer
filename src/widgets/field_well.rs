use bevy::app::Propagate;
use bevy::prelude::*;
use bevy_feathers::theme::{SurfaceLevel, ThemeBackgroundColor, ThemeBorderColor, ThemeContext};
use bevy_feathers::tokens;

use super::CORNER_PX;
use super::space;

/// A box to set text fields in.
///
/// Feathers fills a field with the same gray as a pane's body and a menu, so
/// a field placed straight in either could not be seen. It is drawn to sit in
/// a group, a step lighter, and this is that group's body standing alone —
/// saying so with its surface level, so a field in it is lifted a step
/// lighter again rather than drawn in the well's own gray.
pub fn field_well() -> impl Scene {
    bsn! {
        Node {
            flex_direction: { FlexDirection::Column },
            width: { Val::Percent(100.0) },
            row_gap: { Val::Px(space::ROWS) },
            padding: { UiRect::all(Val::Px(space::CONTROL_INSET)) },
            border: { UiRect::all(Val::Px(1.0)) },
            border_radius: { BorderRadius::all(Val::Px(CORNER_PX)) },
        }
        ThemeBackgroundColor({ tokens::GROUP_BG })
        ThemeBorderColor({ tokens::GROUP_BORDER })
        Propagate::<ThemeContext>(ThemeContext(SurfaceLevel::Highest))
    }
}
