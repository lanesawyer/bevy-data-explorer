//! Frames that pan, zoom and page together.
//!
//! A frame is linked by the chain button in its header. Every linked flat
//! frame follows whichever of them last moved — by a drag, the wheel, `R`, or
//! anything else that writes its camera — so one gesture compares the same
//! place across several datasets. A frame that joins takes the group's view
//! rather than pulling the group to its own.
//!
//! What is shared is a place and a magnification: the point at the center of
//! the frame, and world units per screen pixel. Frames of different sizes
//! therefore show more or less around the same point at the same zoom. Two
//! datasets measured in different lengths are converted, so a stack in
//! microns and a cloud in millimeters still line up; a slide in its own
//! pixels shares nothing with either and is left where it is. A frame looking
//! at a volume in 3D, or filled with a table, has no flat view to share and
//! sits the group out until it has one.
//!
//! Paging a linked frame's stack pages every linked frame's stack by the same
//! step, which is `view/input.rs`'s side of this.

use bevy::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersToolButton};
use bevy_ui_widgets::Activate;

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
    last: Option<(Vec2, f32)>,
}

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
    view: (Vec2, f32),
    last: Option<(Vec2, f32)>,
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

/// Move every linked frame to the view of whichever one leads.
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
    sources: Query<&DataSource>,
    tables: Query<(), With<SourceTable>>,
) {
    let members: Vec<Member> = frames
        .iter()
        .filter(|(_, _, shows, ..)| !tables.contains(shows.0))
        .filter_map(|(entity, panel, shows, transform, projection, _, linked)| {
            let Projection::Orthographic(ortho) = projection else {
                return None;
            };
            Some(Member {
                entity,
                index: panel.index,
                source: shows.0,
                view: (transform.translation.truncate(), ortho.scale),
                last: linked.last,
            })
        })
        .collect();
    let Some(lead) = leader(&members, selected.0) else {
        return;
    };
    let (center, scale) = members[lead].view;
    let Ok(from) = sources.get(members[lead].source) else {
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
            .and_then(|to| from.units_in(to));
        let view = match factor {
            Some(factor) if member.entity != members[lead].entity => {
                let view = (
                    center * factor,
                    (scale * factor).clamp(limits.min_scale, limits.max_scale),
                );
                if view != member.view {
                    transform.translation = view.0.extend(transform.translation.z);
                    if let Projection::Orthographic(ortho) = projection.as_mut() {
                        ortho.scale = view.1;
                    }
                }
                view
            }
            // The leader, and anything in a space of its own, stay put.
            _ => member.view,
        };
        if linked.last != Some(view) {
            linked.last = Some(view);
        }
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
            view: (Vec2::splat(view), 1.0),
            last: last.map(|last| (Vec2::splat(last), 1.0)),
        }
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
