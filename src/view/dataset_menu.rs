//! The dataset picker: a search field over every dataset open or on offer.
//!
//! It appears wherever a dataset is chosen. On a frame's title it is the popup
//! of a dropdown that repoints that frame; filling an empty frame it is the
//! browser that finds what the frame will show; in the Layers section and a
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
//!
//! An empty frame holds a larger picker, [`spawn_dataset_browser`], with
//! buttons narrowing it to one [`Category`], and another row keeping it to
//! one source. That is the frame's own narrowing, not settings' switch: the
//! other sources are only left out of this list and are not searched for it.
//! Whatever is typed that looks like
//! an address is offered as one to read, ahead of any match, so the search is
//! also where a URL is pasted.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersMenu, FeathersMenuButton, FeathersMenuItem,
    FeathersMenuPopup,
};
use bevy_feathers::display::label_dim;
use bevy_feathers::rounded_corners::RoundedCorners;
use bevy_feathers::theme::{ThemeBorderColor, ThemeTextColor};
use bevy_feathers::tokens;
use bevy_ui_widgets::{Activate, MenuAction, MenuEvent, ScrollArea};

use super::grid::MAX_LAYERS;
use super::layers::{stacked_sources, unit_mismatch};
use super::overlay::{ChoiceAction, ChoiceUrl, PanelTitle, SourceChoice};
use super::{DatasetTarget, FrameLayers, LayerOf, MAX_PANELS, Panel, PanelRequest};
use crate::catalog::Catalogs;
use crate::formats::discover::names_a_table;
use crate::source::table::SourceTable;
use crate::source::{Category, DataSource, ShowsSource, SourceUrl};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, MENU_WIDTH, button_text, field_well, matches_search, patch_node, size,
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
    /// Whether it fills a frame, where its section headings have room to
    /// stand out, rather than a dropdown.
    browser: bool,
}

impl Default for DatasetList {
    fn default() -> Self {
        DatasetList {
            picker: Entity::PLACEHOLDER,
            target: PickerTarget::NewFrame,
            browser: false,
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

/// The category a picker is narrowed to, if any. On the picker's column.
#[derive(Component, Clone, Copy, Default, PartialEq)]
pub struct PickerFilter(pub Option<Category>);

/// A button narrowing a picker to one category, or widening it to all.
#[derive(Component, Clone)]
pub struct FilterChip {
    picker: Entity,
    only: Option<Category>,
}

impl Default for FilterChip {
    fn default() -> Self {
        FilterChip {
            picker: Entity::PLACEHOLDER,
            only: None,
        }
    }
}

/// The source a picker is kept to, if any, by its key in
/// [`Catalogs::sources`]. On the picker's column.
#[derive(Component, Clone, Default, PartialEq)]
pub struct PickerSource(pub Option<String>);

/// A button keeping a picker to one source, or opening it to all.
#[derive(Component, Clone)]
pub struct SourceChip {
    picker: Entity,
    only: Option<String>,
}

impl Default for SourceChip {
    fn default() -> Self {
        SourceChip {
            picker: Entity::PLACEHOLDER,
            only: None,
        }
    }
}

/// The dropdown item that turns a frame into a browser for what it shows.
#[derive(Component, Clone)]
pub struct BrowseItem {
    panel: Entity,
}

impl Default for BrowseItem {
    fn default() -> Self {
        BrowseItem {
            panel: Entity::PLACEHOLDER,
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

    // Ahead of the picker, which is short, for when a longer look is wanted.
    let browse = commands
        .spawn_scene(bsn! {
            @FeathersMenuItem {
                @caption: { bsn_list![button_text("Browse all datasets\u{2026}")] }
            }
            BrowseItem { panel: { panel } }
        })
        .id();
    let picker = spawn_dataset_picker(commands, PickerTarget::Frame(panel));
    commands.entity(popup).add_children(&[browse, picker]);
    // The popup has to be the menu's own child: that is where Feathers looks
    // for it on open, and what it is positioned against.
    commands.entity(menu).add_children(&[button, popup]);
    menu
}

/// A search field over a list of datasets, returning the column holding both.
pub fn spawn_dataset_picker(commands: &mut Commands, target: PickerTarget) -> Entity {
    let (picker, _) = spawn_picker(commands, target, None);
    picker
}

/// A picker that fills whatever holds it, with buttons narrowing it by
/// category: what an empty frame shows. Returns the column and its search
/// field, which is given the keyboard as the frame opens.
pub fn spawn_dataset_browser(
    commands: &mut Commands,
    catalogs: &Catalogs,
    target: PickerTarget,
) -> (Entity, Entity) {
    spawn_picker(commands, target, Some(catalogs))
}

/// `browser` is the catalogs whose sources a browser offers to keep to;
/// nothing for a dropdown's picker, which has no room for its buttons.
fn spawn_picker(
    commands: &mut Commands,
    target: PickerTarget,
    browser: Option<&Catalogs>,
) -> (Entity, Entity) {
    let fill = browser.is_some();
    let picker = commands
        .spawn_scene(bsn! {
            PickerFilter
            PickerSource
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::ROWS) },
            }
        })
        .id();

    let hint = if fill {
        "Search datasets, or paste a URL"
    } else {
        "Search datasets"
    };
    let search = spawn_search_field(commands, hint);
    commands
        .entity(search.field)
        .insert(DatasetSearch { picker });

    let list = commands
        .spawn_scene(bsn! {
            DatasetList { picker: { picker }, target: { target }, browser: { fill } }
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
    if let Some(catalogs) = browser {
        let chips = spawn_filter_chips(commands, picker);
        let sources = spawn_source_chips(commands, catalogs, picker);
        commands.entity(well).add_children(&[chips, sources]);
        // The list takes whatever height the frame has, and scrolls within it.
        commands.entity(picker).insert(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            row_gap: Val::Px(space::ROWS),
            ..default()
        });
        commands.entity(list).insert(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            overflow: Overflow::scroll_y(),
            ..default()
        });
    }
    commands.entity(picker).add_children(&[well, list]);
    (picker, search.field)
}

/// One button per category and one for all of them, joined into one control
/// as the theme buttons in settings are.
fn spawn_filter_chips(commands: &mut Commands, picker: Entity) -> Entity {
    let options: Vec<(Option<Category>, &'static str)> = std::iter::once((None, "All"))
        .chain(Category::ALL.map(|category| (Some(category), category.plural())))
        .collect();
    spawn_chip_row(commands, options, |only| FilterChip { picker, only })
}

/// One button per source a picker can be kept to, and one for all of them.
///
/// Every source is given a button, turned on or not, and those turned off in
/// settings are hidden by [`sync_source_chips`], so switching one back on
/// needs no rebuild.
fn spawn_source_chips(commands: &mut Commands, catalogs: &Catalogs, picker: Entity) -> Entity {
    let options: Vec<(Option<String>, String)> = std::iter::once((None, "All sources".to_string()))
        .chain(
            catalogs
                .sources()
                .into_iter()
                .map(|(key, name)| (Some(key.to_string()), name.to_string())),
        )
        .collect();
    spawn_chip_row(commands, options, |only| SourceChip { picker, only })
}

/// Buttons joined into one control as the theme buttons in settings are,
/// each carrying the chip `chip` makes of its option.
fn spawn_chip_row<T, C: Component>(
    commands: &mut Commands,
    options: Vec<(T, impl Into<String>)>,
    chip: impl Fn(T) -> C,
) -> Entity {
    let row = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                column_gap: { Val::Px(space::SEAM) },
            }
        })
        .id();
    let last = options.len().saturating_sub(1);
    let chips: Vec<Entity> = options
        .into_iter()
        .enumerate()
        .map(|(at, (only, label))| {
            let label: String = label.into();
            let corners = match at {
                0 => RoundedCorners::Left,
                _ if at == last => RoundedCorners::Right,
                _ => RoundedCorners::None,
            };
            let button = commands
                .spawn_scene(bsn! {
                    @FeathersButton {
                        @caption: { bsn_list![button_text(label)] },
                        @corners: { corners }
                    }
                    Node { flex_grow: { 1.0_f32 } }
                })
                .id();
            commands.entity(button).insert(chip(only));
            button
        })
        .collect();
    commands.entity(row).add_children(&chips);
    row
}

/// Narrow a picker to the category its button names.
pub fn on_filter_chip(
    activate: On<Activate>,
    chips: Query<&FilterChip>,
    mut filters: Query<&mut PickerFilter>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    if let Ok(mut filter) = filters.get_mut(chip.picker) {
        filter.set_if_neq(PickerFilter(chip.only));
    }
}

/// Mark the button of the category each picker is narrowed to.
pub fn sync_filter_chips(
    filters: Query<&PickerFilter>,
    mut chips: Query<(&FilterChip, &mut ButtonVariant)>,
) {
    for (chip, mut variant) in &mut chips {
        let chosen = filters
            .get(chip.picker)
            .is_ok_and(|filter| filter.0 == chip.only);
        variant.set_if_neq(if chosen {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        });
    }
}

/// Keep a picker to the source its button names.
pub fn on_source_chip(
    activate: On<Activate>,
    chips: Query<&SourceChip>,
    mut sources: Query<&mut PickerSource>,
) {
    let Ok(chip) = chips.get(activate.entity) else {
        return;
    };
    if let Ok(mut source) = sources.get_mut(chip.picker) {
        source.set_if_neq(PickerSource(chip.only.clone()));
    }
}

/// Mark the button of the source each picker is kept to, and hide those of
/// sources turned off in settings. A picker kept to one turned off is opened
/// to all again rather than left listing nothing.
pub fn sync_source_chips(
    catalogs: Res<Catalogs>,
    mut sources: Query<&mut PickerSource>,
    mut chips: Query<(&SourceChip, &mut ButtonVariant, &mut Node)>,
) {
    for mut source in &mut sources {
        if source
            .0
            .as_deref()
            .is_some_and(|key| !catalogs.source_on(key))
        {
            source.0 = None;
        }
    }
    for (chip, mut variant, node) in &mut chips {
        let chosen = sources
            .get(chip.picker)
            .is_ok_and(|source| source.0 == chip.only);
        variant.set_if_neq(if chosen {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        });
        let display = if chip
            .only
            .as_deref()
            .is_none_or(|key| catalogs.source_on(key))
        {
            Display::Flex
        } else {
            Display::None
        };
        patch_node(node, |node| node.display = display);
    }
}

/// Turn a frame into a browser for what it shows, from its dropdown.
pub fn on_browse_item(
    activate: On<Activate>,
    items: Query<&BrowseItem>,
    mut requests: MessageWriter<PanelRequest>,
) {
    if let Ok(item) = items.get(activate.entity) {
        requests.write(PanelRequest::Browse(Some(item.panel)));
    }
}

/// Whether what was typed is an address to read rather than words to search
/// for: a URL of any scheme, or a path on this machine.
pub fn looks_like_address(text: &str) -> bool {
    let text = text.trim();
    !text.contains(char::is_whitespace)
        && (text.contains("://")
            || text.starts_with('/')
            || text.starts_with("./")
            || text.starts_with("../"))
}

/// Everything a list is built from. Rebuilt when any of it changes.
#[derive(Clone, PartialEq)]
pub struct ListState {
    list: Entity,
    /// What its frame shows, bottom first; nothing for a picker that opens
    /// new frames.
    stack: Vec<Entity>,
    query: String,
    only: Option<Category>,
    from: Option<String>,
    sources: usize,
    /// The catalogs' generation.
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
    filters: Query<&PickerFilter>,
    picker_sources: Query<&PickerSource>,
    // An empty frame shows no source but still holds a picker, so the source
    // is optional here.
    panels: Query<(Option<&ShowsSource>, Option<&FrameLayers>), With<Panel>>,
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
                    shows.map_or_else(Vec::new, |shows| {
                        stacked_sources(shows, layers, &layer_cameras)
                    })
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
                only: filters.get(list.picker).ok().and_then(|filter| filter.0),
                from: picker_sources
                    .get(list.picker)
                    .ok()
                    .and_then(|source| source.0.clone()),
                sources: source_count,
                catalogued: catalogs.generation(),
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
        let from = state.from.as_deref();
        let big = list.browser;
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
        // An address is offered ahead of anything it happens to match, so
        // that Enter reads what was pasted.
        if looks_like_address(&state.query) && !refuse {
            let url = state.query.trim().to_string();
            let target = match list.target {
                PickerTarget::Frame(panel) => DatasetTarget::Show(panel),
                PickerTarget::NewFrame => DatasetTarget::NewFrame,
                PickerTarget::Layer(panel) => DatasetTarget::Layer(panel),
            };
            section = Some("Address");
            items.push(heading(&mut commands, "Address", true, big));
            let item = item(
                &mut commands,
                &url,
                "Read what is at this address",
                SourceChoice {
                    panel,
                    source: Entity::PLACEHOLDER,
                    action: ChoiceAction::Address(target),
                },
                false,
            );
            commands.entity(item).insert(ChoiceUrl(url));
            items.push(item);
        }
        for (entity, data, url) in &listed {
            // A layer is offered only if it could go on top: not what the
            // frame already stacks, and never a table, whose rows are records
            // rather than a place to lay anything over.
            if layering && (state.stack.contains(entity) || tables.contains(*entity)) {
                continue;
            }
            // What is open is kept to a source by the catalog that lists it;
            // one no catalog lists, such as a pasted address, belongs to none.
            if !matches_search(&state.query, &[&data.name, &data.detail, url.unwrap_or("")])
                || state.only.is_some_and(|only| data.category != only)
                || from
                    .is_some_and(|from| url.and_then(|url| catalogs.source_of(url)) != Some(from))
            {
                continue;
            }
            if section.is_none() {
                section = Some("Open");
                items.push(heading(&mut commands, "Open", items.len() == noted, big));
            }
            // Drawn anyway, since nothing rescales a layer, so say why it may
            // not line up rather than leave the two to look aligned by
            // coincidence.
            let note = match base
                .filter(|_| layering)
                .and_then(|base| unit_mismatch(base, data))
            {
                Some(mismatch) => format!("{mismatch}, not rescaled"),
                None => format!("{} \u{00b7} already open", data.detail),
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
        for (id, catalog, entry) in catalogs.unopened(&opened, &state.query, state.only, from) {
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
                items.push(heading(&mut commands, catalog, items.len() == noted, big));
            }
            let item = item(
                &mut commands,
                &entry.name,
                // Named on every row as well as over the section, since a
                // filtered list can hold one row of a catalog far down it.
                &format!("{} \u{00b7} {catalog}", entry.kind),
                SourceChoice {
                    panel,
                    source: Entity::PLACEHOLDER,
                    action: show_catalog(id),
                },
                refuse,
            );
            items.push(item);
        }
        // A searched catalog has nothing to offer until asked, so it says what
        // it is doing under its heading instead.
        for (catalog, note) in catalogs.search_notes(&state.query, state.only, from) {
            let Some(note) = note else { continue };
            if section != Some(catalog) {
                section = Some(catalog);
                items.push(heading(&mut commands, catalog, items.len() == noted, big));
            }
            let note = note.text();
            items.push(
                commands
                    .spawn_scene(bsn! {
                        DatasetListContent
                        text_dim(note, size::SMALL)
                        Node { margin: { UiRect::axes(Val::Px(space::CONTROL_INSET), Val::Px(space::ITEM_INSET)) } }
                    })
                    .id(),
            );
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
///
/// In a browser, where a list runs long and one catalog's rows look much like
/// the next's, it is set at a heading's size under a rule, so a new section is
/// seen in passing rather than read for.
fn heading(commands: &mut Commands, content: &str, first: bool, big: bool) -> Entity {
    let content = content.to_string();
    if !big {
        let gap = if first { 2.0 } else { 10.0 };
        return commands
            .spawn_scene(bsn! {
                DatasetListContent
                text_dim(content, size::SMALL)
                Node { margin: { UiRect::new(Val::Px(space::CONTROL_INSET), Val::Px(space::CONTROL_INSET), Val::Px(gap), Val::Px(space::STACKED)) } }
            })
            .id();
    }
    let (gap, rule) = if first { (2.0, 0.0) } else { (18.0, 1.0) };
    commands
        .spawn_scene(bsn! {
            DatasetListContent
            Node {
                margin: { UiRect::new(Val::Px(space::CONTROL_INSET), Val::Px(space::CONTROL_INSET), Val::Px(gap), Val::Px(space::ROWS)) },
                padding: { UiRect::top(Val::Px(if first { 0.0 } else { 10.0 })) },
                border: { UiRect::top(Val::Px(rule)) },
            }
            ThemeBorderColor({ tokens::GROUP_BODY_BORDER })
            Children [
                text(content, size::DOCK_TITLE)
            ]
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

/// Hand what is typed into a picker, and the category it is narrowed to, to
/// the searched catalogs, which ask once it settles.
///
/// Only the picker last typed in: every other picker's field, empty in a frame
/// just opened or cleared as its dropdown closed, would otherwise ask in its
/// place and take its answer away. Remembered rather than read off the focus,
/// so that pressing one of its category buttons, which takes the focus, still
/// narrows its search.
pub fn search_catalogs(
    focus: Res<InputFocus>,
    fields: Query<(Entity, &DatasetSearch, &EditableText)>,
    filters: Query<&PickerFilter>,
    sources: Query<&PickerSource>,
    mut catalogs: ResMut<Catalogs>,
    time: Res<Time>,
    mut typing_in: Local<Option<Entity>>,
) {
    if let Some(field) = focus.get().filter(|field| fields.contains(*field)) {
        *typing_in = Some(field);
    }
    let Some((_, search, text)) = typing_in.and_then(|field| fields.get(field).ok()) else {
        return;
    };
    let only = filters.get(search.picker).ok().and_then(|filter| filter.0);
    let from = sources
        .get(search.picker)
        .ok()
        .and_then(|source| source.0.as_deref());
    catalogs.want(&text.value().to_string(), only, from, time.elapsed_secs());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty frame shows no source, and its list was once skipped for it:
    /// the browser opened blank and stayed blank whatever was typed.
    #[test]
    fn an_empty_frames_list_is_filled() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<Font>()
        .init_resource::<Catalogs>()
        .add_systems(Update, rebuild_dataset_lists);
        crate::source::register_in(
            app.world_mut(),
            crate::source::SourceInfo {
                name: "Slide".into(),
                unit: "px".into(),
                detail: String::new(),
                stat: String::new(),
                category: Category::Image,
            },
            crate::source::SourceExtent {
                centre: Vec2::ZERO,
                size: Vec2::splat(10.0),
                finest: 1.0,
            },
        );
        let panel = app
            .world_mut()
            .spawn((Panel { index: 0 }, super::super::Browsing))
            .id();
        let picker = app.world_mut().spawn(PickerFilter(None)).id();
        let list = app
            .world_mut()
            .spawn(DatasetList {
                picker,
                target: PickerTarget::Frame(panel),
                browser: true,
            })
            .id();
        app.update();
        app.update();

        let mut content = app
            .world_mut()
            .query_filtered::<&ChildOf, With<DatasetListContent>>();
        let listed = content
            .iter(app.world())
            .filter(|parent| parent.parent() == list)
            .count();
        assert!(listed >= 2, "a heading and the open slide, found {listed}");
    }

    #[test]
    fn an_address_is_told_apart_from_words_to_search_for() {
        assert!(looks_like_address("https://store/a.zarr/"));
        assert!(looks_like_address("  s3://bucket/key.zarr "));
        assert!(looks_like_address("zarr2://s3://bucket/key"));
        assert!(looks_like_address("/data/cells.csv"));
        assert!(looks_like_address("./cells.csv"));
        assert!(!looks_like_address("smartspim 695466"));
        assert!(!looks_like_address("zarr"));
        assert!(!looks_like_address(""));
    }
}
