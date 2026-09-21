//! What an empty window shows.
//!
//! Nothing is loaded at startup any more, so the frame area would otherwise be
//! a cleared rectangle with no way into the app. This fills it: what the viewer
//! is for, the bookmarks saved so far, every dataset it knows the address of —
//! the Brain Knowledge Platform's beside the rest — a button opening the same
//! empty frame the sidebar's New frame does, for anything else, and who made
//! it.
//!
//! It is UI rather than frame chrome, but it is placed against
//! [`FrameArea`] like the chrome is, so the docks take their space off it
//! without this knowing they exist. It shows itself exactly when there are no
//! frames, which is a state the grid already allows rather than one invented
//! here.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::label_dim;
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::app::schedule::{Boot, Stage};
use crate::bookmark::store::SavedBookmarks;
use crate::catalog::bkp;
use crate::catalog::examples::{EXAMPLES, Example};
use crate::ui::add_source::status_line;
use crate::ui::bookmarks::BookmarkList;
use crate::ui::help::{AUTHOR, LICENSE, LICENSE_URL, REPOSITORY};
use crate::view::browse::new_frame_button;
use crate::view::{FrameArea, Panel};
use crate::widgets::space;
use crate::widgets::{
    BlocksFrameInput, Icon, button_text, link_button, size, text, text_dim, title,
};

/// The empty-state panel itself.
#[derive(Component, Clone, Default)]
pub struct WelcomeScreen;

/// The saved bookmarks, shown only when there are some.
#[derive(Component, Clone, Default)]
pub struct WelcomeBookmarks;

/// A button that opens an example, by its address.
#[derive(Component, Clone, Default)]
pub struct ExampleButton {
    pub url: &'static str,
}

/// Width the prose and the controls are held to, so neither runs the width of a
/// wide window.
const COLUMN_PX: f32 = 520.0;

/// Width of each column of examples. Two sit side by side in a wide frame
/// area and wrap one under the other in a narrow one.
const EXAMPLES_PX: f32 = 440.0;

/// Above the frames, which is where it is drawn, but below the menus that open
/// over everything.
const WELCOME_Z: i32 = 5;

pub const BLURB: &str = "An experimental streaming explorer for large scientific datasets. \
                     Currently supports OME-Zarr v2 and v3, Deep Zoom images, the \
                     Allen Institute Scatterbrain format for point clouds, SVG \
                     annotations, CSV and TSV tables, and specimen records from the \
                     Brain Knowledge Platform.";

pub fn spawn_welcome(mut commands: Commands) {
    let screen = commands
        .spawn_scene(bsn! {
            WelcomeScreen
            // It covers the grid, so it has to stop clicks reaching whatever is
            // behind it. Frames can be opened while it is on screen.
            BlocksFrameInput
            // A short frame area, under an open log panel, holds less than the
            // screen needs: it scrolls rather than spilling over the docks.
            ScrollArea
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::Center },
                row_gap: { Val::Px(space::SCREEN_GAP) },
                padding: { UiRect::all(Val::Px(space::SCREEN_GAP)) },
                overflow: { Overflow::scroll_y() },
            }
            GlobalZIndex({ WELCOME_Z })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    let title = commands
        .spawn_scene(bsn! {
            title("Bevy Data Explorer")
            // Centred by auto margins at either end rather than by
            // `JustifyContent::Center`, which overflows both ways once the
            // content is taller than the screen and puts the top out of reach
            // of the scroll. Auto margins shrink to nothing instead.
            Node { margin: { UiRect::top(Val::Auto) } }
        })
        .id();

    let blurb = commands
        .spawn_scene(bsn! {
            text_dim(BLURB, size::BODY)
            Node { max_width: { Val::Px(COLUMN_PX) } }
            TextLayout { justify: { Justify::Center } }
        })
        .id();

    // Above the examples: someone with bookmarks has been here before, and is
    // more likely back for one of them than for an example.
    let bookmarks = commands
        .spawn_scene(bsn! {
            WelcomeBookmarks
            Node {
                display: { Display::None },
                flex_direction: { FlexDirection::Column },
                width: { Val::Px(COLUMN_PX) },
                max_width: { Val::Percent(100.0) },
            }
            Children [
                (
                    text("Bookmarks", size::BODY)
                    Node { margin: { UiRect::bottom(Val::Px(space::HEADING)) } }
                ),
                (
                    BookmarkList { compact: true }
                    Node {
                        flex_direction: { FlexDirection::Column },
                        width: { Val::Percent(100.0) },
                    }
                ),
            ]
        })
        .id();

    // The platform's own visualizations lead, since they are what most people
    // come to look at; the formats the viewer reads follow beside them.
    let bkp = example_column(&mut commands, "Brain Knowledge Platform", &bkp::EXAMPLES);
    let others = example_column(&mut commands, "Other examples", &EXAMPLES);
    let columns = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Percent(100.0) },
                flex_wrap: { FlexWrap::Wrap },
                justify_content: { JustifyContent::Center },
                column_gap: { Val::Px(space::SCREEN_WIDE) },
                row_gap: { Val::Px(space::SCREEN_GAP) },
            }
        })
        .id();
    commands.entity(columns).add_children(&[bkp, others]);

    let mut children = vec![title, blurb, bookmarks, columns];

    // Everything else, and any address, is found in an empty frame: the same
    // one the sidebar's New frame button opens.
    let browse = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::Center },
                width: { Val::Px(COLUMN_PX) },
                max_width: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                new_frame_button("Browse all datasets", ButtonVariant::Primary),
                label_dim("Search every catalog, or paste the URL of a dataset."),
                status_line(),
            ]
        })
        .id();
    children.push(browse);

    // Pushed to the foot of the screen by its auto margin, which with the
    // title's leaves the rest centred in what is between them.
    let footer = commands
        .spawn_scene(bsn! {
            Node {
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
                margin: { UiRect::top(Val::Auto) },
                padding: { UiRect::top(Val::Px(space::SCREEN_GAP)) },
            }
            Children [
                label_dim(format!("Made by {AUTHOR} \u{00b7} Licensed under")),
                link_button(Icon::ExternalLink, LICENSE, LICENSE_URL, ButtonVariant::Plain),
                label_dim("\u{00b7}"),
                link_button(Icon::ExternalLink, "GitHub", REPOSITORY, ButtonVariant::Plain),
            ]
            InheritableFont { font_size: { 12.0f32 } }
        })
        .id();
    children.push(footer);

    commands.entity(screen).add_children(&children);
}

/// A heading over a button for each of `examples`, with its kind beside it.
///
/// A grid rather than a row per example, so the kinds share one column sized
/// to the longest of them, and every button in the list ends in the same
/// place rather than wherever its own kind happens to start.
fn example_column(commands: &mut Commands, heading: &str, examples: &[Example]) -> Entity {
    let heading = heading.to_string();
    let column = commands
        .spawn_scene(bsn! {
            Node {
                width: { Val::Px(EXAMPLES_PX) },
                display: { Display::Grid },
                // The buttons take what the kinds leave, and may shrink to
                // nothing rather than widening the column past its width.
                grid_template_columns: { vec![
                    GridTrack::minmax(MinTrackSizingFunction::Px(0.0), MaxTrackSizingFunction::Fraction(1.0)),
                    GridTrack::auto(),
                ] },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::GROUPS) },
                row_gap: { Val::Px(space::LIST_ITEMS) },
            }
            Children [
                (
                    text(heading, size::BODY)
                    Node {
                        grid_column: { GridPlacement::span(2) },
                        margin: { UiRect::bottom(Val::Px(space::HEADING)) },
                    }
                ),
            ]
        })
        .id();
    for example in examples {
        let cells = example_cells(commands, example);
        commands.entity(column).add_children(&cells);
    }
    column
}

/// One example: a button that opens it, and the kind of dataset it is.
fn example_cells(commands: &mut Commands, example: &Example) -> [Entity; 2] {
    let name = example.name.to_string();
    let kind = example.kind.to_string();
    let url = example.url;
    let button = commands
        .spawn_scene(bsn! {
            @FeathersButton {
                @variant: { ButtonVariant::Normal },
                @caption: { bsn_list![(
                    button_text(name)
                    // A name too long for the button is clipped rather than
                    // wrapped onto a second line over its edge.
                    TextLayout { linebreak: { LineBreak::NoWrap } }
                )] }
            }
            BlocksFrameInput
            ExampleButton { url: { url } }
            Node {
                min_width: { Val::Px(0.0) },
                overflow: { Overflow::clip() },
            }
        })
        .id();
    let kind = commands
        .spawn_scene(bsn! {
            text_dim(kind, size::SECONDARY)
        })
        .id();
    [button, kind]
}

/// Open the example whose button was pressed.
///
/// Asked for by address, the way a layer chosen from a frame's menu is: one
/// open already gets a frame onto the source it opened as, and anything else
/// is read by exactly the path a typed URL is.
pub fn on_example_pressed(
    activate: On<Activate>,
    buttons: Query<&ExampleButton>,
    mut requests: MessageWriter<crate::view::DatasetRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    requests.write(crate::view::DatasetRequest {
        url: button.url.to_string(),
        target: crate::view::DatasetTarget::NewFrame,
    });
}

/// Cover the frame area while there are no frames, and stand down once there
/// are.
pub fn place_welcome(
    area: Res<FrameArea>,
    panels: Query<&Panel>,
    mut screens: Query<&mut Node, With<WelcomeScreen>>,
) {
    let empty = panels.iter().next().is_none();
    for mut node in &mut screens {
        let wanted = if empty { Display::Flex } else { Display::None };
        if node.display != wanted {
            node.display = wanted;
        }
        if !empty {
            continue;
        }
        node.left = Val::Px(area.origin.x);
        node.top = Val::Px(area.origin.y);
        node.width = Val::Px(area.size.x);
        node.height = Val::Px(area.size.y);
    }
}

/// Show the bookmarks only when something has been saved.
pub fn show_welcome_bookmarks(
    saved: Res<SavedBookmarks>,
    mut columns: Query<&mut Node, With<WelcomeBookmarks>>,
) {
    let wanted = if saved.list.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut columns {
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// The empty window, and the examples it offers.
pub struct WelcomePlugin;

impl Plugin for WelcomePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_example_pressed)
            // Placed with the rest of the chrome that measures against the
            // frame area, once the docks have taken their share of it.
            .add_systems(Update, place_welcome.in_set(Stage::Chrome))
            .add_systems(Update, show_welcome_bookmarks.in_set(Stage::ControlsPlace))
            .add_systems(Startup, spawn_welcome.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_the_viewer_draws_is_offered() {
        // An empty window should leave nothing the viewer can draw without a
        // way to open one. Scatterbrain comes from the platform: a UMAP is a
        // single cloud, a grid of sections a sectioned one.
        let kinds: Vec<&str> = EXAMPLES
            .iter()
            .chain(&bkp::EXAMPLES)
            .map(|example| example.kind)
            .collect();
        assert!(kinds.iter().any(|kind| kind.contains("image")));
        assert!(kinds.contains(&"UMAP"));
        assert!(kinds.iter().any(|kind| kind.contains("sections")));
        assert!(kinds.iter().any(|kind| kind.contains("stack")));
        assert!(kinds.iter().any(|kind| kind.contains("Deep Zoom")));
        assert!(kinds.iter().any(|kind| kind.contains("table")));
    }

    #[test]
    fn every_dataset_the_app_knows_is_offered_here() {
        // Not one of each kind. Two images can differ in the version of the
        // store they are written in or in whether they are a stack, and picking
        // one to stand for the other hides what makes them worth opening.
        let all = || EXAMPLES.iter().chain(&bkp::EXAMPLES);
        let mut urls: Vec<&str> = all().map(|example| example.url).collect();
        urls.sort_unstable();
        urls.dedup();
        assert_eq!(urls.len(), all().count(), "two examples share a URL");
    }

    #[test]
    fn every_example_names_an_address_that_can_be_fetched() {
        // Written the way they were copied — one of them straight out of a
        // neuroglancer config — so what matters is that each one comes out of
        // the translation as something fetchable.
        for example in EXAMPLES.iter().chain(&bkp::EXAMPLES) {
            let url = crate::formats::plain_url(example.url);
            assert!(url.starts_with("https://"), "{}: {url}", example.name);
            assert!(!example.name.is_empty());
            assert!(!example.kind.is_empty());
        }
    }
}
