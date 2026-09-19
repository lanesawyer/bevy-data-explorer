//! The bookmarks section of the sidebar: saving what is on screen, the list of
//! what has been saved, and handing a bookmark to someone else.
//!
//! A bookmark leaves as a file, through the export dialog, or as one line of
//! text on the clipboard, and comes back in either way. What comes in is added
//! to the list as well as opened, since a bookmark someone sent is one worth
//! keeping.

use std::path::PathBuf;

use bevy::clipboard::Clipboard;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
    FeathersToolButton,
};
use bevy_feathers::display::{label, label_dim};
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::app::theme::Palette;
use crate::bookmark::codec::{from_text, to_line};
use crate::bookmark::store::{FileDialog, SavedBookmarks};
use crate::bookmark::{BookmarkNotice, local_addresses, open_shared, restore, save_current};
use crate::source::DataSource;
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::{BlocksFrameInput, Panel, ShowsSource};
use crate::widgets::{
    Icon, SectionLevel, button_icon, button_text, caption, field_well, spawn_accordion,
};

/// After the sections that act on a frame: this acts on all of them.
const SECTION_ORDER: u32 = 30;

/// The field a new bookmark's name is typed into.
#[derive(Component, Clone, Default)]
pub struct BookmarkNameInput;

/// The buttons that act on no one bookmark.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookmarkCommand {
    #[default]
    Save,
    Paste,
    Import,
}

/// A button on one saved bookmark's row.
#[derive(Component, Clone)]
pub struct BookmarkButton {
    pub path: PathBuf,
    pub action: BookmarkAction,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookmarkAction {
    Open,
    CopyLine,
    Export,
    Delete,
}

/// The column the saved list is rebuilt into.
#[derive(Component, Clone, Default)]
pub struct BookmarkList;

/// Anything in the list, despawned wholesale on a rebuild.
#[derive(Component, Clone, Default)]
pub struct BookmarkListContent;

/// The line saying how the last thing done went.
#[derive(Component, Clone, Default)]
pub struct BookmarkStatus;

pub fn spawn_bookmarks_section(
    mut commands: Commands,
    content: Query<Entity, With<SidebarContent>>,
) {
    let Ok(parent) = content.single() else { return };
    let accordion = spawn_accordion(&mut commands, "Bookmarks", false, SectionLevel::Pane);
    commands
        .entity(accordion.section)
        .insert(SectionOrder(SECTION_ORDER));
    commands.entity(parent).add_child(accordion.section);

    let field = commands
        .spawn_scene(bsn! {
            @FeathersTextInput
            BookmarkNameInput
        })
        .id();
    let entry = commands
        .spawn_scene(bsn! {
            @FeathersTextInputContainer
            BlocksFrameInput
        })
        .id();
    commands.entity(entry).add_child(field);
    let save = command_button(
        &mut commands,
        Icon::BookmarkPlus,
        "Save",
        BookmarkCommand::Save,
    );
    let save_row = row(&mut commands);
    commands.entity(save_row).add_children(&[entry, save]);

    let hint = caption(
        &mut commands,
        "Name what is on screen, or leave it blank to name it after its datasets.",
    );

    // The field would not show against the pane's body on its own.
    let well = commands.spawn_scene(field_well()).id();
    commands.entity(well).add_children(&[save_row, hint]);

    let paste = command_button(
        &mut commands,
        Icon::ClipboardPaste,
        "Paste",
        BookmarkCommand::Paste,
    );
    let import = command_button(
        &mut commands,
        Icon::FolderOpen,
        "Import\u{2026}",
        BookmarkCommand::Import,
    );
    let share_row = row(&mut commands);
    commands.entity(share_row).add_children(&[paste, import]);

    let status = commands
        .spawn_scene(bsn! {
            BookmarkStatus
            Text({ String::new() })
            TextFont { font_size: { FontSize::Px(11.0) } }
            Node { display: { Display::None } }
        })
        .id();

    let list = commands
        .spawn_scene(bsn! {
            BookmarkList
            Node {
                flex_direction: { FlexDirection::Column },
                width: { Val::Percent(100.0) },
                margin: { UiRect::top(Val::Px(6.0)) },
            }
        })
        .id();

    commands
        .entity(accordion.body)
        .add_children(&[well, share_row, status, list]);
}

fn row(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
            }
        })
        .id()
}

fn command_button(
    commands: &mut Commands,
    icon: Icon,
    text: &'static str,
    command: BookmarkCommand,
) -> Entity {
    commands
        .spawn_scene(bsn! {
            @FeathersButton {
                @caption: { bsn_list![button_icon(icon), button_text(text)] }
            }
            Node { column_gap: { Val::Px(6.0) }, flex_shrink: { 0.0_f32 } }
            BlocksFrameInput
            template_value(command)
        })
        .id()
}

/// Rebuild the list whenever what is saved changes.
pub fn rebuild_list(
    mut commands: Commands,
    saved: Res<SavedBookmarks>,
    list: Query<Entity, With<BookmarkList>>,
    existing: Query<Entity, With<BookmarkListContent>>,
    mut built: Local<bool>,
) {
    let Ok(list) = list.single() else { return };
    if *built && !saved.is_changed() {
        return;
    }
    *built = true;
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    if saved.list.is_empty() {
        let empty = caption(&mut commands, "Nothing saved yet.");
        commands.entity(empty).insert(BookmarkListContent);
        commands.entity(list).add_child(empty);
        return;
    }

    for entry in &saved.list {
        let bookmark = &entry.bookmark;
        let frames = bookmark.frames.len();
        let mut detail = format!(
            "{frames} frame{}, {} dataset{}",
            if frames == 1 { "" } else { "s" },
            bookmark.sources.len(),
            if bookmark.sources.len() == 1 { "" } else { "s" },
        );
        if !local_addresses(bookmark).is_empty() {
            detail.push_str(", local files");
        }
        let name = bookmark.name.clone();
        let open = commands
            .spawn_scene(bsn! {
                @FeathersButton {
                    @variant: { ButtonVariant::Plain }
                }
                BlocksFrameInput
                Node {
                    flex_grow: { 1.0_f32 },
                    flex_shrink: { 1.0_f32 },
                    min_width: { Val::Px(0.0) },
                    overflow: { Overflow::clip() },
                    flex_direction: { FlexDirection::Column },
                    align_items: { AlignItems::Start },
                    padding: { UiRect::axes(Val::Px(6.0), Val::Px(4.0)) },
                    height: { Val::Auto },
                }
                Children [
                    (
                        label(name)
                        TextFont { font_size: { FontSize::Px(13.0) } }
                        TextLayout { linebreak: { LineBreak::NoWrap } }
                    ),
                    (
                        label_dim(detail)
                        TextFont { font_size: { FontSize::Px(11.0) } }
                        TextLayout { linebreak: { LineBreak::NoWrap } }
                    ),
                ]
            })
            .id();
        commands.entity(open).insert(BookmarkButton {
            path: entry.path.clone(),
            action: BookmarkAction::Open,
        });
        let buttons: Vec<Entity> = [
            (Icon::Copy, BookmarkAction::CopyLine),
            (Icon::Download, BookmarkAction::Export),
            (Icon::Trash, BookmarkAction::Delete),
        ]
        .into_iter()
        .map(|(icon, action)| {
            let button = commands
                .spawn_scene(bsn! {
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(icon)] }
                    }
                    BlocksFrameInput
                    Node { flex_shrink: { 0.0_f32 } }
                })
                .id();
            commands.entity(button).insert(BookmarkButton {
                path: entry.path.clone(),
                action,
            });
            button
        })
        .collect();

        let line = row(&mut commands);
        commands.entity(line).insert(BookmarkListContent);
        commands.entity(line).add_child(open).add_children(&buttons);
        commands.entity(list).add_child(line);
    }
}

/// What a bookmark saved without a name is called: the datasets on screen,
/// in grid order.
fn default_name(panels: &Query<(&Panel, &ShowsSource)>, sources: &Query<&DataSource>) -> String {
    let mut frames: Vec<(usize, Entity)> = panels
        .iter()
        .map(|(panel, shows)| (panel.index, shows.0))
        .collect();
    frames.sort_unstable();
    let mut names: Vec<&str> = Vec::new();
    for (_, source) in frames {
        if let Ok(data) = sources.get(source)
            && !names.contains(&data.name.as_str())
        {
            names.push(&data.name);
        }
    }
    if names.is_empty() {
        "Bookmark".into()
    } else {
        names.join(" + ")
    }
}

fn save_named(
    commands: &mut Commands,
    field: Entity,
    inputs: &mut Query<&mut EditableText, With<BookmarkNameInput>>,
    panels: &Query<(&Panel, &ShowsSource)>,
    sources: &Query<&DataSource>,
) {
    let Ok(mut text) = inputs.get_mut(field) else {
        return;
    };
    let typed = text.value().to_string().trim().to_string();
    let name = if typed.is_empty() {
        default_name(panels, sources)
    } else {
        typed
    };
    save_current(commands, name);
    text.clear();
}

pub fn on_command(
    activate: On<Activate>,
    mut commands: Commands,
    buttons: Query<&BookmarkCommand>,
    mut inputs: Query<&mut EditableText, With<BookmarkNameInput>>,
    fields: Query<Entity, With<BookmarkNameInput>>,
    panels: Query<(&Panel, &ShowsSource)>,
    sources: Query<&DataSource>,
    mut clipboard: ResMut<Clipboard>,
    mut dialog: ResMut<FileDialog>,
    mut notice: ResMut<BookmarkNotice>,
) {
    let Ok(command) = buttons.get(activate.entity) else {
        return;
    };
    match command {
        BookmarkCommand::Save => {
            if let Ok(field) = fields.single() {
                save_named(&mut commands, field, &mut inputs, &panels, &sources);
            }
        }
        BookmarkCommand::Paste => {
            let pasted = clipboard
                .fetch_text()
                .poll_result()
                .unwrap_or(Err(bevy::clipboard::ClipboardError::ContentNotAvailable))
                .map_err(|e| format!("could not read the clipboard: {e}"))
                .and_then(|text| from_text(&text));
            match pasted {
                Ok(bookmark) => open_shared(&mut commands, bookmark),
                Err(error) => {
                    warn!("{error}");
                    *notice = BookmarkNotice::Failed(error);
                }
            }
        }
        BookmarkCommand::Import => {
            *notice = BookmarkNotice::Working("choosing a file\u{2026}".into());
            dialog.import();
        }
    }
}

/// Save when return is pressed in the name field.
pub fn on_name_submitted(
    key: On<FocusedInput<KeyboardInput>>,
    mut commands: Commands,
    mut inputs: Query<&mut EditableText, With<BookmarkNameInput>>,
    panels: Query<(&Panel, &ShowsSource)>,
    sources: Query<&DataSource>,
) {
    if !inputs.contains(key.focused_entity) || !key.input.state.is_pressed() {
        return;
    }
    if matches!(key.input.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
        save_named(
            &mut commands,
            key.focused_entity,
            &mut inputs,
            &panels,
            &sources,
        );
    }
}

pub fn on_bookmark_button(
    activate: On<Activate>,
    mut commands: Commands,
    buttons: Query<&BookmarkButton>,
    mut saved: ResMut<SavedBookmarks>,
    mut clipboard: ResMut<Clipboard>,
    mut dialog: ResMut<FileDialog>,
    mut notice: ResMut<BookmarkNotice>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(entry) = saved
        .list
        .iter()
        .find(|entry| entry.path == button.path)
        .cloned()
    else {
        return;
    };
    let bookmark = entry.bookmark;
    match button.action {
        BookmarkAction::Open => restore(&mut commands, bookmark),
        BookmarkAction::CopyLine => match clipboard.set_text(to_line(&bookmark)) {
            Ok(()) => {
                info!("copied bookmark {} to the clipboard", bookmark.name);
                *notice = if local_addresses(&bookmark).is_empty() {
                    BookmarkNotice::Done(format!("copied {}", bookmark.name))
                } else {
                    BookmarkNotice::Done(format!(
                        "copied {}; it names files on this machine",
                        bookmark.name
                    ))
                };
            }
            Err(e) => *notice = BookmarkNotice::Failed(format!("could not copy: {e}")),
        },
        BookmarkAction::Export => {
            *notice = BookmarkNotice::Working("choosing where to save\u{2026}".into());
            dialog.export(bookmark);
        }
        BookmarkAction::Delete => {
            *notice = match saved.delete(&entry.path) {
                Ok(()) => {
                    info!(
                        "deleted bookmark {} ({})",
                        bookmark.name,
                        entry.path.display()
                    );
                    BookmarkNotice::Done(format!("deleted {}", bookmark.name))
                }
                Err(error) => BookmarkNotice::Failed(error),
            };
        }
    }
}

/// Show how the last thing done went, colored by whether it went well.
pub fn sync_status(
    notice: Res<BookmarkNotice>,
    palette: Res<Palette>,
    mut labels: Query<(&mut Text, &mut TextColor, &mut Node), With<BookmarkStatus>>,
) {
    if !notice.is_changed() && !palette.is_changed() {
        return;
    }
    let (message, color) = match &*notice {
        BookmarkNotice::Idle => (String::new(), palette.progress),
        BookmarkNotice::Working(message) | BookmarkNotice::Done(message) => {
            (message.clone(), palette.progress)
        }
        BookmarkNotice::Failed(message) => (message.clone(), palette.problem),
    };
    for (mut text, mut text_color, mut node) in &mut labels {
        let display = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        if text.0 != message {
            text.0.clone_from(&message);
        }
        text_color.set_if_neq(TextColor(color));
    }
}

pub struct BookmarksPlugin;

impl Plugin for BookmarksPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_command)
            .add_observer(on_name_submitted)
            .add_observer(on_bookmark_button)
            .add_systems(Startup, spawn_bookmarks_section.in_set(Boot::DockContent))
            .add_systems(Update, rebuild_list.in_set(Stage::ControlsBuild))
            .add_systems(Update, sync_status.in_set(Stage::ControlsPlace));
    }
}
