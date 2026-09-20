//! A scrollbar for everything that scrolls, added without being asked for.
//!
//! Feathers has a scrollbar, but it is a headless one wearing a theme: it has
//! to be told which entity it scrolls, and it does not place itself. Wiring
//! one up by hand at every scrolling area is how scrolling areas end up
//! without them, which is where this app was. So nothing here is opt-in —
//! every [`ScrollArea`] and [`ScrollList`] gets a bar the frame after it is
//! spawned, and the sections, menus and docks that scroll say nothing about
//! it.
//!
//! The bar is an absolutely positioned child of the area it scrolls, held
//! still against the scroll by [`IgnoreScroll`] and drawn above its siblings
//! by a local [`ZIndex`] rather than by being the last child — the docks
//! rebuild their rows, and a bar that relied on child order would end up
//! underneath them.
//!
//! It is given a lane of its own rather than laid over the content. The area
//! reserves one through `Node::scrollbar_width`, which is what taffy carves a
//! scrollbar gutter out of, so every row is laid out in what is left and none
//! can reach under the bar. The gutter is kept whether or not the bar is
//! showing, so that a section growing long enough to scroll does not shunt
//! every row beside it sideways.

use bevy::ecs::template::EntityTemplate;
use bevy::prelude::*;
use bevy::ui::IgnoreScroll;
use bevy_feathers::controls::FeathersScrollbar;
use bevy_ui_widgets::{ControlOrientation, ScrollArea};

use super::{BlocksFrameInput, scroll::ScrollList};

/// How thick a bar is, across the axis it scrolls.
const BAR_PX: f32 = 8.0;

/// Clear space kept between the content and the bar, so that a row ending in
/// a number does not end against it.
const GAP_PX: f32 = 4.0;

/// What a scrolling area gives up to the bar it holds: the bar and the gap
/// beside it.
const GUTTER_PX: f32 = BAR_PX + GAP_PX;

/// How much taller than its area a content has to be before a bar is worth
/// showing.
///
/// Not zero, because the bar is itself an absolutely positioned child and
/// taffy counts one in the content size it reports: an area that exactly fits
/// measures its content as its own height, and without the slack every area
/// would claim to overflow by a hair.
const SLACK_PX: f32 = 1.0;

/// Marks a scrolling area a bar has already been added to, so it is added
/// once rather than every frame.
#[derive(Component, Clone, Default)]
pub struct HasScrollbar;

/// The area a bar scrolls, and along which axis, on the bar.
#[derive(Component, Clone)]
pub struct ScrollbarFor {
    pub area: Entity,
    pub orientation: ControlOrientation,
}

impl Default for ScrollbarFor {
    fn default() -> Self {
        ScrollbarFor {
            area: Entity::PLACEHOLDER,
            orientation: ControlOrientation::Vertical,
        }
    }
}

/// Give every scrolling area that has no bar yet a bar.
pub fn add_scrollbars(
    mut commands: Commands,
    mut areas: Query<
        (Entity, &mut Node),
        (
            Or<(With<ScrollArea>, With<ScrollList>)>,
            Without<HasScrollbar>,
        ),
    >,
) {
    for (area, mut node) in &mut areas {
        let mut bars = Vec::new();
        if node.overflow.y == OverflowAxis::Scroll {
            bars.push(spawn_bar(&mut commands, area, ControlOrientation::Vertical));
        }
        if node.overflow.x == OverflowAxis::Scroll {
            bars.push(spawn_bar(
                &mut commands,
                area,
                ControlOrientation::Horizontal,
            ));
        }
        if bars.is_empty() {
            continue;
        }
        // Reserved on whichever axes scroll, which taffy works out for
        // itself: a node that scrolls up and down needs the space across.
        node.scrollbar_width = GUTTER_PX;
        commands.entity(area).insert(HasScrollbar);
        commands.entity(area).add_children(&bars);
    }
}

fn spawn_bar(commands: &mut Commands, area: Entity, orientation: ControlOrientation) -> Entity {
    // Laid along the edge it scrolls and thin across it, so one scene serves
    // both orientations, anchored to the right and bottom either way.
    //
    // The inset it is pushed out by is negative because taffy resolves the
    // insets of an absolute child against the box *inside* the gutter — the
    // same reservation that keeps the content clear would otherwise leave the
    // bar sitting on the content's edge and the gutter empty.
    let (right, bottom, width, height) = match orientation {
        ControlOrientation::Vertical => (
            Val::Px(-GUTTER_PX),
            Val::Px(0.0),
            Val::Px(BAR_PX),
            Val::Percent(100.0),
        ),
        ControlOrientation::Horizontal => (
            Val::Px(0.0),
            Val::Px(-GUTTER_PX),
            Val::Percent(100.0),
            Val::Px(BAR_PX),
        ),
    };
    commands
        .spawn_scene(bsn! {
            @FeathersScrollbar {
                @target: { EntityTemplate::from(area) },
                @orientation: { orientation }
            }
            ScrollbarFor { area: { area }, orientation: { orientation } }
            Node {
                position_type: { PositionType::Absolute },
                // Hidden until it has been measured, so a bar never flashes
                // over content that turns out to fit.
                display: { Display::None },
                right: { right },
                bottom: { bottom },
                width: { width },
                height: { height },
            }
            IgnoreScroll({ BVec2::TRUE })
            ZIndex({ 1 })
            BlocksFrameInput
        })
        .id()
}

/// Show each bar only while its area has something to scroll.
pub fn show_scrollbars(areas: Query<&ComputedNode>, mut bars: Query<(&ScrollbarFor, &mut Node)>) {
    for (bar, mut node) in &mut bars {
        let Ok(area) = areas.get(bar.area) else {
            continue;
        };
        let scale = area.inverse_scale_factor();
        let visible = (area.size() - area.scrollbar_size) * scale;
        let content = area.content_size() * scale;
        let (content, visible) = match bar.orientation {
            ControlOrientation::Vertical => (content.y, visible.y),
            ControlOrientation::Horizontal => (content.x, visible.x),
        };
        node.display = if overflows(content, visible) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Whether an area showing `visible` pixels of `content` has anything to
/// scroll to.
fn overflows(content: f32, visible: f32) -> bool {
    content > visible + SLACK_PX
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ui_widgets::Scrollbar;

    /// An app with just enough of Bevy to resolve a scene.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .add_systems(Update, add_scrollbars);
        app
    }

    /// The bar `area` was given, if it was given one.
    fn bar_of(app: &mut App, area: Entity) -> Option<(Entity, ControlOrientation)> {
        let children = app.world().entity(area).get::<Children>()?;
        let bar = children.iter().next()?;
        let scrollbar = app.world().entity(bar).get::<ScrollbarFor>()?;
        Some((bar, scrollbar.orientation))
    }

    fn scrolling(app: &mut App, overflow: Overflow) -> Entity {
        let area = app
            .world_mut()
            .spawn((
                ScrollArea,
                Node {
                    overflow,
                    ..default()
                },
            ))
            .id();
        app.update();
        area
    }

    #[test]
    fn a_scrolling_area_is_given_a_bar_pointed_back_at_it() {
        let mut app = app();
        let area = scrolling(&mut app, Overflow::scroll_y());
        let (bar, orientation) = bar_of(&mut app, area).expect("a bar");
        assert_eq!(orientation, ControlOrientation::Vertical);
        assert_eq!(
            app.world().entity(bar).get::<Scrollbar>().unwrap().target,
            area
        );
    }

    #[test]
    fn a_scrolling_area_gives_up_a_lane_to_the_bar_it_holds() {
        let mut app = app();
        let area = scrolling(&mut app, Overflow::scroll_y());
        let node = app.world().entity(area).get::<Node>().unwrap();
        assert_eq!(node.scrollbar_width, GUTTER_PX);
    }

    #[test]
    fn the_bar_is_pushed_back_out_into_the_lane_it_reserved() {
        // Taffy resolves an absolute child's insets against the box inside
        // the gutter, so a bar left at zero would stand on the content's edge
        // with the lane it reserved empty beside it.
        let mut app = app();
        let area = scrolling(&mut app, Overflow::scroll_y());
        let (bar, _) = bar_of(&mut app, area).expect("a bar");
        let node = app.world().entity(bar).get::<Node>().unwrap();
        assert_eq!(node.right, Val::Px(-GUTTER_PX));
        assert_eq!(node.width, Val::Px(BAR_PX));
    }

    #[test]
    fn an_area_that_scrolls_sideways_is_given_a_bar_lying_that_way() {
        let mut app = app();
        let area = scrolling(&mut app, Overflow::scroll_x());
        let (_, orientation) = bar_of(&mut app, area).expect("a bar");
        assert_eq!(orientation, ControlOrientation::Horizontal);
    }

    #[test]
    fn an_area_that_clips_without_scrolling_is_given_none() {
        let mut app = app();
        let area = scrolling(&mut app, Overflow::clip());
        assert!(bar_of(&mut app, area).is_none());
        // And gives up none of its width for one.
        let node = app.world().entity(area).get::<Node>().unwrap();
        assert_eq!(node.scrollbar_width, 0.0);
    }

    #[test]
    fn an_area_is_given_one_bar_however_long_it_lives() {
        // The marker is what stops a second going on every frame after.
        let mut app = app();
        let area = scrolling(&mut app, Overflow::scroll_y());
        app.update();
        app.update();
        assert_eq!(app.world().entity(area).get::<Children>().unwrap().len(), 1);
    }

    #[test]
    fn an_area_its_content_fits_in_shows_no_bar() {
        assert!(!overflows(200.0, 400.0));
    }

    #[test]
    fn an_area_measuring_its_own_bar_shows_no_bar() {
        // What taffy reports for an area that exactly fits, once the bar is a
        // child of it.
        assert!(!overflows(400.0, 400.0));
    }

    #[test]
    fn an_area_its_content_runs_past_shows_a_bar() {
        assert!(overflows(401.5, 400.0));
        assert!(overflows(4000.0, 400.0));
    }

    #[test]
    fn a_collapsed_area_shows_no_bar() {
        // A hidden menu or dock measures zero both ways.
        assert!(!overflows(0.0, 0.0));
    }
}
