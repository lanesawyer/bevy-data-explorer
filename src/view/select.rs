//! Dragging a rectangle over a frame to select the cells inside it.
//!
//! A frame takes a selection by carrying a [`SelectMode`], and only over a
//! source that advertises [`CellColumns`] — so the header's button appears only
//! where there are cells to select. While it is on, a drag draws a rectangle
//! rather than panning; the wheel still zooms, so a selection can be aimed
//! without leaving the mode.
//!
//! The rectangle is kept in display coordinates, which is what lets it stay
//! over the same cells as the frame is zoomed and panned. Turning it into the
//! dataset's own coordinates is the format's job, through
//! [`RegionProbe`][crate::source::region::RegionProbe]: the grid has no idea
//! whether the points under it were drawn where their coordinates put them or
//! laid out into a grid of sections.
//!
//! A press inside a rectangle already drawn carries it at the size it has,
//! rather than starting another; one outside draws a new one, and a click
//! outside clears it. Which of the three a press means is settled as it lands,
//! so a rectangle moving out from under the pointer never changes the meaning
//! of a gesture halfway through.
//!
//! Selecting is a property of the frame rather than of the dataset. Two frames
//! on one dataset would otherwise fight over one rectangle, and the frame the
//! outline is around is the one every other control already acts on.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersToolButton};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy_ui_widgets::Activate;

use crate::app::theme::token;
use crate::source::ShowsSource;
use crate::source::properties::CellColumns;
use crate::source::region::RegionProbe;
use crate::widgets::{BlocksFrameInput, Icon, button_icon, patch_node, set_display};

use super::grid::{panel_under_cursor, within_frames};
use super::{FrameArea, Panel};

/// Width of the rectangle's outline, in logical pixels.
const OUTLINE_PX: f32 = 1.0;

/// Drawn above the frame's own chrome, as the selection outline is, so the
/// rectangle is never half-hidden by a header it was dragged across.
const REGION_Z: i32 = 2;

/// The smallest drag counted as a selection rather than a click, in logical
/// pixels. Without it, clicking a frame to select it would also ask a service
/// to count a rectangle of nothing.
const MIN_DRAG_PX: f32 = 4.0;

/// A frame whose drags draw a rectangle instead of panning.
#[derive(Component, Clone, Copy, Default)]
pub struct SelectMode;

/// The rectangle a frame is showing, in display coordinates.
///
/// Both corners are kept as dragged rather than sorted, so the rectangle can be
/// drawn while a drag is still running in any direction.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FrameRegion {
    pub from: Vec2,
    pub to: Vec2,
}

impl FrameRegion {
    pub fn min(&self) -> Vec2 {
        self.from.min(self.to)
    }

    pub fn max(&self) -> Vec2 {
        self.from.max(self.to)
    }

    /// Whether `world` lies within the rectangle, so a press there takes hold
    /// of it rather than starting another.
    pub fn contains(&self, world: Vec2) -> bool {
        let (min, max) = (self.min(), self.max());
        (min.x..=max.x).contains(&world.x) && (min.y..=max.y).contains(&world.y)
    }

    /// The same rectangle, `delta` away.
    pub fn moved_by(&self, delta: Vec2) -> Self {
        FrameRegion {
            from: self.from + delta,
            to: self.to + delta,
        }
    }
}

/// A drag in progress, in the frame that started it.
#[derive(Default)]
pub struct RegionDrag {
    panel: Option<Entity>,
    /// Where the pointer went down, in screen pixels, for measuring the drag
    /// against [`MIN_DRAG_PX`] before it counts.
    started_at: Vec2,
    moved: bool,
    /// The rectangle as it stood when the press landed inside it, while it is
    /// being carried; nothing while a new one is being drawn out.
    carrying: Option<FrameRegion>,
}

/// The header button that turns a frame's selection mode on and off.
#[derive(Component, Clone)]
pub struct PanelSelectButton {
    panel: Entity,
}

impl Default for PanelSelectButton {
    fn default() -> Self {
        PanelSelectButton {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Add a frame's selection button to its header, hidden until its source has
/// cells to select.
pub(super) fn spawn_select_button(commands: &mut Commands, header: Entity, panel: Entity) {
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(Icon::BoxSelect)] }
            }
            BlocksFrameInput
            Node { display: { Display::None } }
            PanelSelectButton { panel: { panel } }
        })
        .id();
    commands.entity(header).add_child(button);
}

/// Show each frame's selection button only over a source with cells, and light
/// it while the mode is on.
pub fn sync_select_buttons(
    buttons: Query<(Entity, &PanelSelectButton)>,
    mut nodes: Query<&mut Node>,
    mut variants: Query<&mut ButtonVariant>,
    frames: Query<(&ShowsSource, Has<SelectMode>)>,
    cells: Query<(), With<CellColumns>>,
) {
    for (entity, button) in &buttons {
        let Ok((shows, selecting)) = frames.get(button.panel) else {
            continue;
        };
        set_display(&mut nodes, entity, cells.contains(shows.0));
        // Lit while the mode is on, so a frame that swallows drags says why.
        if let Ok(mut variant) = variants.get_mut(entity) {
            variant.set_if_neq(if selecting {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Normal
            });
        }
    }
}

/// Turn a frame's selection mode on, or off — which also drops the rectangle,
/// since leaving one on screen that no longer responds to a drag reads as the
/// frame having stopped working.
pub fn on_select_toggled(
    activate: On<Activate>,
    mut commands: Commands,
    buttons: Query<&PanelSelectButton>,
    frames: Query<(&ShowsSource, Has<SelectMode>)>,
    sources: Query<&crate::source::DataSource>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Ok((shows, selecting)) = frames.get(button.panel) else {
        return;
    };
    let name = sources.get(shows.0).map(|source| source.name.clone());
    if selecting {
        commands
            .entity(button.panel)
            .remove::<SelectMode>()
            .remove::<FrameRegion>();
    } else {
        commands.entity(button.panel).insert(SelectMode);
    }
    if let Ok(name) = name {
        info!(
            "{name}: {} cell selection",
            if selecting { "left" } else { "selecting" }
        );
    }
}

/// Drag out a rectangle in whichever selecting frame the pointer is over.
///
/// Runs before [`super::input::panel_controls`], which skips a frame in
/// selection mode, so one drag is never both a selection and a pan.
pub fn drag_region(
    mut commands: Commands,
    windows: Query<&Window>,
    area: Res<FrameArea>,
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<bevy::picking::hover::HoverMap>,
    chrome: Query<(), With<BlocksFrameInput>>,
    parents: Query<&ChildOf>,
    panels: Query<(Entity, &Panel, &Camera, &GlobalTransform), With<SelectMode>>,
    panel_indices: Query<&Panel>,
    mut regions: Query<&mut FrameRegion>,
    mut drag: Local<RegionDrag>,
) {
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    if !buttons.pressed(MouseButton::Left) {
        // A drag that never went anywhere was a click, and a click clears the
        // selection — unless it landed inside the rectangle, where it was a
        // grip that thought better of it. Clearing there would make a
        // rectangle something you lose by touching it.
        if let Some(panel) = drag.panel.take()
            && !drag.moved
            && drag.carrying.is_none()
        {
            commands.entity(panel).remove::<FrameRegion>();
        }
        drag.panel = None;
        drag.moved = false;
        drag.carrying = None;
        return;
    }

    if drag.panel.is_none() {
        if !buttons.just_pressed(MouseButton::Left) {
            return;
        }
        // A press on the frame's own chrome belongs to the chrome.
        if super::input::pointer_over_chrome(&hover, &chrome, &parents) {
            return;
        }
        let local = cursor - area.origin;
        if !within_frames(local, area.size) {
            return;
        }
        let count = panel_indices.iter().count();
        let index = panel_under_cursor(local, area.size, count);
        let Some((entity, _, camera, global)) =
            panels.iter().find(|(_, panel, ..)| panel.index == index)
        else {
            return;
        };
        // A press inside the rectangle carries it at the size it already has;
        // one outside starts another. Decided once, as the press lands, so a
        // rectangle dragged out from under the pointer does not change what
        // the gesture means halfway through.
        drag.carrying = camera
            .viewport_to_world_2d(global, cursor)
            .ok()
            .and_then(|world| regions.get(entity).ok().filter(|r| r.contains(world)))
            .copied();
        drag.panel = Some(entity);
        drag.started_at = cursor;
        drag.moved = false;
    }

    let Some(panel) = drag.panel else { return };
    let Ok((_, _, camera, global)) = panels.get(panel) else {
        // The frame left selection mode mid-drag.
        drag.panel = None;
        return;
    };
    if !drag.moved && cursor.distance(drag.started_at) < MIN_DRAG_PX {
        return;
    }
    let (Ok(from), Ok(to)) = (
        camera.viewport_to_world_2d(global, drag.started_at),
        camera.viewport_to_world_2d(global, cursor),
    ) else {
        return;
    };
    drag.moved = true;
    let wanted = match drag.carrying {
        // Carried by the world delta rather than by where the pointer is, so
        // the rectangle keeps its size and the spot it was grabbed by stays
        // under the pointer.
        Some(held) => held.moved_by(to - from),
        None => FrameRegion { from, to },
    };
    if let Ok(mut region) = regions.get_mut(panel) {
        region.set_if_neq(wanted);
    } else {
        commands.entity(panel).insert(wanted);
    }
}

/// Tell each selecting frame's source what rectangle was drawn over it.
///
/// The mirror of [`super::input::probe_hover`]: the grid writes where, and the
/// format answers with where that is in the data. A source no frame has a
/// rectangle over loses its probe, which is what stops it being counted.
pub fn probe_region(
    mut commands: Commands,
    frames: Query<(Entity, &ShowsSource, &FrameRegion)>,
    probed: Query<Entity, With<RegionProbe>>,
) {
    let mut wanted: Vec<(Entity, RegionProbe)> = Vec::new();
    for (panel, shows, region) in &frames {
        wanted.push((
            shows.0,
            RegionProbe {
                panel,
                min: region.min(),
                max: region.max(),
            },
        ));
    }
    for entity in &probed {
        if !wanted.iter().any(|(source, _)| *source == entity) {
            commands.entity(entity).remove::<RegionProbe>();
        }
    }
    for (source, probe) in wanted {
        commands.entity(source).insert(probe);
    }
}

/// The outline drawn round a frame's selection.
#[derive(Component, Clone)]
pub struct RegionOutline {
    panel: Entity,
}

impl Default for RegionOutline {
    fn default() -> Self {
        RegionOutline {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Keep one outline per frame, and drop those of frames that have gone away.
pub fn sync_region_outlines(
    mut commands: Commands,
    panels: Query<Entity, With<Panel>>,
    outlines: Query<(Entity, &RegionOutline)>,
) {
    for (entity, outline) in &outlines {
        if panels.get(outline.panel).is_err() {
            commands.entity(entity).despawn();
        }
    }
    for panel in &panels {
        if outlines.iter().any(|(_, o)| o.panel == panel) {
            continue;
        }
        commands.spawn_scene(bsn! {
            RegionOutline { panel: { panel } }
            // Decoration, like the frame's own selection border: a rectangle
            // that swallowed the pointer would stop the drag still sizing it.
            template_value(Pickable::IGNORE)
            Node {
                position_type: { PositionType::Absolute },
                border: { UiRect::all(Val::Px(OUTLINE_PX)) },
                display: { Display::None },
            }
            GlobalZIndex({ REGION_Z })
            ThemeBorderColor({ token::SELECTION })
            ThemeBackgroundColor({ token::OVERLAY_BG })
        });
    }
}

/// Put each outline over the rectangle its frame is showing, clipped to that
/// frame's cell.
///
/// A rectangle stays over the same cells while the frame is panned, so it can
/// be dragged partly out of view — and would otherwise be drawn across the
/// frame beside it.
pub fn place_region_outlines(
    area: Res<FrameArea>,
    panels: Query<(&Panel, &Camera, &GlobalTransform, Option<&FrameRegion>)>,
    mut outlines: Query<(&RegionOutline, &mut Node)>,
) {
    let count = panels.iter().count();
    for (outline, node) in &mut outlines {
        let Ok((panel, camera, global, region)) = panels.get(outline.panel) else {
            continue;
        };
        let Some(region) = region else {
            patch_node(node, |node| node.display = Display::None);
            continue;
        };
        let (Ok(min), Ok(max)) = (
            camera.world_to_viewport(global, region.min().extend(0.0)),
            camera.world_to_viewport(global, region.max().extend(0.0)),
        ) else {
            patch_node(node, |node| node.display = Display::None);
            continue;
        };
        // World y runs the other way from screen y, so the corners cross over.
        let cell = area.cell(count, panel.index);
        let left = min.x.min(max.x).max(cell.min.x);
        let right = min.x.max(max.x).min(cell.max.x);
        let top = min.y.min(max.y).max(cell.min.y);
        let bottom = min.y.max(max.y).min(cell.max.y);
        if right <= left || bottom <= top {
            patch_node(node, |node| node.display = Display::None);
            continue;
        }
        patch_node(node, |node| {
            node.display = Display::Flex;
            node.left = Val::Px(left);
            node.top = Val::Px(top);
            node.width = Val::Px(right - left);
            node.height = Val::Px(bottom - top);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_reads_the_same_whichever_way_it_was_dragged() {
        let down = FrameRegion {
            from: Vec2::new(5.0, -10.0),
            to: Vec2::new(15.0, -30.0),
        };
        let up = FrameRegion {
            from: Vec2::new(15.0, -30.0),
            to: Vec2::new(5.0, -10.0),
        };
        assert_eq!(down.min(), up.min());
        assert_eq!(down.max(), up.max());
        assert_eq!(down.min(), Vec2::new(5.0, -30.0));
    }

    #[test]
    fn a_press_inside_the_rectangle_is_a_grip_on_it() {
        // Which one a press means is decided by where it lands, and the
        // rectangle is held in display space, where y runs negative.
        let region = FrameRegion {
            from: Vec2::new(5.0, -10.0),
            to: Vec2::new(15.0, -30.0),
        };
        assert!(region.contains(Vec2::new(10.0, -20.0)));
        // Its edges count: a press on the outline is a press on the thing.
        assert!(region.contains(Vec2::new(5.0, -10.0)));
        assert!(region.contains(Vec2::new(15.0, -30.0)));
        assert!(!region.contains(Vec2::new(4.0, -20.0)));
        assert!(!region.contains(Vec2::new(10.0, -9.0)));
        assert!(!region.contains(Vec2::new(10.0, -31.0)));
    }

    #[test]
    fn a_rectangle_dragged_backwards_is_still_grabbed_from_inside() {
        // The corners are kept as dragged rather than sorted, so containment
        // cannot assume which of them is the low one.
        let forward = FrameRegion {
            from: Vec2::new(5.0, -10.0),
            to: Vec2::new(15.0, -30.0),
        };
        let backward = FrameRegion {
            from: forward.to,
            to: forward.from,
        };
        assert!(backward.contains(Vec2::new(10.0, -20.0)));
        assert!(!backward.contains(Vec2::new(40.0, -20.0)));
    }

    #[test]
    fn carrying_a_rectangle_keeps_its_size() {
        let region = FrameRegion {
            from: Vec2::new(5.0, -10.0),
            to: Vec2::new(15.0, -30.0),
        };
        let carried = region.moved_by(Vec2::new(100.0, -7.0));
        assert_eq!(carried.max() - carried.min(), region.max() - region.min());
        assert_eq!(carried.min(), region.min() + Vec2::new(100.0, -7.0));
    }

    #[test]
    fn the_spot_a_rectangle_was_grabbed_by_stays_under_the_pointer() {
        // Carried by the delta from where the press landed, not by putting a
        // corner or the middle under the pointer, which would make it jump the
        // moment it was grabbed anywhere else.
        let region = FrameRegion {
            from: Vec2::new(5.0, -10.0),
            to: Vec2::new(15.0, -30.0),
        };
        let grabbed_at = Vec2::new(7.0, -12.0);
        let now_at = Vec2::new(57.0, -42.0);
        let carried = region.moved_by(now_at - grabbed_at);
        // The grabbed spot sat 2 in and 2 down from the corner, and still does.
        assert_eq!(carried.min() + (grabbed_at - region.min()), now_at);
        assert!(carried.contains(now_at));
    }

    #[test]
    fn the_outline_draws_above_the_frames_own_selection_border() {
        // The header is drawn over the frame, and a rectangle dragged across
        // it would otherwise disappear under it.
        const { assert!(REGION_Z > super::super::chrome::SELECTION_Z) };
    }
}
