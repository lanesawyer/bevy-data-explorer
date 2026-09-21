//! The dataset picker: a search field over every dataset open or on offer.
//!
//! It appears wherever a dataset is chosen. On a frame's title it is the popup
//! of a dropdown that repoints that frame; in View configuration it sits inline
//! and opens what is chosen in a frame of its own; in the Layers section and a
//! frame's `...` menu it draws what is chosen over the frame. One picker in
//! every place, so the lists cannot drift apart.
//!
//! The frame's dropdown is built on Feathers' own menu rather than on
//! [`crate::widgets::spawn_menu`], for what that brings: arrow keys between
//! items, Escape, and dismissal when focus leaves the popup. That last one is
//! also what makes a search field work there — the popup stays open while its
//! field holds the keyboard, and opening it puts the keyboard there, so typing
//! filters straight away.
//!
//! Beyond what is open, it offers every entry of every [`crate::catalog`], one
//! section each — some read over HTTP, and growing as they land — which is why
//! it searches and scrolls rather than assuming it fits.
//!
//! Choosing an item raises a [`SourceChoice`], which lands in
//! [`super::overlay::on_source_chosen`] wherever the picker is.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{
    FeathersMenu, FeathersMenuButton, FeathersMenuItem, FeathersMenuPopup,
};
use bevy_feathers::display::label_dim;
use bevy_feathers::theme::ThemeTextColor;
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, MenuAction, MenuEvent, ScrollArea};

use super::grid::MAX_LAYERS;
use super::layers::{stacked_sources, unit_mismatch};
use super::overlay::{ChoiceAction, PanelTitle, SourceChoice};
use super::{FrameLayers, LayerOf, MAX_PANELS, Panel};
use crate::catalog::Catalogs;
use crate::formats::discover::names_a_table;
use crate::source::table::SourceTable;
use crate::source::{DataSource, ShowsSource, SourceUrl};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, MENU_WIDTH, button_text, field_well, matches_search, size,
    spawn_search_field, text, text_dim, truncate_to_width,
};

/// Where a picker puts what is chosen from it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PickerTarget {
    /// In place of what this frame shows.
    Frame(Entity),
    /// In a frame of its own.
    #[default]
    NewFrame,
    /// Drawn over what this frame shows, as a layer.
    Layer(Entity),
}

/// The list a picker is rebuilt into.
#[derive(Component, Clone)]
pub struct DatasetList {
    picker: Entity,
    target: PickerTarget,
}

impl Default for DatasetList {
    fn default() -> Self {
        DatasetList {
            picker: Entity::PLACEHOLDER,
            target: PickerTarget::NewFrame,
        }
    }
}

/// The field that filters a picker's list. On the inner text entity, which is
/// the one holding the [`EditableText`].
#[derive(Component, Clone)]
pub struct DatasetSearch {
    picker: Entity,
}

impl Default for DatasetSearch {
    fn default() -> Self {
        DatasetSearch {
            picker: Entity::PLACEHOLDER,
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
                    TextFont { font_size: { FontSize::Px(size::FRAME_TITLE) } }
                    Node { margin: { UiRect::right(Val::Px(space::ICON_LABEL)) } }
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
                padding: { UiRect::all(Val::Px(space::PANEL_INSET)) },
            }
        })
        .id();

    let picker = spawn_dataset_picker(commands, PickerTarget::Frame(panel));
    commands.entity(popup).add_child(picker);
    // The popup has to be the menu's own child: that is where Feathers looks
    // for it on open, and what it is positioned against.
    commands.entity(menu).add_children(&[button, popup]);
    menu
}

/// A search field over a list of datasets, returning the column holding both.
pub fn spawn_dataset_picker(commands: &mut Commands, target: PickerTarget) -> Entity {
    let picker = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::ROWS) },
            }
        })
        .id();

    let search = spawn_search_field(commands, "Search datasets");
    commands
        .entity(search.field)
        .insert(DatasetSearch { picker });

    let list = commands
        .spawn_scene(bsn! {
            DatasetList { picker: { picker }, target: { target } }
            ScrollArea
            Node {
                flex_direction: { FlexDirection::Column },
                max_height: { Val::Px(LIST_MAX_PX) },
                overflow: { Overflow::scroll_y() },
            }
        })
        .id();

    // The field would not show against the menu on its own.
    let well = commands.spawn_scene(field_well()).id();
    commands.entity(well).add_child(search.entry);
    commands.entity(picker).add_children(&[well, list]);
    picker
}

/// Everything a list is built from. Rebuilt when any of it changes.
#[derive(Clone, PartialEq)]
pub struct ListState {
    list: Entity,
    /// What its frame shows, bottom first; nothing for a picker that opens
    /// new frames.
    stack: Vec<Entity>,
    query: String,
    sources: usize,
    catalogued: usize,
    /// Whether the grid has room for another frame.
    room: bool,
}

/// Refill each list when what it lists, what its frame shows, or what has been
/// typed into it changes.
///
/// Rebuilt rather than filtered in place, so that arrow keys and Tab only ever
/// move between items that are on screen.
pub fn rebuild_dataset_lists(
    mut commands: Commands,
    lists: Query<(Entity, &DatasetList)>,
    fields: Query<(&DatasetSearch, &EditableText)>,
    panels: Query<(&ShowsSource, Option<&FrameLayers>), With<Panel>>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    sources: Query<(Entity, &DataSource)>,
    tables: Query<(), With<SourceTable>>,
    urls: Query<&SourceUrl>,
    existing: Query<(Entity, &ChildOf), With<DatasetListContent>>,
    catalogs: Res<Catalogs>,
    mut shown: Local<Vec<ListState>>,
) {
    let source_count = sources.iter().count();
    let room = panels.iter().count() < MAX_PANELS;
    let mut current: Vec<ListState> = lists
        .iter()
        .filter_map(|(entity, list)| {
            let stack = match list.target {
                PickerTarget::Frame(panel) | PickerTarget::Layer(panel) => {
                    let (shows, layers) = panels.get(panel).ok()?;
                    stacked_sources(shows, layers, &layer_cameras)
                }
                PickerTarget::NewFrame => Vec::new(),
            };
            let query = fields
                .iter()
                .find(|(search, _)| search.picker == list.picker)
                .map_or_else(String::new, |(_, text)| text.value().to_string());
            Some(ListState {
                list: entity,
                stack,
                query,
                sources: source_count,
                catalogued: catalogs.len(),
                room,
            })
        })
        .collect();
    current.sort_unstable_by_key(|state| state.list);
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

    for state in &current {
        if shown.contains(state) {
            continue;
        }
        let Ok((_, list)) = lists.get(state.list) else {
            continue;
        };
        for (entity, parent) in &existing {
            if parent.parent() == state.list {
                commands.entity(entity).despawn();
            }
        }

        let (panel, show, show_catalog): (_, _, fn(_) -> _) = match list.target {
            PickerTarget::Frame(panel) => (panel, ChoiceAction::Show, ChoiceAction::ShowCatalog),
            PickerTarget::NewFrame => (
                Entity::PLACEHOLDER,
                ChoiceAction::Open,
                ChoiceAction::OpenCatalog,
            ),
            PickerTarget::Layer(panel) => {
                (panel, ChoiceAction::AddLayer, ChoiceAction::LayerCatalog)
            }
        };
        let layering = matches!(list.target, PickerTarget::Layer(_));
        let base = state
            .stack
            .first()
            .and_then(|base| sources.get(*base).ok())
            .map(|(_, data)| data);
        // Nowhere for another frame, or another layer, to go. Repointing a
        // frame needs no room.
        let refuse = match list.target {
            PickerTarget::Frame(_) => false,
            PickerTarget::NewFrame => !state.room,
            PickerTarget::Layer(_) => state.stack.is_empty() || state.stack.len() >= MAX_LAYERS,
        };

        let mut items = Vec::new();
        // Said once, above the dimmed items, so they read as held rather than
        // broken.
        if matches!(list.target, PickerTarget::NewFrame) && !state.room {
            items.push(
                commands
                    .spawn_scene(bsn! {
                        DatasetListContent
                        text_dim(
                            format!("The grid is full at {MAX_PANELS} frames. Close one to open another."),
                            size::SMALL
                        )
                        Node { margin: { UiRect::axes(Val::Px(space::CONTROL_INSET), Val::Px(space::ITEM_INSET)) } }
                    })
                    .id(),
            );
        }
        let noted = items.len();
        let mut section = None;
        for (entity, data, url) in &listed {
            // A layer is offered only if it could go on top: not what the
            // frame already stacks, and never a table, whose rows are records
            // rather than a place to lay anything over.
            if layering && (state.stack.contains(entity) || tables.contains(*entity)) {
                continue;
            }
            if !matches_search(&state.query, &[&data.name, &data.detail, url.unwrap_or("")]) {
                continue;
            }
            if section.is_none() {
                section = Some("Open");
                items.push(heading(&mut commands, "Open", items.len() == noted));
            }
            // Drawn anyway, since nothing rescales a layer, so say why it may
            // not line up rather than leave the two to look aligned by
            // coincidence.
            let note = match base
                .filter(|_| layering)
                .and_then(|base| unit_mismatch(base, data))
            {
                Some(mismatch) => format!("{mismatch}, not rescaled"),
                None => data.detail.clone(),
            };
            let item = item(
                &mut commands,
                &data.name,
                &note,
                SourceChoice {
                    panel,
                    source: *entity,
                    action: show,
                },
                // Listed so the frame's own dataset is there to be found, but
                // choosing it would change nothing.
                refuse || (!layering && state.stack.first() == Some(entity)),
            );
            items.push(item);
        }
        for (id, catalog, entry) in catalogs.unopened(&opened) {
            if layering && names_a_table(&entry.url) {
                continue;
            }
            if !matches_search(
                &state.query,
                &[
                    &entry.name,
                    &entry.kind,
                    &entry.url,
                    &entry.keywords,
                    catalog,
                ],
            ) {
                continue;
            }
            if section != Some(catalog) {
                section = Some(catalog);
                items.push(heading(&mut commands, catalog, items.len() == noted));
            }
            let item = item(
                &mut commands,
                &entry.name,
                &format!("{}, not loaded yet", entry.kind),
                SourceChoice {
                    panel,
                    source: Entity::PLACEHOLDER,
                    action: show_catalog(id),
                },
                refuse,
            );
            items.push(item);
        }
        if items.len() == noted {
            let none = commands
                .spawn_scene(bsn! {
                    DatasetListContent
                    label_dim("Nothing matches")
                    Node { margin: { UiRect::all(Val::Px(space::CONTROL_INSET)) } }
                })
                .id();
            items.push(none);
        }
        commands.entity(state.list).add_children(&items);
    }
    *shown = current;
}

/// The name over one section of a list: what is open, or a catalog.
fn heading(commands: &mut Commands, content: &str, first: bool) -> Entity {
    let content = content.to_string();
    let gap = if first { 2.0 } else { 10.0 };
    commands
        .spawn_scene(bsn! {
            DatasetListContent
            text_dim(content, size::SMALL)
            Node { margin: { UiRect::new(Val::Px(space::CONTROL_INSET), Val::Px(space::CONTROL_INSET), Val::Px(gap), Val::Px(space::STACKED)) } }
        })
        .id()
}

/// One dataset: its name, and what it is on a dimmer line under it.
///
/// Both are cut to fit rather than wrapped, as the layers menu's rows are.
///
/// A disabled item is dimmed here rather than left to Feathers: it greys an
/// item's text through the color the caption inherits, and these lines name
/// their own colors, so an item it had disabled looked no different. It is
/// also left out of picking, item and caption alike, since Feathers lights a
/// hovered item whether or not it can be chosen.
fn item(
    commands: &mut Commands,
    name: &str,
    note: &str,
    choice: SourceChoice,
    disabled: bool,
) -> Entity {
    let name = truncate_to_width(name, ITEM_TEXT_PX, 13.0);
    let note = truncate_to_width(note, ITEM_TEXT_PX, 11.0);
    let (name_color, note_color) = if disabled {
        (
            tokens::MENUITEM_TEXT_DISABLED,
            tokens::MENUITEM_TEXT_DISABLED,
        )
    } else {
        (tokens::TEXT_MAIN, tokens::TEXT_DIM)
    };
    let pickable = if disabled {
        Pickable::IGNORE
    } else {
        Pickable::default()
    };
    let item = commands
        .spawn_scene(bsn! {
            @FeathersMenuItem {
                @caption: { bsn_list![(
                    Node {
                        flex_direction: { FlexDirection::Column },
                        min_width: { Val::Px(0.0) },
                        overflow: { Overflow::clip() },
                    }
                    template_value(pickable)
                    Children [
                        (
                            text(name, size::BODY)
                            ThemeTextColor({ name_color })
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                            template_value(pickable)
                        ),
                        (
                            text_dim(note, size::SMALL)
                            ThemeTextColor({ note_color })
                            TextLayout { linebreak: { LineBreak::NoWrap } }
                            template_value(pickable)
                        ),
                    ]
                )] }
            }
            DatasetListContent
            template_value(choice)
            template_value(pickable)
            // Two lines rather than the one a Feathers item is sized for.
            Node {
                height: { Val::Auto },
                padding: { UiRect::axes(Val::Px(space::CONTROL_INSET), Val::Px(space::ITEM_INSET)) },
            }
        })
        .id();
    if disabled {
        commands.entity(item).insert(InteractionDisabled);
    }
    item
}

/// Start each frame's dropdown afresh: once closed, whatever was typed into it is
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
        .filter(|(list, _)| list.picker == search.picker)
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
