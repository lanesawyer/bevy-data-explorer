//! A lineage and the frames opened from it, pointing at each other.
//!
//! A card whose record is stored somewhere names the address it opens, and
//! a frame opened from an address keeps it as its source's [`SourceUrl`];
//! the two are matched on that and nothing else, so a record opened some
//! other way — typed in, or from the inspector — is found all the same.
//!
//! Hovering such a card outlines every frame showing its record. Selecting a
//! frame marks every card whose record it shows, in the accent the selected
//! frame is outlined in. And a card's open button, on a record already in a
//! frame, selects that frame rather than opening it a second time.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy_feathers::theme::ThemeBorderColor;
#[cfg(test)]
use bevy_feathers::theme::ThemeToken;

use crate::app::theme::token;
use crate::source::{ShowsSource, SourceUrl};
use crate::widgets::patch_node;

use super::super::chrome::SELECTION_Z;
use super::super::{FrameArea, Panel, SelectedPanel};

/// How thick the outline round a frame showing a hovered card's record is:
/// thicker than the selection's, so the two are told apart when they fall
/// on the same frame.
const OUTLINE_PX: f32 = 4.0;

/// A card, and the address its record opens as if it is stored somewhere.
#[derive(Component, Clone, Default)]
pub struct CardRecord {
    pub(super) opens: Option<String>,
    /// Whether it is the record the lineage is drawn from, whose border sets
    /// it apart when it is not marked.
    pub(super) root: bool,
}

/// The address of the record on the card under the pointer.
#[derive(Resource, Default, PartialEq)]
pub struct HoveredRecord(Option<String>);

/// An outline round a frame showing the hovered card's record.
#[derive(Component)]
pub struct RecordOutline;

/// Whether two addresses name the same thing, as typed and as listed.
pub(super) fn same_address(a: &str, b: &str) -> bool {
    let bare = |it: &str| it.trim().trim_end_matches('/').to_string();
    bare(a) == bare(b)
}

/// The frames showing whatever is at `url`.
pub(super) fn frames_showing<'a>(
    url: &'a str,
    panels: &'a Query<(Entity, &Panel, &ShowsSource)>,
    urls: &'a Query<&SourceUrl>,
) -> impl Iterator<Item = (Entity, &'a Panel)> + 'a {
    panels.iter().filter_map(move |(entity, panel, shows)| {
        urls.get(shows.0)
            .is_ok_and(|shown| same_address(&shown.0, url))
            .then_some((entity, panel))
    })
}

/// Note which card's record is under the pointer, if any.
pub fn hover_records(
    hover: Res<HoverMap>,
    cards: Query<&CardRecord>,
    parents: Query<&ChildOf>,
    mut hovered: ResMut<HoveredRecord>,
) {
    let under = hover
        .values()
        .flat_map(|hits| hits.keys())
        .find_map(|entity| {
            std::iter::once(*entity)
                .chain(parents.iter_ancestors(*entity))
                .find_map(|it| cards.get(it).ok())
        })
        .and_then(|card| card.opens.clone());
    hovered.set_if_neq(HoveredRecord(under));
}

/// Outline each frame showing the hovered card's record.
pub fn outline_hovered_frames(
    mut commands: Commands,
    hovered: Res<HoveredRecord>,
    area: Res<FrameArea>,
    panels: Query<(Entity, &Panel, &ShowsSource)>,
    urls: Query<&SourceUrl>,
    mut outlines: Query<(Entity, &mut Node), With<RecordOutline>>,
) {
    let count = panels.iter().count();
    let cells: Vec<Rect> = hovered
        .0
        .as_deref()
        .map(|url| {
            frames_showing(url, &panels, &urls)
                .map(|(_, panel)| area.cell(count, panel.index))
                .collect()
        })
        .unwrap_or_default();
    let mut outlines: Vec<(Entity, Mut<Node>)> = outlines.iter_mut().collect();
    while outlines.len() > cells.len() {
        let (entity, _) = outlines.pop().expect("longer than the cells");
        commands.entity(entity).despawn();
    }
    for (at, cell) in cells.iter().enumerate() {
        let place = |node: &mut Node| {
            node.left = Val::Px(cell.min.x);
            node.top = Val::Px(cell.min.y);
            node.width = Val::Px(cell.width());
            node.height = Val::Px(cell.height());
        };
        match outlines.get_mut(at) {
            Some((_, node)) => patch_node(node.reborrow(), place),
            None => {
                let mut node = Node {
                    position_type: PositionType::Absolute,
                    border: UiRect::all(Val::Px(OUTLINE_PX)),
                    ..default()
                };
                place(&mut node);
                commands.spawn((
                    RecordOutline,
                    node,
                    // Decoration only, as the selection outline is.
                    Pickable::IGNORE,
                    GlobalZIndex(SELECTION_Z),
                    ThemeBorderColor(token::SELECTION),
                ));
            }
        }
    }
}

/// Mark each card whose record the selected frame shows.
pub fn mark_shown_cards(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<&ShowsSource, With<Panel>>,
    urls: Query<&SourceUrl>,
    cards: Query<(Entity, &CardRecord, &ThemeBorderColor)>,
) {
    let shown = selected
        .0
        .and_then(|panel| panels.get(panel).ok())
        .and_then(|shows| urls.get(shows.0).ok());
    for (entity, card, border) in &cards {
        let marked = shown
            .zip(card.opens.as_deref())
            .is_some_and(|(shown, opens)| same_address(&shown.0, opens));
        let wanted = match (marked, card.root) {
            (true, _) => token::SELECTION,
            (false, true) => token::OVERLAY_TEXT,
            (false, false) => token::DIVIDER,
        };
        // Immutable, so replaced rather than changed.
        if border.0 != wanted {
            commands.entity(entity).insert(ThemeBorderColor(wanted));
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy_ui_widgets::Activate;

    use super::super::{DatasetRequest, OpenStored, on_open_stored};
    use super::*;

    const STORE: &str = "s3://bucket/aligned.zarr/";

    /// Two frames: a lineage in the first, and in the second an image
    /// opened from `STORE` without its last slash.
    struct Scene {
        app: App,
        lineage: Entity,
        image: Entity,
        root: Entity,
        stored: Entity,
        other: Entity,
    }

    fn scene() -> Scene {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<DatasetRequest>()
            .insert_resource(FrameArea {
                origin: Vec2::ZERO,
                size: Vec2::new(1000.0, 500.0),
            })
            .init_resource::<SelectedPanel>()
            .init_resource::<HoveredRecord>()
            .add_observer(on_open_stored);
        let world = app.world_mut();
        let graph = world.spawn_empty().id();
        let store = world
            .spawn(SourceUrl("s3://bucket/aligned.zarr".into()))
            .id();
        let lineage = world.spawn((Panel { index: 0 }, ShowsSource(graph))).id();
        let image = world.spawn((Panel { index: 1 }, ShowsSource(store))).id();
        let mut card = |opens: Option<&str>, root: bool| {
            world
                .spawn((
                    CardRecord {
                        opens: opens.map(str::to_string),
                        root,
                    },
                    ThemeBorderColor(token::DIVIDER),
                ))
                .id()
        };
        let root = card(None, true);
        let stored = card(Some(STORE), false);
        let other = card(Some("s3://bucket/elsewhere.zarr/"), false);
        Scene {
            app,
            lineage,
            image,
            root,
            stored,
            other,
        }
    }

    fn border(scene: &Scene, card: Entity) -> ThemeToken {
        scene
            .app
            .world()
            .get::<ThemeBorderColor>(card)
            .unwrap()
            .0
            .clone()
    }

    #[test]
    fn selecting_a_frame_marks_the_cards_whose_record_it_shows() {
        let mut scene = scene();
        scene.app.world_mut().resource_mut::<SelectedPanel>().0 = Some(scene.image);
        scene
            .app
            .world_mut()
            .run_system_once(mark_shown_cards)
            .unwrap();
        assert_eq!(border(&scene, scene.stored), token::SELECTION);
        assert_eq!(border(&scene, scene.other), token::DIVIDER);
        assert_eq!(border(&scene, scene.root), token::OVERLAY_TEXT);

        scene.app.world_mut().resource_mut::<SelectedPanel>().0 = Some(scene.lineage);
        scene
            .app
            .world_mut()
            .run_system_once(mark_shown_cards)
            .unwrap();
        assert_eq!(border(&scene, scene.stored), token::DIVIDER);
    }

    #[test]
    fn hovering_a_card_outlines_the_frame_showing_its_record() {
        let mut scene = scene();
        let world = scene.app.world_mut();
        world.insert_resource(HoveredRecord(Some(STORE.into())));
        world.run_system_once(outline_hovered_frames).unwrap();
        let cell = world.resource::<FrameArea>().cell(2, 1);
        let mut outlines = world.query_filtered::<&Node, With<RecordOutline>>();
        let placed: Vec<Val> = outlines.iter(world).map(|node| node.left).collect();
        assert_eq!(placed, [Val::Px(cell.min.x)]);

        world.insert_resource(HoveredRecord(None));
        world.run_system_once(outline_hovered_frames).unwrap();
        assert_eq!(outlines.iter(world).count(), 0);
    }

    #[test]
    fn opening_a_record_already_in_a_frame_selects_that_frame() {
        let mut scene = scene();
        let world = scene.app.world_mut();
        world.resource_mut::<SelectedPanel>().0 = Some(scene.lineage);
        let open = world.spawn(OpenStored { url: STORE.into() }).id();
        world.trigger(Activate { entity: open });
        world.flush();
        assert_eq!(world.resource::<SelectedPanel>().0, Some(scene.image));
        assert!(world.resource::<Messages<DatasetRequest>>().is_empty());

        // One in no frame is opened in a new one.
        let fresh = world
            .spawn(OpenStored {
                url: "s3://bucket/new.zarr/".into(),
            })
            .id();
        world.trigger(Activate { entity: fresh });
        world.flush();
        assert_eq!(world.resource::<Messages<DatasetRequest>>().len(), 1);
    }

    #[test]
    fn an_address_is_the_same_with_or_without_its_last_slash() {
        assert!(same_address("s3://bucket/aligned/", " s3://bucket/aligned"));
        assert!(!same_address("s3://bucket/aligned/", "s3://bucket/align"));
    }
}
