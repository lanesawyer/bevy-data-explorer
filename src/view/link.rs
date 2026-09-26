//! Frames that pan, zoom and page together.
//!
//! A frame is linked by the chain button in its header. Every linked flat
//! frame follows whichever of them last moved — by a drag, the wheel, `R`, or
//! anything else that writes its camera — so one gesture compares the same
//! place across several datasets. A frame that joins takes the group's view
//! rather than pulling the group to its own.
//!
//! What is shared is a place and a magnification: a point in space, and world
//! units per screen pixel. A frame's point is its center, along the axes it
//! shows across and down, and its slice, along the axis it pages through —
//! which [`SourceAxes`] names — so a volume cut looking down z and the same
//! volume cut looking along x meet where they cross, as Neuroglancer's
//! panels do: paging one moves the line the other is centered on. A source
//! that names no axes is flat, x across and y down.
//!
//! Frames of different sizes show more or less around the same point at the
//! same zoom. Two datasets measured in different lengths are converted, so a
//! stack in microns and a cloud in millimeters still line up; a slide in its
//! own pixels shares nothing with either and is left where it is. A frame
//! looking at a volume in 3D, or filled with a table, has no flat view to
//! share and sits the group out until it has one.
//!
//! A stack with no place along its depth — sections with no z — is paged
//! step for step with the frame being paged instead, which is
//! `view/input.rs`'s side of this.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersToolButton};
use bevy_ui_widgets::Activate;

use crate::source::stack::{SliceStack, SourceAxes};
use crate::source::table::SourceTable;
use crate::source::{DataSource, ShowsSource, ViewLimits};
use crate::widgets::{BlocksFrameInput, Icon, button_icon, set_display};

use super::{Orbit, Panel, SelectedPanel};

/// A frame that follows the others linked with it.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Linked {
    /// The view last written to this frame, or read from it, by the link: a
    /// frame whose view no longer matches has moved on its own, and leads.
    /// Nothing until the frame has been seen linked once, which is what marks
    /// a frame that has just joined.
    last: Option<Seen>,
}

/// A frame's view as the link compares it: center, scale, and slice.
type Seen = (Vec2, f32, Option<u64>);

/// The header button that links and unlinks a frame.
#[derive(Component, Clone)]
pub struct PanelLinkButton {
    panel: Entity,
}

impl Default for PanelLinkButton {
    fn default() -> Self {
        PanelLinkButton {
            panel: Entity::PLACEHOLDER,
        }
    }
}

/// Add a frame's link button to its header, hidden until it shows a source
/// with a view to share.
pub(super) fn spawn_link_button(commands: &mut Commands, header: Entity, panel: Entity) {
    let button = commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_icon(Icon::Link)] }
            }
            BlocksFrameInput
            Node { display: { Display::None } }
            PanelLinkButton { panel: { panel } }
        })
        .id();
    commands.entity(header).add_child(button);
}

/// Show each link button over anything but a table, lit while linked.
pub fn sync_link_buttons(
    buttons: Query<(Entity, &PanelLinkButton)>,
    mut nodes: Query<&mut Node>,
    mut variants: Query<&mut ButtonVariant>,
    frames: Query<(&ShowsSource, Has<Linked>)>,
    tables: Query<(), With<SourceTable>>,
) {
    for (entity, button) in &buttons {
        let Ok((shows, linked)) = frames.get(button.panel) else {
            continue;
        };
        set_display(&mut nodes, entity, !tables.contains(shows.0));
        if let Ok(mut variant) = variants.get_mut(entity) {
            variant.set_if_neq(if linked {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Normal
            });
        }
    }
}

pub fn on_link_toggled(
    activate: On<Activate>,
    mut commands: Commands,
    buttons: Query<&PanelLinkButton>,
    frames: Query<Has<Linked>>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Ok(linked) = frames.get(button.panel) else {
        return;
    };
    if linked {
        commands.entity(button.panel).remove::<Linked>();
    } else {
        commands.entity(button.panel).insert(Linked::default());
    }
}

/// One linked frame, as the link reads it.
struct Member {
    entity: Entity,
    index: usize,
    source: Entity,
    view: Seen,
    last: Option<Seen>,
    axes: SourceAxes,
}

impl Member {
    /// Where this frame is along each axis it names.
    fn point(&self) -> Vec<(char, f32)> {
        let (center, _, slice) = self.view;
        let mut point = vec![(self.axes.across, center.x), (self.axes.down, -center.y)];
        if let (Some(through), Some(slice)) = (self.axes.through, slice) {
            point.push((through.axis, through.at(slice)));
        }
        point
    }
}

/// Which member the rest follow this frame, if any: one that moved, the
/// selected one first; or, when frames have only just joined, one that was
/// already linked, so joining never drags the group.
fn leader(members: &[Member], selected: Option<Entity>) -> Option<usize> {
    let first = |pick: &dyn Fn(&Member) -> bool| {
        members
            .iter()
            .position(|member| pick(member) && Some(member.entity) == selected)
            .or_else(|| {
                members
                    .iter()
                    .enumerate()
                    .filter(|(_, member)| pick(member))
                    .min_by_key(|(_, member)| member.index)
                    .map(|(at, _)| at)
            })
    };
    if let Some(moved) = first(&|member| member.last.is_some_and(|last| last != member.view)) {
        return Some(moved);
    }
    if members.iter().all(|member| member.last.is_some()) {
        return None;
    }
    first(&|member| member.last.is_some()).or_else(|| first(&|_| true))
}

/// Where `member` should look to share `point`, at `scale` of the leader's
/// units, `factor` of its own to one of the leader's: each axis it names that
/// the point has, and its own view for the rest.
fn follow(member: &Member, point: &[(char, f32)], scale: f32, factor: f32, count: u64) -> Seen {
    let along = |axis: char| {
        point
            .iter()
            .find(|(named, _)| *named == axis)
            .map(|(_, at)| at * factor)
    };
    let (center, _, slice) = member.view;
    let center = Vec2::new(
        along(member.axes.across).unwrap_or(center.x),
        along(member.axes.down).map_or(center.y, |at| -at),
    );
    let slice = match (member.axes.through, slice) {
        (Some(through), Some(slice)) => {
            Some(along(through.axis).map_or(slice, |at| through.slice_at(at, count)))
        }
        _ => slice,
    };
    (center, scale * factor, slice)
}

/// Move every linked frame to the place and zoom of whichever one leads.
pub fn follow_links(
    selected: Res<SelectedPanel>,
    mut frames: Query<
        (
            Entity,
            &Panel,
            &ShowsSource,
            &mut Transform,
            &mut Projection,
            &ViewLimits,
            &mut Linked,
        ),
        Without<Orbit>,
    >,
    sources: Query<(&DataSource, Option<&SourceAxes>)>,
    mut stacks: Query<&mut SliceStack>,
    tables: Query<(), With<SourceTable>>,
) {
    let members: Vec<Member> = frames
        .iter()
        .filter(|(_, _, shows, ..)| !tables.contains(shows.0))
        .filter_map(|(entity, panel, shows, transform, projection, _, linked)| {
            let Projection::Orthographic(ortho) = projection else {
                return None;
            };
            let axes = sources
                .get(shows.0)
                .ok()
                .and_then(|(_, axes)| axes.copied())
                .unwrap_or_default();
            let slice = stacks.get(shows.0).ok().map(|stack| stack.current);
            Some(Member {
                entity,
                index: panel.index,
                source: shows.0,
                view: (transform.translation.truncate(), ortho.scale, slice),
                last: linked.last,
                axes,
            })
        })
        .collect();
    let Some(lead) = leader(&members, selected.0) else {
        return;
    };
    let point = members[lead].point();
    let scale = members[lead].view.1;
    let Ok((from, _)) = sources.get(members[lead].source) else {
        return;
    };

    for member in &members {
        let Ok((_, _, _, mut transform, mut projection, limits, mut linked)) =
            frames.get_mut(member.entity)
        else {
            continue;
        };
        let factor = sources
            .get(member.source)
            .ok()
            .and_then(|(to, _)| from.units_in(to));
        let seen = match factor {
            Some(factor) if member.entity != members[lead].entity => {
                let count = stacks.get(member.source).map_or(1, |stack| stack.count);
                let (center, scale, slice) = follow(member, &point, scale, factor, count);
                let scale = scale.clamp(limits.min_scale, limits.max_scale);
                if center != member.view.0 {
                    transform.translation = center.extend(transform.translation.z);
                }
                if scale != member.view.1
                    && let Projection::Orthographic(ortho) = projection.as_mut()
                {
                    ortho.scale = scale;
                }
                // Read before writing: a stack taken mutably is a stack
                // changed, and a changed stack reads its tiles again.
                if let Some(slice) = slice
                    && slice != member.view.2.unwrap_or(slice)
                    && let Ok(mut stack) = stacks.get_mut(member.source)
                {
                    stack.go_to(slice);
                }
                (center, scale, slice)
            }
            // The leader, and anything in a space of its own, stay put.
            _ => member.view,
        };
        if linked.last != Some(seen) {
            linked.last = Some(seen);
        }
    }
}

/// Two presses of the left button closer than this, in seconds and logical
/// pixels, are a double-click.
const DOUBLE_CLICK_SECS: f32 = 0.4;
const DOUBLE_CLICK_PX: f32 = 6.0;

/// Double-click in a linked frame to move the point the link shares there:
/// the frame is centered on what was clicked, and the others follow it, as a
/// click moves the position in Neuroglancer.
pub fn recenter_on_double_click(
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    windows: Query<&Window>,
    hover: Res<bevy::picking::hover::HoverMap>,
    chrome: Query<(), With<BlocksFrameInput>>,
    parents: Query<&ChildOf>,
    mut frames: Query<
        (&Camera, &GlobalTransform, &mut Transform, &Projection),
        (
            With<Linked>,
            With<Panel>,
            Without<Orbit>,
            Without<super::SelectMode>,
        ),
    >,
    mut last: Local<Option<(f32, Vec2)>>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        return;
    };
    if super::input::pointer_over_chrome(&hover, &chrome, &parents) {
        *last = None;
        return;
    }
    let now = time.elapsed_secs();
    let double = last.is_some_and(|(at, from)| {
        now - at <= DOUBLE_CLICK_SECS && from.distance(cursor) <= DOUBLE_CLICK_PX
    });
    if !double {
        *last = Some((now, cursor));
        return;
    }
    *last = None;
    for (camera, global, mut transform, projection) in &mut frames {
        let inside = camera
            .logical_viewport_rect()
            .is_some_and(|rect| rect.contains(cursor));
        if !inside || !matches!(projection, Projection::Orthographic(_)) {
            continue;
        }
        if let Ok(world) = camera.viewport_to_world_2d(global, cursor) {
            transform.translation = world.extend(transform.translation.z);
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(index: usize, view: f32, last: Option<f32>) -> Member {
        Member {
            entity: Entity::from_raw_u32(index as u32 + 1).unwrap(),
            index,
            source: Entity::PLACEHOLDER,
            view: (Vec2::splat(view), 1.0, None),
            last: last.map(|last| (Vec2::splat(last), 1.0, None)),
            axes: SourceAxes::default(),
        }
    }

    fn cut(across: char, down: char, through: char, center: Vec2, slice: u64) -> Member {
        Member {
            entity: Entity::PLACEHOLDER,
            index: 0,
            source: Entity::PLACEHOLDER,
            view: (center, 2.0, Some(slice)),
            last: None,
            axes: SourceAxes {
                across,
                down,
                through: Some(crate::source::stack::Through {
                    axis: through,
                    origin: 0.0,
                    step: 2.0,
                }),
            },
        }
    }

    #[test]
    fn frames_cut_two_ways_meet_where_they_cross() {
        // Looking down z at x 100, y 300, on slice 40 (z 80).
        let flat = cut('x', 'y', 'z', Vec2::new(100.0, -300.0), 40);
        // Looking along x, z across and y down.
        let side = cut('z', 'y', 'x', Vec2::new(7.0, -9.0), 3);
        let (center, scale, slice) = follow(&side, &flat.point(), 2.0, 1.0, 1000);
        assert_eq!(
            center,
            Vec2::new(80.0, -300.0),
            "centered on z 80 and y 300"
        );
        assert_eq!(slice, Some(50), "on the slice at x 100");
        assert_eq!(scale, 2.0);
    }

    #[test]
    fn a_flat_source_follows_only_the_axes_it_has() {
        let flat = cut('x', 'y', 'z', Vec2::new(100.0, -300.0), 40);
        let cloud = member(1, 5.0, Some(5.0));
        // A cloud in millimeters following a stack in microns.
        let (center, _, slice) = follow(&cloud, &flat.point(), 2.0, 1e-3, 1);
        assert_eq!(center, Vec2::new(0.1, -0.3));
        assert_eq!(slice, None);
    }

    #[test]
    fn a_frame_that_moved_leads() {
        let members = [member(0, 1.0, Some(1.0)), member(1, 5.0, Some(1.0))];
        assert_eq!(leader(&members, None), Some(1));
    }

    #[test]
    fn nothing_leads_while_nothing_moves() {
        let members = [member(0, 1.0, Some(1.0)), member(1, 1.0, Some(1.0))];
        assert_eq!(leader(&members, None), None);
    }

    #[test]
    fn a_frame_joining_follows_the_group_rather_than_leading_it() {
        let members = [member(0, 9.0, None), member(1, 1.0, Some(1.0))];
        let selected = Some(members[0].entity);
        assert_eq!(leader(&members, selected), Some(1));
    }

    #[test]
    fn the_selected_frame_leads_when_several_moved() {
        let members = [member(0, 2.0, Some(1.0)), member(1, 3.0, Some(1.0))];
        assert_eq!(leader(&members, Some(members[1].entity)), Some(1));
        assert_eq!(leader(&members, None), Some(0));
    }

    fn app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<SelectedPanel>()
            .add_systems(Update, follow_links);
        let mut source = |unit: &str| {
            app.world_mut()
                .spawn(DataSource {
                    name: unit.into(),
                    unit: unit.into(),
                    detail: String::new(),
                    stat: String::new(),
                    category: crate::source::Category::Image,
                    layer: 1,
                })
                .id()
        };
        let (microns, millimeters) = (source("um"), source("millimeter"));
        let mut frame = |index: usize, source: Entity, at: Vec2| {
            app.world_mut()
                .spawn((
                    Panel { index },
                    ShowsSource(source),
                    Transform::from_translation(at.extend(0.0)),
                    Projection::Orthographic(OrthographicProjection::default_2d()),
                    ViewLimits {
                        min_scale: 1e-9,
                        max_scale: 1e9,
                        fit_scale: 1.0,
                        center: Vec2::ZERO,
                    },
                    Linked::default(),
                ))
                .id()
        };
        let a = frame(0, microns, Vec2::new(1000.0, 2000.0));
        let b = frame(1, millimeters, Vec2::new(-7.0, 3.0));
        (app, a, b)
    }

    #[test]
    fn a_frame_in_millimeters_follows_one_in_microns() {
        let (mut app, a, b) = app();
        app.update();
        // Both joined together, so the first leads and the second is brought
        // to the same place, a thousandth the size in its own units.
        let at = app.world().get::<Transform>(b).unwrap().translation;
        assert!(
            (at.truncate() - Vec2::new(1.0, 2.0)).length() < 1e-4,
            "{at}"
        );

        app.world_mut()
            .get_mut::<Transform>(b)
            .unwrap()
            .translation
            .x = 3.0;
        app.update();
        let at = app.world().get::<Transform>(a).unwrap().translation;
        assert!((at.x - 3000.0).abs() < 1e-2, "{at}");
    }
}
