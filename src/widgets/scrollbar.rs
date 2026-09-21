//! A scrollbar for everything that scrolls, added without being asked for.
//!
//! Feathers has a scrollbar, but it is a headless one wearing a theme: it has
//! to be told which entity it scrolls, and it does not place itself. Wiring
//! one up by hand at every scrolling area is how scrolling areas end up
//! without them, which is where this app was. So nothing here is opt-in —
//! every [`ScrollArea`], [`ScrollList`] and [`ScrollBoth`] gets a bar the
//! frame after it is spawned, and the sections, menus, docks and tables that
//! scroll say nothing about it.
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
//!
//! That lane is taken out of padding rather than out of the rows, so a list
//! that scrolls ends where everything beside it ends and its bar sits in the
//! margin. An area with padding of its own gives up its right padding to the
//! bar. One without reaches out into the padding of whatever holds it, by as
//! much as that padding allows: the sidebar's sections, a picker's list and a
//! property's values all end flush with the controls above them.

use bevy::ecs::template::EntityTemplate;
use bevy::prelude::*;
use bevy::ui::IgnoreScroll;
use bevy_feathers::controls::FeathersScrollbar;
use bevy_ui_widgets::{ControlOrientation, ScrollArea};

use super::spacing::step;
use super::{BlocksFrameInput, scroll::ScrollBoth, scroll::ScrollList};

/// How thick a bar is, across the axis it scrolls.
const BAR_PX: f32 = 6.0;

/// Clear space kept between the content and the bar, so that a row ending in
/// a number does not end against it.
const GAP_PX: f32 = step::XXS;

/// Clear space kept between the bar and the edge of what holds it, so the bar
/// reads as sitting in the margin rather than as a border drawn along it.
const EDGE_GAP_PX: f32 = step::XS;

/// What a scrolling area gives up to the bar it holds: the bar and the space
/// either side of it. As wide as a panel's inset, so a bar fits in the
/// padding of the dock or menu it scrolls.
const GUTTER_PX: f32 = GAP_PX + BAR_PX + EDGE_GAP_PX;

/// How far up from an area to look for padding to put its bar in. Far enough
/// to pass the unpadded columns a picker is built from, and no further, since
/// padding beyond that belongs to something unrelated.
const PADDING_SEARCH_DEPTH: usize = 4;

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

/// Where a vertical bar's lane comes from.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Lane {
    /// The area's own right padding, cut down by this much.
    OwnPadding(f32),
    /// The padding around it, reached into by this much.
    Around(f32),
    /// Neither: the lane comes out of the rows.
    Rows,
}

/// Where the lane for a bar in an area laid out like `node` comes from, given
/// the right padding of each of its ancestors, nearest first, and whether its
/// parent stretches it across.
fn lane_for(node: &Node, stretched: bool, ancestors: &[AncestorEdge]) -> Lane {
    if let Val::Px(own) = node.padding.right
        && own > 0.0
    {
        return Lane::OwnPadding(own.min(GUTTER_PX));
    }
    // Only an area its parent stretches can grow past its siblings, and only
    // one whose width is left to that stretch.
    if !stretched || !matches!(node.width, Val::Auto | Val::Percent(100.0)) {
        return Lane::Rows;
    }
    for edge in ancestors.iter().take(PADDING_SEARCH_DEPTH) {
        match edge {
            AncestorEdge::Padded(padding) => return Lane::Around(padding.min(GUTTER_PX)),
            // What clips would hide a bar that reached past it.
            AncestorEdge::Clips => return Lane::Rows,
            AncestorEdge::Open => {}
        }
    }
    Lane::Rows
}

/// What an ancestor offers an area reaching out past its right edge.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AncestorEdge {
    Padded(f32),
    Clips,
    Open,
}

fn edge_of(node: &Node) -> AncestorEdge {
    match node.padding.right {
        Val::Px(padding) if padding > 0.0 => AncestorEdge::Padded(padding),
        _ if node.overflow.x != OverflowAxis::Visible => AncestorEdge::Clips,
        _ => AncestorEdge::Open,
    }
}

/// Whether a child of `parent` is stretched across it by default.
fn stretches_children(parent: &Node) -> bool {
    matches!(
        parent.flex_direction,
        FlexDirection::Column | FlexDirection::ColumnReverse
    ) && matches!(
        parent.align_items,
        AlignItems::Default | AlignItems::Stretch
    )
}

/// Give every scrolling area that has no bar yet a bar.
pub fn add_scrollbars(
    mut commands: Commands,
    new: Query<
        Entity,
        (
            Or<(With<ScrollArea>, With<ScrollList>, With<ScrollBoth>)>,
            Without<HasScrollbar>,
        ),
    >,
    parents: Query<&ChildOf>,
    mut nodes: Query<&mut Node>,
) {
    for area in &new {
        let Ok(node) = nodes.get(area) else {
            continue;
        };
        let lane = if node.overflow.y == OverflowAxis::Scroll {
            let stretched = parents
                .get(area)
                .ok()
                .and_then(|parent| nodes.get(parent.parent()).ok())
                .is_some_and(|parent| {
                    stretches_children(parent)
                        && matches!(node.align_self, AlignSelf::Auto | AlignSelf::Stretch)
                });
            let edges: Vec<AncestorEdge> = parents
                .iter_ancestors(area)
                .take(PADDING_SEARCH_DEPTH)
                .filter_map(|ancestor| nodes.get(ancestor).ok().map(edge_of))
                .collect();
            lane_for(node, stretched, &edges)
        } else {
            Lane::Rows
        };
        let Ok(mut node) = nodes.get_mut(area) else {
            continue;
        };
        match lane {
            Lane::OwnPadding(given) => {
                if let Val::Px(own) = node.padding.right {
                    node.padding.right = Val::Px(own - given);
                }
            }
            Lane::Around(reach) => {
                node.width = Val::Auto;
                node.margin.right = Val::Px(-reach);
            }
            Lane::Rows => {}
        }
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
    // bar sitting on the content's edge and the gutter empty. It stops short
    // of the far side of the gutter by the gap kept from the edge.
    let out = -(GUTTER_PX - EDGE_GAP_PX);
    let (right, bottom, width, height) = match orientation {
        ControlOrientation::Vertical => (
            Val::Px(out),
            Val::Px(0.0),
            Val::Px(BAR_PX),
            Val::Percent(100.0),
        ),
        ControlOrientation::Horizontal => (
            Val::Px(0.0),
            Val::Px(out),
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
    use crate::widgets::space;
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
        assert_eq!(node.right, Val::Px(-(GUTTER_PX - EDGE_GAP_PX)));
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

    fn column(width: Val) -> Node {
        Node {
            width,
            overflow: Overflow::scroll_y(),
            ..default()
        }
    }

    #[test]
    fn a_bar_and_the_space_around_it_fit_a_panels_inset() {
        assert_eq!(GUTTER_PX, space::PANEL_INSET);
    }

    #[test]
    fn an_area_with_padding_gives_the_bar_its_own() {
        let padded = Node {
            padding: UiRect::all(Val::Px(space::SCREEN_GAP)),
            ..column(Val::Auto)
        };
        assert_eq!(lane_for(&padded, true, &[]), Lane::OwnPadding(GUTTER_PX));
        let thin = Node {
            padding: UiRect::all(Val::Px(space::CONTROLS)),
            ..column(Val::Auto)
        };
        assert_eq!(
            lane_for(&thin, true, &[]),
            Lane::OwnPadding(space::CONTROLS)
        );
    }

    #[test]
    fn an_area_reaches_into_the_padding_of_what_holds_it() {
        // The sidebar's sections: straight inside the padded dock.
        let edges = [AncestorEdge::Padded(12.0)];
        assert_eq!(
            lane_for(&column(Val::Percent(100.0)), true, &edges),
            Lane::Around(GUTTER_PX)
        );
        // A picker's list: through the unpadded columns it is built from.
        let edges = [
            AncestorEdge::Open,
            AncestorEdge::Open,
            AncestorEdge::Padded(8.0),
        ];
        assert_eq!(
            lane_for(&column(Val::Auto), true, &edges),
            Lane::Around(8.0),
            "no further than the padding allows"
        );
    }

    #[test]
    fn an_area_that_cannot_reach_out_keeps_its_lane_in_its_rows() {
        let edges = [AncestorEdge::Padded(12.0)];
        // Not stretched, or holding a width of its own, it cannot grow.
        assert_eq!(lane_for(&column(Val::Auto), false, &edges), Lane::Rows);
        assert_eq!(lane_for(&column(Val::Px(300.0)), true, &edges), Lane::Rows);
        // Something between that clips would hide the bar.
        let edges = [AncestorEdge::Clips, AncestorEdge::Padded(12.0)];
        assert_eq!(lane_for(&column(Val::Auto), true, &edges), Lane::Rows);
        // And with no padding anywhere near there is nowhere else for it.
        assert_eq!(lane_for(&column(Val::Auto), true, &[]), Lane::Rows);
    }

    #[test]
    fn a_list_in_a_padded_column_ends_flush_with_its_siblings() {
        let mut app = app();
        let parent = app
            .world_mut()
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(space::PANEL_INSET)),
                ..default()
            })
            .id();
        let area = app
            .world_mut()
            .spawn((ScrollArea, column(Val::Percent(100.0)), ChildOf(parent)))
            .id();
        app.update();
        let node = app.world().entity(area).get::<Node>().unwrap();
        assert_eq!(node.margin.right, Val::Px(-GUTTER_PX));
        assert_eq!(node.width, Val::Auto);
        assert_eq!(node.scrollbar_width, GUTTER_PX);
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
