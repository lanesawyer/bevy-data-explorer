//! The dropdown on a frame's title that picks what the frame shows.
//!
//! Built on Feathers' own menu rather than on [`crate::widgets::spawn_menu`],
//! for what that brings: arrow keys between items, Escape, and dismissal when
//! focus leaves the popup. That last one is also what makes a search field work
//! here — the popup stays open while its field holds the keyboard, and opening
//! it puts the keyboard there, so typing filters straight away.
//!
//! The list is expected to grow well past the handful of examples it starts
//! with, from catalogues read over HTTP and from files opened locally, which is
//! why it searches and scrolls rather than assuming it fits.
//!
//! Choosing an item raises the same [`SourceChoice`] the layers menu does, so
//! both land in [`super::overlay::on_source_chosen`].

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{
    FeathersMenu, FeathersMenuButton, FeathersMenuItem, FeathersMenuPopup, FeathersTextInput,
    FeathersTextInputContainer,
};
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{Activate, MenuAction, MenuEvent, ScrollArea};

use super::overlay::{ChoiceAction, PanelTitle, SourceChoice};
use super::{BlocksFrameInput, Panel, ShowsSource};
use crate::source::{DataSource, SourceUrl};
use crate::widgets::{MENU_WIDTH, button_text, truncate_to_width};

/// The list a frame's dropdown is rebuilt into.
#[derive(Component, Clone)]
pub struct DatasetList {
    panel: Entity,
}

impl Default for DatasetList {
    fn default() -> Self {
        DatasetList {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// The field that filters a frame's dropdown. On the inner text entity, which
/// is the one holding the [`EditableText`].
#[derive(Component, Clone)]
pub struct DatasetSearch {
    panel: Entity,
}

impl Default for DatasetSearch {
    fn default() -> Self {
        DatasetSearch {
            panel: Entity::PLACEHOLDER,
        }
    }
}

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

/// Anything spawned into a list, despawned wholesale on a rebuild.
#[derive(Component, Clone, Default)]
pub struct DatasetListContent;

/// Tallest the list grows before it scrolls, in logical pixels.
const LIST_MAX_PX: f32 = 360.0;

/// Room an item's name and caption get: the popup less its padding and the
/// item's own.
const ITEM_TEXT_PX: f32 = MENU_WIDTH - 2.0 * 6.0 - 2.0 * 8.0;

/// Build a frame's title as the button of its dataset dropdown, returning the
/// menu to place in the header.
pub fn spawn_dataset_menu(commands: &mut Commands, panel: Entity) -> Entity {
    let menu = commands
        .spawn_scene(bsn! {
            @FeathersMenu
            Node {
                margin: { UiRect::right(Val::Px(2.0)) },
                flex_shrink: { 1.0_f32 },
                min_width: { Val::Px(0.0) },
            }
        })
        .id();

    let button = commands
        .spawn_scene(bsn! {
            @FeathersMenuButton {
                @caption: { bsn_list![(
                    button_text("")
                    PanelTitle
                    InheritableFont { font_size: { 14.0f32 } }
                    Node { margin: { UiRect::right(Val::Px(4.0)) } }
                )] }
            }
            BlocksFrameInput
        })
        .id();

    let popup = commands
        .spawn_scene(bsn! {
            @FeathersMenuPopup
            BlocksFrameInput
            Node {
                width: { Val::Px(MENU_WIDTH) },
                padding: { UiRect::all(Val::Px(6.0)) },
                row_gap: { Val::Px(4.0) },
            }
        })
        .id();

    // Spawned apart and parented by hand, as the URL field is: Feathers'
    // container is a scene of its own, and the hint has to know the field.
    let field = commands
        .spawn_scene(bsn! {
            @FeathersTextInput
            DatasetSearch { panel: { panel } }
        })
        .id();
    let hint = commands
        .spawn_scene(bsn! {
            label_dim("Search datasets")
            SearchHint { field: { field } }
            Node {
                position_type: { PositionType::Absolute },
                left: { Val::Px(6.0) },
            }
            template_value(Pickable::IGNORE)
        })
        .id();
    let entry = commands
        .spawn_scene(bsn! {
            @FeathersTextInputContainer
            Node { flex_grow: { 0.0_f32 } }
        })
        .id();
    commands.entity(entry).add_children(&[field, hint]);

    let list = commands
        .spawn_scene(bsn! {
            DatasetList { panel: { panel } }
            ScrollArea
            Node {
                flex_direction: { FlexDirection::Column },
                max_height: { Val::Px(LIST_MAX_PX) },
                overflow: { Overflow::scroll_y() },
            }
        })
        .id();

    commands.entity(popup).add_children(&[entry, list]);
    // The popup has to be the menu's own child: that is where Feathers looks
    // for it on open, and what it is positioned against.
    commands.entity(menu).add_children(&[button, popup]);
    menu
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

/// Refill each dropdown when what it lists, what its frame shows, or what has
/// been typed into it changes.
///
/// Rebuilt rather than filtered in place, so that arrow keys and Tab only ever
/// move between items that are on screen.
pub fn rebuild_dataset_lists(
    mut commands: Commands,
    lists: Query<(Entity, &DatasetList)>,
    fields: Query<(&DatasetSearch, &EditableText)>,
    panels: Query<&ShowsSource, With<Panel>>,
    sources: Query<(Entity, &DataSource)>,
    urls: Query<&SourceUrl>,
    existing: Query<(Entity, &ChildOf), With<DatasetListContent>>,
    mut shown: Local<Vec<(Entity, Entity, String, usize)>>,
) {
    let count = sources.iter().count();
    let mut current: Vec<(Entity, Entity, String, usize)> = lists
        .iter()
        .filter_map(|(entity, list)| {
            let shows = panels.get(list.panel).ok()?;
            let query = fields
                .iter()
                .find(|(search, _)| search.panel == list.panel)
                .map_or_else(String::new, |(_, text)| text.value().to_string());
            Some((entity, shows.0, query, count))
        })
        .collect();
    current.sort_unstable_by_key(|(entity, ..)| *entity);
    if *shown == current {
        return;
    }

    let mut listed: Vec<(Entity, &DataSource, Option<&str>)> = sources
        .iter()
        .map(|(entity, data)| {
            (
                entity,
                data,
                urls.get(entity).ok().map(|url| url.0.as_str()),
            )
        })
        .collect();
    listed.sort_by_key(|(_, data, _)| data.layer);
    let opened: Vec<&str> = urls.iter().map(|url| url.0.as_str()).collect();

    for (list, showing, query, _) in &current {
        if shown
            .iter()
            .any(|previous| previous == &(*list, *showing, query.clone(), count))
        {
            continue;
        }
        let Ok((_, DatasetList { panel })) = lists.get(*list) else {
            continue;
        };
        for (entity, parent) in &existing {
            if parent.parent() == *list {
                commands.entity(entity).despawn();
            }
        }

        let mut items = Vec::new();
        for (entity, data, url) in &listed {
            if !matches_search(query, &[&data.name, &data.detail, url.unwrap_or("")]) {
                continue;
            }
            let item = item(
                &mut commands,
                &data.name,
                &data.detail,
                SourceChoice {
                    panel: *panel,
                    source: *entity,
                    action: ChoiceAction::Show,
                },
            );
            // Listed so the frame's own dataset is there to be found, but
            // choosing it would change nothing.
            if showing == entity {
                commands.entity(item).insert(InteractionDisabled);
            }
            items.push(item);
        }
        for (index, example) in crate::formats::unopened_examples(&opened) {
            if !matches_search(query, &[example.name, example.kind, example.url]) {
                continue;
            }
            items.push(item(
                &mut commands,
                example.name,
                &format!("{}, not loaded yet", example.kind),
                SourceChoice {
                    panel: *panel,
                    source: Entity::PLACEHOLDER,
                    action: ChoiceAction::ShowExample(index),
                },
            ));
        }
        if items.is_empty() {
            let none = commands
                .spawn_scene(bsn! {
                    DatasetListContent
                    label_dim("Nothing matches")
                    Node { margin: { UiRect::all(Val::Px(8.0)) } }
                })
                .id();
            items.push(none);
        }
        commands.entity(*list).add_children(&items);
    }
    *shown = current;
}

/// One dataset: its name, and what it is on a dimmer line under it.
///
/// Both are cut to fit rather than wrapped, as the layers menu's rows are.
fn item(commands: &mut Commands, name: &str, note: &str, choice: SourceChoice) -> Entity {
    let name = truncate_to_width(name, ITEM_TEXT_PX, 13.0);
    let note = truncate_to_width(note, ITEM_TEXT_PX, 11.0);
    commands
        .spawn_scene(bsn! {
            @FeathersMenuItem {
                @caption: { bsn_list![(
                    Node {
                        flex_direction: { FlexDirection::Column },
                        min_width: { Val::Px(0.0) },
                        overflow: { Overflow::clip() },
                    }
                    Children [
                        (
                            label(name)
                            InheritableFont { font_size: { 13.0f32 } }
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        ),
                        (
                            label_dim(note)
                            InheritableFont { font_size: { 11.0f32 } }
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                        ),
                    ]
                )] }
            }
            DatasetListContent
            template_value(choice)
            // Two lines rather than the one a Feathers item is sized for.
            Node {
                height: { Val::Auto },
                padding: { UiRect::axes(Val::Px(8.0), Val::Px(4.0)) },
            }
        })
        .id()
}

/// Show the hint only while its field is empty.
pub fn sync_search_hints(
    fields: Query<&EditableText, With<DatasetSearch>>,
    mut hints: Query<(&SearchHint, &mut Node)>,
) {
    for (hint, mut node) in &mut hints {
        let empty = fields
            .get(hint.field)
            .is_ok_and(|text| text.value().to_string().is_empty());
        let wanted = if empty { Display::Flex } else { Display::None };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Start each dropdown afresh: once closed, whatever was typed into it is
/// cleared, so the next time it opens it lists everything.
pub fn clear_closed_searches(
    popups: Query<(Entity, &Visibility), (With<FeathersMenuPopup>, Changed<Visibility>)>,
    parents: Query<&ChildOf>,
    mut fields: Query<(Entity, &mut EditableText), With<DatasetSearch>>,
) {
    for (popup, visibility) in &popups {
        if *visibility != Visibility::Hidden {
            continue;
        }
        for (field, mut text) in &mut fields {
            if !text.value().to_string().is_empty()
                && parents
                    .iter_ancestors(field)
                    .any(|ancestor| ancestor == popup)
            {
                text.clear();
            }
        }
    }
}

/// The keys the search field answers for the menu around it.
///
/// Feathers' menu handles keys only while an item or the popup itself has
/// focus, and here the field has it: Enter takes the first match, the down
/// arrow moves into the list, and Escape closes the menu.
pub fn on_search_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    fields: Query<(), With<DatasetSearch>>,
    lists: Query<(&DatasetList, &Children)>,
    searches: Query<&DatasetSearch>,
    items: Query<Has<InteractionDisabled>, With<SourceChoice>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let field = key.focused_entity;
    if !fields.contains(field) || key.input.state != ButtonState::Pressed {
        return;
    }
    let Ok(search) = searches.get(field) else {
        return;
    };
    let mut in_list = lists
        .iter()
        .filter(|(list, _)| list.panel == search.panel)
        .flat_map(|(_, children)| children.iter())
        .filter(|item| items.get(*item).is_ok_and(|disabled| !disabled));

    match key.input.key_code {
        KeyCode::Enter | KeyCode::NumpadEnter => {
            key.propagate(false);
            if let Some(first) = in_list.next() {
                commands.trigger(Activate { entity: first });
                commands.trigger(MenuEvent {
                    source: field,
                    action: MenuAction::FocusRoot,
                });
                commands.trigger(MenuEvent {
                    source: field,
                    action: MenuAction::CloseAll,
                });
            }
        }
        KeyCode::ArrowDown => {
            key.propagate(false);
            if let Some(first) = in_list.next() {
                focus.set(first, FocusCause::Navigated);
            }
        }
        KeyCode::Escape => {
            key.propagate(false);
            commands.trigger(MenuEvent {
                source: field,
                action: MenuAction::FocusRoot,
            });
            commands.trigger(MenuEvent {
                source: field,
                action: MenuAction::CloseAll,
            });
        }
        _ => {}
    }
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
