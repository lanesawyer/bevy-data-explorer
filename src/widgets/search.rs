//! A search field, and the matching every list searched through it shares.
//!
//! Bevy's text input has no placeholder, so the field carries a hint of its
//! own, laid over it and shown only while it is empty, and a button to clear
//! it shown only while it is not. Whatever the field
//! searches reads its [`EditableText`] and decides for itself when to act on
//! it.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_feathers::controls::{
    ButtonVariant, FeathersTextInput, FeathersTextInputContainer, FeathersToolButton,
};
use bevy_feathers::display::label_dim;
use bevy_ui_widgets::Activate;

use super::{BlocksFrameInput, Icon, button_icon};

/// The clear button's size: smaller than Feathers' row, so it sits inside the
/// field rather than filling it.
const CLEAR_PX: f32 = 18.0;

/// Stands in for the placeholder text Bevy's field does not have yet, shown
/// only while the field is empty.
#[derive(Component, Clone)]
pub struct SearchHint {
    field: Entity,
}

impl Default for SearchHint {
    fn default() -> Self {
        SearchHint {
            field: Entity::PLACEHOLDER,
        }
    }
}

/// The button that empties a search field.
#[derive(Component, Clone)]
pub struct ClearSearch {
    field: Entity,
}

impl Default for ClearSearch {
    fn default() -> Self {
        ClearSearch {
            field: Entity::PLACEHOLDER,
        }
    }
}

/// The parts of a spawned search field.
pub struct SearchField {
    /// The box to place, holding the field and its hint.
    pub entry: Entity,
    /// The inner text entity, which holds the [`EditableText`]. Mark it with
    /// whatever finds it.
    pub field: Entity,
}

/// Spawn a search field reading `hint` while empty.
///
/// A field sitting straight in a pane or a menu needs setting in a
/// [`super::field_well`] to be seen; one in a group does not.
pub fn spawn_search_field(commands: &mut Commands, hint: impl Into<String>) -> SearchField {
    let hint = hint.into();
    // Spawned apart and parented by hand: Feathers' container is a scene of
    // its own, and the hint has to know the field.
    let field = commands.spawn_scene(bsn! { @FeathersTextInput }).id();
    let hint = commands
        .spawn_scene(bsn! {
            label_dim(hint)
            SearchHint { field: { field } }
            Node {
                position_type: { PositionType::Absolute },
                left: { Val::Px(6.0) },
            }
            template_value(Pickable::IGNORE)
        })
        .id();
    let clear = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @variant: { ButtonVariant::Plain },
                @caption: { bsn_list![button_icon(Icon::X)] }
            }
            BlocksFrameInput
            ClearSearch { field: { field } }
            Node {
                display: { Display::None },
                flex_shrink: { 0.0_f32 },
                width: { Val::Px(CLEAR_PX) },
                min_width: { Val::Px(CLEAR_PX) },
                height: { Val::Px(CLEAR_PX) },
                padding: { UiRect::ZERO },
            }
        })
        .id();
    let entry = commands
        .spawn_scene(bsn! {
            @FeathersTextInputContainer
            BlocksFrameInput
            Node { flex_grow: { 0.0_f32 } }
        })
        .id();
    commands.entity(entry).add_children(&[field, hint, clear]);
    SearchField { entry, field }
}

/// Show each hint only while its field is empty, and each clear button only
/// while it is not.
pub fn sync_search_hints(
    fields: Query<&EditableText>,
    mut hints: Query<(&SearchHint, &mut Node), Without<ClearSearch>>,
    mut clears: Query<(&ClearSearch, &mut Node), Without<SearchHint>>,
) {
    let empty = |field: Entity| {
        fields
            .get(field)
            .is_ok_and(|text| text.value().to_string().is_empty())
    };
    for (hint, mut node) in &mut hints {
        let wanted = if empty(hint.field) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
    for (clear, mut node) in &mut clears {
        let wanted = if empty(clear.field) {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Empty the field whose clear button was pressed. Whatever it searches sees
/// the text change as it would a keystroke.
pub fn on_clear_search(
    activate: On<Activate>,
    clears: Query<&ClearSearch>,
    mut fields: Query<&mut EditableText>,
) {
    let Ok(clear) = clears.get(activate.entity) else {
        return;
    };
    if let Ok(mut text) = fields.get_mut(clear.field) {
        text.clear();
    }
}

/// Whether every word of `query` appears somewhere in `fields`, ignoring case.
///
/// Words rather than the whole query, so "zarr mouse" finds a dataset whose
/// kind says one and whose name says the other.
pub fn matches_search(query: &str, fields: &[&str]) -> bool {
    let haystack = fields.join(" ").to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_search_lists_everything() {
        assert!(matches_search("", &["Anything", "at all"]));
        assert!(matches_search("   ", &["Anything"]));
    }

    #[test]
    fn every_word_has_to_appear_but_not_in_the_same_field() {
        let fields = ["SEA-AD slide", "Deep Zoom image", "https://store/a.dzi"];
        assert!(matches_search("deep sea", &fields));
        assert!(matches_search("DZI", &fields));
        assert!(!matches_search("deep zarr", &fields));
    }
}
