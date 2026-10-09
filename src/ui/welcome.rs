//! What an empty window shows.
//!
//! Nothing is loaded at startup any more, so the frame area would otherwise be
//! a cleared rectangle with no way into the app. This fills it: what the viewer
//! is for, a button opening the same empty frame the sidebar's New frame does
//! to search for anything, the front page of each data source that has one
//! ([`crate::ui::dashboards`]), an example of each kind of dataset it reads,
//! and who made it.
//!
//! The saved bookmarks are not here: the sidebar lists them, and starts open.
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
use bevy_ui_widgets::Activate;

use crate::app::schedule::{Boot, Stage};
use crate::catalog::Catalogs;
use crate::catalog::examples::{EXAMPLES, Example};
use crate::ui::add_source::status_line;
use crate::ui::help::{AUTHOR, LICENSE, LICENSE_URL, REPOSITORY};
use crate::view::browse::new_frame_button;
use crate::view::{FrameArea, Panel};
use crate::widgets::{
    BlocksFrameInput, Icon, button_text, display, link_button, patch_node, size, text, text_dim,
    title,
};
use crate::widgets::{ScrollBoth, space};

/// The empty-state panel itself.
#[derive(Component, Clone, Default)]
pub struct WelcomeScreen;

/// A button that opens an example, by its address.
#[derive(Component, Clone, Default)]
pub struct ExampleButton {
    pub url: &'static str,
}

/// Width the prose and the controls are held to, so neither runs the width of a
/// wide window.
const COLUMN_PX: f32 = 520.0;

/// Width of the list of examples.
const EXAMPLES_PX: f32 = 440.0;

/// Above the frames, which is where it is drawn, but below the menus that open
/// over everything.
const WELCOME_Z: i32 = 5;

pub const BLURB: &str = "An experimental streaming explorer for large scientific datasets. \
                     Currently supports OME-Zarr v2 and v3, Deep Zoom images, the \
                     Allen Institute Scatterbrain format for point clouds, SVG \
                     annotations, CSV, TSV and Parquet tables, and specimen records from the \
                     Brain Knowledge Platform.";

pub fn spawn_welcome(mut commands: Commands, catalogs: Res<Catalogs>) {
    let screen = commands
        .spawn_scene(bsn! {
            WelcomeScreen
            // It covers the grid, so it has to stop clicks reaching whatever is
            // behind it. Frames can be opened while it is on screen.
            BlocksFrameInput
            // A short or narrow frame area, under an open log panel or beside
            // wide docks, holds less than the screen needs: it scrolls both
            // ways rather than spilling over the docks or squeezing the cards.
            ScrollBoth
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::FlexStart },
                overflow: { Overflow::scroll() },
            }
            GlobalZIndex({ WELCOME_Z })
            InheritableFont { font_size: { 13.0f32 } }
        })
        .id();

    // The padding is the page's rather than the screen's, so it scrolls with
    // the page: an area scrolling both ways clips at its content box, and
    // padding of its own would leave a band round the edge that cuts the page
    // off short of the window. At least as large as the screen, so the footer
    // still finds the foot of it.
    let page = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::FlexStart },
                flex_shrink: { 0.0_f32 },
                min_width: { Val::Percent(100.0) },
                min_height: { Val::Percent(100.0) },
                row_gap: { Val::Px(space::SCREEN_GAP) },
                padding: { UiRect::all(Val::Px(space::SCREEN_GAP)) },
            }
        })
        .id();
    commands.entity(screen).add_child(page);

    let title = commands
        .spawn_scene(bsn! {
            @title("Bevy Data Explorer")
        })
        .id();

    let blurb = commands
        .spawn_scene(bsn! {
            @text_dim(BLURB, size::BODY)
            Node { max_width: { Val::Px(COLUMN_PX) } }
        })
        .id();

    // Each data source's front page — what it holds, and where to start in
    // it — one at a time.
    let dashboards = crate::ui::dashboards::spawn_dashboards(&mut commands, &catalogs);

    // One of each kind the viewer reads, which says what can be pasted into
    // it as much as it offers something to look at.
    let examples = example_column(
        &mut commands,
        "Examples",
        "One of every kind of data the explorer supports. Paste an address like any of these to open your own.",
        &EXAMPLES,
    );

    // Any dataset, and any address, is found in an empty frame: the same one
    // the sidebar's New frame button opens. First, since it reaches everything
    // the rest of the screen only samples.
    let browse = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                align_items: { AlignItems::FlexStart },
                width: { Val::Px(COLUMN_PX) },
                row_gap: { Val::Px(space::ROWS) },
            }
            Children [
                @new_frame_button("Browse all datasets", ButtonVariant::Primary)
                --
                @label_dim("Search every catalog, or paste the URL of a dataset.")
                --
                @status_line()
            ]
        })
        .id();

    let mut children = vec![title, blurb, browse, dashboards, examples];

    // Pushed to the foot of the screen by its auto margin.
    let footer = commands
        .spawn_scene(bsn! {
            Node {
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(space::CONTROLS) },
                margin: { UiRect::top(Val::Auto) },
                padding: { UiRect::top(Val::Px(space::SCREEN_GAP)) },
            }
            Children [
                @label_dim(format!("Made by {AUTHOR} \u{00b7} Licensed under"))
                --
                @link_button(Icon::ExternalLink, LICENSE, LICENSE_URL, ButtonVariant::Plain)
                --
                @label_dim("\u{00b7}")
                --
                @link_button(Icon::ExternalLink, "GitHub", REPOSITORY, ButtonVariant::Plain)
            ]
            InheritableFont { font_size: { 12.0f32 } }
        })
        .id();
    children.push(footer);

    commands.entity(page).add_children(&children);
}

/// A heading and a line on what they are over a button for each of
/// `examples`, with its kind beside it.
///
/// A grid rather than a row per example, so the kinds share one column sized
/// to the longest of them, and every button in the list ends in the same
/// place rather than wherever its own kind happens to start.
fn example_column(
    commands: &mut Commands,
    heading: &str,
    about: &str,
    examples: &[Example],
) -> Entity {
    let heading = heading.to_string();
    let about = about.to_string();
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
                @text(heading, size::BODY)
                Node { grid_column: { GridPlacement::span(2) } }
                --
                @text_dim(about, size::SECONDARY)
                Node {
                    grid_column: { GridPlacement::span(2) },
                    margin: { UiRect::bottom(Val::Px(space::HEADING)) },
                }
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
                        @caption: { bsn_list! {@button_text(name)
        // A name too long for the button is clipped rather than
        // wrapped onto a second line over its edge.
        TextLayout { linebreak: { LineBreak::NoWrap } }} }
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
            @text_dim(kind, size::SECONDARY)
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
    for node in &mut screens {
        patch_node(node, |node| {
            node.display = display(empty);
            if empty {
                node.left = Val::Px(area.origin.x);
                node.top = Val::Px(area.origin.y);
                node.width = Val::Px(area.size.x);
                node.height = Val::Px(area.size.y);
            }
        });
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
            .add_systems(Startup, spawn_welcome.in_set(Boot::Shell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_the_viewer_draws_is_offered() {
        // An empty window should leave nothing the viewer can draw without a
        // way to open one. Scatterbrain is both a single cloud and a grid of
        // sections, and each opens a different frame.
        let kinds: Vec<&str> = EXAMPLES.iter().map(|example| example.kind).collect();
        for kind in [
            "OME-Zarr",
            "Neuroglancer",
            "Deep Zoom",
            "SVG",
            "UMAP",
            "sections",
            "CSV",
            "Parquet",
            "specimen table",
        ] {
            assert!(kinds.iter().any(|k| k.contains(kind)), "no {kind} example");
        }
    }
}
