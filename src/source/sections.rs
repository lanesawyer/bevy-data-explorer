//! Sources drawn under the sections of another.
//!
//! A sectioned source pulls its sections apart — into a grid, or one at a time
//! — so what was measured alongside a section only lines up with it if it is
//! moved the same way. The sectioned source says where each section is now
//! ([`SectionPlacement`]); a source that follows it ([`FollowsSections`]) is
//! handed a copy of itself for each of its sections, cut to that section and
//! moved with it ([`Placements`]). Neither format has to know the other's.

use bevy::prelude::*;

use super::SourceUrl;

/// One section of a sectioned source, and where it is drawn now.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedSection {
    /// The dataset's own name for the section.
    pub id: String,
    /// What the section covers, in display coordinates before it is moved.
    pub bounds: Rect,
    /// How far it is moved to where it is drawn.
    pub offset: Vec2,
    pub shown: bool,
}

/// Where a sectioned source draws each of its sections, kept current by the
/// format as its layout changes.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct SectionPlacement(pub Vec<PlacedSection>);

/// One copy of a source: what part of it is drawn, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The part drawn, in the source's own display coordinates.
    pub window: Rect,
    /// How far that part is moved.
    pub offset: Vec2,
    pub shown: bool,
}

/// A source drawn as copies of parts of itself rather than whole where it
/// lies. Empty draws nothing. A format that cannot place copies ignores it.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Placements(pub Vec<Placement>);

/// Draw this source under the named sections of another, found by address.
#[derive(Component, Clone, Debug)]
pub struct FollowsSections {
    pub sections: String,
    /// Which sections, by their ids.
    pub ids: Vec<String>,
}

/// Keep each follower's copies where its sections are.
///
/// A copy is cut to its section's bounds: the image under one section is
/// usually far larger than the section, and uncut it would cover its
/// neighbours in the grid.
pub fn follow_sections(
    mut commands: Commands,
    followers: Query<(Entity, &FollowsSections, Option<&Placements>)>,
    sections: Query<(&SourceUrl, &SectionPlacement)>,
) {
    for (entity, follows, current) in &followers {
        let placed = sections
            .iter()
            .find(|(url, _)| url.0 == follows.sections)
            .map(|(_, placement)| placements_for(placement, &follows.ids))
            .unwrap_or_default();
        if current != Some(&placed) {
            commands.entity(entity).insert(placed);
        }
    }
}

fn placements_for(placement: &SectionPlacement, ids: &[String]) -> Placements {
    Placements(
        placement
            .0
            .iter()
            .filter(|section| ids.contains(&section.id))
            .map(|section| Placement {
                window: section.bounds,
                offset: section.offset,
                shown: section.shown,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: &str, x: f32, shown: bool) -> PlacedSection {
        PlacedSection {
            id: id.into(),
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            offset: Vec2::new(x, 0.0),
            shown,
        }
    }

    #[test]
    fn a_follower_is_copied_under_its_own_sections_only() {
        let placement = SectionPlacement(vec![
            section("a", 0.0, true),
            section("b", 20.0, true),
            section("c", 40.0, false),
        ]);
        let placed = placements_for(&placement, &["b".into(), "c".into()]);
        assert_eq!(placed.0.len(), 2);
        assert_eq!(placed.0[0].offset, Vec2::new(20.0, 0.0));
        assert!(!placed.0[1].shown);
    }

    #[test]
    fn a_follower_draws_nothing_until_its_sections_are_open() {
        let mut app = App::new();
        app.add_systems(Update, follow_sections);
        let follower = app
            .world_mut()
            .spawn(FollowsSections {
                sections: "points".into(),
                ids: vec!["a".into()],
            })
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Placements>(follower),
            Some(&Placements::default())
        );

        app.world_mut().spawn((
            SourceUrl("points".into()),
            SectionPlacement(vec![section("a", 5.0, true)]),
        ));
        app.update();
        let placed = app.world().get::<Placements>(follower).unwrap();
        assert_eq!(placed.0[0].offset, Vec2::new(5.0, 0.0));
    }
}
