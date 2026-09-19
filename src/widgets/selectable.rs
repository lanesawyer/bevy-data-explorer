//! Text that can be selected and copied but not edited.
//!
//! Bevy selects text only inside an [`EditableText`], so read-only text is an
//! editable one whose edits are sifted before they are applied: whatever moves
//! the cursor, selects or copies goes through, and whatever would change the
//! text is dropped. Cut becomes copy, since that is the half of it that still
//! makes sense.
//!
//! An editable text is as tall as its node says, not as its text, and scrolls
//! itself only to follow the cursor. This one is grown to fit its text
//! instead, so it sits in an ordinary scroll area that the wheel moves.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, TextCursorStyle, TextEdit};
use bevy::window::SystemCursorIcon;
use bevy_feathers::cursor::EntityCursor;
use bevy_feathers::theme::UiTheme;
use bevy_feathers::tokens;

use crate::app::schedule::Stage;

/// Marks an [`EditableText`] as selectable only.
///
/// It is not typed into, so it does not take the keyboard from the frame
/// shortcuts the way a text field does.
///
/// The `TabIndex` is what keeps a selection. A click gives focus to the
/// nearest thing with one, and without it that is the window, which takes
/// focus off the text and so collapses the selection as it is made.
#[derive(Component, Clone, Default)]
#[require(
    TextCursorStyle,
    TabIndex,
    EntityCursor::System(SystemCursorIcon::Text)
)]
pub struct SelectableText;

/// Whether an edit leaves the text as it was, and so may be applied.
fn keeps_text(edit: &TextEdit) -> bool {
    matches!(
        edit,
        TextEdit::Copy
            | TextEdit::Left(_)
            | TextEdit::Right(_)
            | TextEdit::WordLeft(_)
            | TextEdit::WordRight(_)
            | TextEdit::Up(_)
            | TextEdit::Down(_)
            | TextEdit::TextStart(_)
            | TextEdit::TextEnd(_)
            | TextEdit::HardLineStart(_)
            | TextEdit::HardLineEnd(_)
            | TextEdit::LineStart(_)
            | TextEdit::LineEnd(_)
            | TextEdit::CollapseSelection
            | TextEdit::SelectAll
            | TextEdit::SelectAllIfCollapsed
            | TextEdit::MoveToPoint(_)
            | TextEdit::SelectWordAtPoint(_)
            | TextEdit::SelectLineAtPoint(_)
            | TextEdit::SelectedHardLineAtPoint(_)
            | TextEdit::ExtendSelectionToPoint(_)
            | TextEdit::ShiftClickExtension(_)
    )
}

fn sift_edits(mut texts: Query<&mut EditableText, With<SelectableText>>) {
    for mut text in &mut texts {
        if text.pending_edits.is_empty() {
            continue;
        }
        let edits = std::mem::take(&mut text.pending_edits);
        text.pending_edits = edits
            .into_iter()
            .map(|edit| match edit {
                TextEdit::Cut => TextEdit::Copy,
                edit => edit,
            })
            .filter(keeps_text)
            .collect();
    }
}

/// Whether replacing the text would throw away a selection in it.
pub fn has_selection(text: &EditableText) -> bool {
    !text.editor().raw_selection().is_collapsed()
}

/// Grow the node to the height of its text, which the layout measures in
/// physical pixels.
fn fit_to_text(mut texts: Query<(&EditableText, &ComputedNode, &mut Node), With<SelectableText>>) {
    for (text, computed, mut node) in &mut texts {
        let Some(layout) = text.editor().try_layout() else {
            continue;
        };
        let height = Val::Px((layout.height() * computed.inverse_scale_factor).ceil());
        if node.height != height {
            node.height = height;
        }
    }
}

/// Selection in the text inputs' colors. No cursor: there is nothing to type
/// where it would stand.
fn style_selection(
    theme: Res<UiTheme>,
    mut styles: Query<(&mut TextCursorStyle, Ref<SelectableText>)>,
) {
    for (mut style, added) in &mut styles {
        if !theme.is_changed() && !added.is_added() {
            continue;
        }
        *style = TextCursorStyle {
            color: Color::NONE,
            selection_color: theme.color(&tokens::TEXT_INPUT_SELECTION),
            unfocused_selection_color: theme.color(&tokens::TEXT_INPUT_SELECTION_UNFOCUSED),
            selected_text_color: None,
        };
    }
}

pub struct SelectableTextPlugin;

impl Plugin for SelectableTextPlugin {
    fn build(&self, app: &mut App) {
        // Edits are queued by input observers any time before this and applied
        // in `EditableTextSystems`, so this is the one point that sees them all.
        app.add_systems(PostUpdate, sift_edits.before(EditableTextSystems))
            .add_systems(
                Update,
                (fit_to_text, style_selection).in_set(Stage::ControlsPlace),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_and_copying_pass_and_changing_the_text_does_not() {
        assert!(keeps_text(&TextEdit::Copy));
        assert!(keeps_text(&TextEdit::ExtendSelectionToPoint(Vec2::ZERO)));
        assert!(keeps_text(&TextEdit::Left(true)));
        assert!(!keeps_text(&TextEdit::Insert("x".into())));
        assert!(!keeps_text(&TextEdit::Paste));
        assert!(!keeps_text(&TextEdit::Backspace));
        assert!(!keeps_text(&TextEdit::Cut));
    }
}
