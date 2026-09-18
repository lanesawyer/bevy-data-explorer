//! Streaming the Scatterbrain octree into the points panel.
//!
//! Node selection mirrors the image tiles: walk down from the root, keep any
//! node whose region is on screen, and descend while that region is still
//! large enough in screen terms to be worth more detail. Because the format is
//! additive — a node holds its own subsample and its children add more — every
//! node visited is drawn, so zooming in genuinely increases point density
//! rather than swapping one level for another.

use std::collections::HashSet;
use std::sync::Arc;

use crate::app::net::fetching;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::formats::scatterbrain::nodes::{NodeCache, NodeOutcome, PICK_PX, load_node, pick_reach};
use crate::formats::scatterbrain::{Rect, Scatterbrain, Slide};
use crate::render::points::{PointMaterial, SourceHighlight};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::properties::{CellProperties, CellSelection};
use crate::source::{self, DataSource, SourceBusy, SourceExtent, SourceStatus};
use crate::view::ShowsSource;

/// Descend into a node's children while its region covers at least this many
/// screen pixels. Lower values load deeper, denser detail sooner.
const SUBDIVIDE_PX: f32 = 420.0;

/// Maximum points held on the GPU. Reached only when zoomed into a dense
/// region; nodes beyond it are simply not requested.
///
/// Each point is a quad so it can be given a size: four vertices of position,
/// packed colour and corner, or [`crate::render::points::BYTES_PER_POINT`].
pub const DEFAULT_POINT_BUDGET: usize = 3_000_000;

#[derive(Component)]
/// Marks a spawned point-cloud node. Which node it is lives in the
/// streamer's slot map.
pub struct PointNode;

/// A point found under the pointer.
pub struct Hit {
    /// Index of the octree node holding it.
    pub node: usize,
    /// Its position within that node's column files, which together with the
    /// node name is the only identifier this format gives a point.
    pub index: usize,
    pub position: Vec2,
    /// Its value in the column the cloud is coloured by, if any.
    pub category: Option<u16>,
}

/// Streams one point cloud.
///
/// A component on the source entity rather than a resource, so two clouds can
/// be open at once — each frame's streamer is found through the source it is
/// bound to.
#[derive(Component)]
pub struct PointStreamer {
    /// The source entity this streamer serves.
    pub source: Entity,
    cloud: Arc<Scatterbrain>,
    /// What to colour by and what to filter out, mirrored from the source's
    /// properties so that workers can be handed a copy.
    pub selection: CellSelection,
    /// Which slide this panel draws. Single-cloud datasets have only one.
    pub slide: usize,
    /// Keyed by node index within the slide.
    nodes: NodeCache<usize>,
    pub deepest: usize,
}

impl PointStreamer {
    pub fn new(cloud: Arc<Scatterbrain>, source: Entity) -> Self {
        PointStreamer {
            source,
            selection: CellSelection::default(),
            cloud,
            slide: 0,
            nodes: NodeCache::new(DEFAULT_POINT_BUDGET),
            deepest: 0,
        }
    }

    pub fn cloud(&self) -> &Arc<Scatterbrain> {
        &self.cloud
    }

    fn slide(&self) -> &Slide {
        &self.cloud.slides[self.slide]
    }

    /// The resident point nearest the probe, within a few pixels of it.
    ///
    /// Only nodes whose bounds reach the pointer are searched, which is the
    /// path from the root down to the deepest node covering it rather than
    /// every point on screen.
    pub fn pick(&self, probe: &HoverProbe) -> Option<Hit> {
        // Display space negates y; stored positions and node bounds are both in
        // the dataset's own coordinates.
        let target = Vec2::new(probe.world.x, -probe.world.y);
        let radius = probe.radius(PICK_PX);
        let reach = pick_reach(target, radius);
        let limit = radius * radius;
        let nodes = &self.cloud.slides[self.slide].nodes;

        let mut best: Option<(f32, Hit)> = None;
        for (index, _, resident) in self.nodes.ready() {
            if !nodes[*index].bounds.intersects(&reach) {
                continue;
            }
            let Some((distance, offset, position, category)) = resident.nearest(target, limit)
            else {
                continue;
            };
            if best.as_ref().is_none_or(|(nearest, _)| distance < *nearest) {
                best = Some((
                    distance,
                    Hit {
                        node: *index,
                        index: offset,
                        position,
                        category,
                    },
                ));
            }
        }
        best.map(|(_, hit)| hit)
    }
}

/// Walk the octree and decide which nodes should be resident.
pub fn select_nodes(
    mut streamers: Query<&mut PointStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    for mut streamer in &mut streamers {
        select_for(&mut streamer, &panels);
    }
}

fn select_for(
    streamer: &mut PointStreamer,
    panels: &Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    let cloud = streamer.cloud.clone();
    let slide = streamer.slide;
    let mut wanted = Vec::new();
    let mut deepest = 0usize;
    let mut budget = streamer.nodes.budget;
    let mut seen = HashSet::new();

    // Every panel of this kind draws the same entities, so the resident set is
    // the union of what each of them needs. A duplicated panel zoomed somewhere
    // else therefore pulls in its own detail.
    let source = streamer.source;
    for (camera, transform, projection, _) in
        panels.iter().filter(|(_, _, _, shows)| shows.0 == source)
    {
        let Projection::Orthographic(ortho) = projection else {
            continue;
        };
        let Some(viewport) = camera.logical_viewport_size() else {
            continue;
        };

        let centre = transform.translation().truncate();
        let half = Vec2::new(ortho.area.width(), ortho.area.height()) * 0.5;
        let view = Rect {
            min_x: centre.x - half.x,
            // World y is negated for display, so the visible band in dataset
            // coordinates is the mirror of the camera's.
            min_y: -(centre.y + half.y),
            max_x: centre.x + half.x,
            max_y: -(centre.y - half.y),
        };
        let units_per_px = ortho.area.width() / viewport.x.max(1.0);

        // Breadth-first so that coarse nodes are requested before fine ones and
        // a usable picture appears while detail is still arriving.
        let mut queue = vec![0usize];
        while let Some(index) = queue.pop() {
            let node = &cloud.slides[slide].nodes[index];
            if !node.bounds.intersects(&view) {
                continue;
            }
            if seen.insert(index) {
                if node.count as usize > budget {
                    seen.remove(&index);
                    continue;
                }
                budget -= node.count as usize;
                wanted.push(index);
                deepest = deepest.max(node.depth);
            }

            let screen_px = node.bounds.width() / units_per_px.max(f32::MIN_POSITIVE);
            if screen_px >= SUBDIVIDE_PX {
                queue.extend(node.children.iter().copied());
            }
        }
    }

    streamer.deepest = deepest;
    streamer.nodes.want(wanted);
}

/// Fetch the coordinates and colour column for nodes that are not loaded yet.
pub fn spawn_node_tasks(mut streamers: Query<&mut PointStreamer>) {
    for mut streamer in &mut streamers {
        let cloud = streamer.cloud.clone();
        let selection = streamer.selection.clone();
        let slide = streamer.slide;
        streamer.nodes.start(|index| {
            let cloud = cloud.clone();
            let selection = selection.clone();
            fetching(async move {
                let node = &cloud.slides[slide].nodes[index];
                match load_node(&cloud, node, &selection).await {
                    Ok((positions, categories)) => NodeOutcome::Ready(positions, categories),
                    Err(e) => NodeOutcome::Failed(e),
                }
            })
        });
    }
}

/// Turn finished fetches into meshes.
pub fn collect_node_tasks(
    mut commands: Commands,
    mut streamers: Query<&mut PointStreamer>,
    sources: Query<&DataSource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PointMaterial>>,
) {
    for mut streamer in &mut streamers {
        let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
            continue;
        };
        // Held back while the selection it replaces is still on screen:
        // showing each node as it arrived would draw the new picture
        // half-built over the old one.
        let visibility = if streamer.nodes.swapping() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        let failed = streamer.nodes.collect(|_, mesh| {
            commands
                .spawn((
                    Mesh2d(meshes.add(mesh)),
                    MeshMaterial2d(materials.add(PointMaterial::default())),
                    Transform::default(),
                    RenderLayers::layer(layer),
                    PointNode,
                    visibility,
                ))
                .id()
        });
        for (index, e) in failed {
            warn!("point node {}: {e}", streamer.slide().nodes[index].name);
        }
    }
}

/// Show a new selection once all of it has arrived, and drop the one it
/// replaces.
///
/// The two sets are on the GPU together for as long as the swap takes, which is
/// what buys the picture staying put. Nothing is revealed early, so a filter
/// either applies to the whole cloud or to none of it.
pub fn swap_generations(mut commands: Commands, mut streamers: Query<&mut PointStreamer>) {
    for mut streamer in &mut streamers {
        // Read before touching it: taking the streamer mutably every frame
        // would mark it changed for everything watching.
        if !streamer.nodes.swapping() || !streamer.nodes.generation_ready() {
            continue;
        }
        streamer.nodes.reveal(&mut commands, |_| true);
    }
}

/// Drop nodes that are no longer wanted once the budget is exceeded.
pub fn evict_nodes(mut commands: Commands, mut streamers: Query<&mut PointStreamer>) {
    for mut streamer in &mut streamers {
        let cloud = streamer.cloud.clone();
        let slide = streamer.slide;
        streamer.nodes.evict(&mut commands, |index| {
            cloud.slides[slide].nodes[*index].depth
        });
    }
}

/// Rebuild when this source's colouring or filters change.
///
/// Colouring and filtering both decide what the vertices are, and the raw
/// columns are not kept after a node is built, so a change means loading those
/// nodes again. The same trade the image panel makes for its channels.
///
/// The properties and the streamer are both components of the source entity, so
/// this is one query and nothing outside this module needs to know the streamer
/// exists.
fn apply_selection(
    mut commands: Commands,
    mut streamers: Query<(&CellProperties, &mut PointStreamer), Changed<CellProperties>>,
) {
    for (properties, mut streamer) in &mut streamers {
        let selection = properties.selection();
        if streamer.selection != selection {
            streamer.selection = selection;
            streamer.nodes.retire(&mut commands);
        }
    }
}

/// The systems every point cloud shares, registered once however many clouds
/// are open.
pub struct PointCloudSystems;

impl Plugin for PointCloudSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                apply_selection,
                select_nodes,
                spawn_node_tasks,
                collect_node_tasks,
                swap_generations,
                evict_nodes,
                report_status,
            )
                .chain()
                .in_set(Stage::Sources),
        )
        // Resolving the pointer reads the nodes that are resident now; the
        // schedule already puts `HoverProbing` after this frame's arrivals and
        // evictions.
        .add_systems(Update, resolve_hover.in_set(source::hover::HoverProbing));
    }
}

/// Register a parsed cloud as a source, and bind a streamer to it.
pub fn spawn_source(
    world: &mut World,
    name: String,
    cloud: Arc<Scatterbrain>,
    budget: usize,
) -> Entity {
    let bounds = cloud.slides[0].tight_bounds;
    let (cx, cy) = bounds.centre();
    let source = source::register_in(
        world,
        source::SourceInfo {
            name,
            unit: cloud.unit.clone(),
            detail: format!("Scatterbrain octree, depth {}", cloud.max_depth()),
            stat: format!("{} CELLS", source::compact_count(cloud.total_points())),
        },
        SourceExtent {
            // World y is negated for display, matching the image panel.
            centre: Vec2::new(cx, -cy),
            size: Vec2::new(bounds.width(), bounds.height()),
            finest: bounds.width() / 100_000.0,
        },
    );

    // Advertising a point size is what puts the size control in the
    // sidebar; sources without one simply do not offer it.
    world.entity_mut(source).insert((
        crate::render::points::SourcePointSize::default(),
        // Both start empty. Carrying them from registration means the hover
        // systems can write through a query rather than through commands,
        // and so can leave them untouched when nothing has changed.
        SourceHighlight::default(),
        HoverInfo::default(),
        // Placeholder until a lookup service supplies the real value
        // labels; the column names and ids are the dataset's own.
        crate::formats::scatterbrain::placeholder_properties(
            &cloud.category_columns(),
            &cloud.numeric_columns(),
        ),
    ));

    let mut streamer = PointStreamer::new(cloud, source);
    streamer.nodes.budget = budget;
    world.entity_mut(source).insert(streamer);
    source
}

/// Answer the pointer: what is under it, and which cells share its value.
///
/// Both outputs are left alone when they have not changed, because the
/// highlight drives a uniform upload per resident node and the tooltip drives a
/// text layout — and the pointer sits still for most of the frames it is over a
/// cloud.
pub fn resolve_hover(
    mut sources: Query<(
        &PointStreamer,
        &DataSource,
        &CellProperties,
        Option<&HoverProbe>,
        &mut HoverInfo,
        &mut SourceHighlight,
    )>,
) {
    for (streamer, source, properties, probe, mut info, mut highlight) in &mut sources {
        let hit = probe.and_then(|probe| streamer.pick(probe));
        let found = hit
            .as_ref()
            .map(|hit| describe(hit, streamer, source, properties));

        let category = hit.as_ref().and_then(|hit| hit.category);
        if highlight.0 != category {
            highlight.0 = category;
        }

        let next = found.unwrap_or_default();
        if *info != next {
            *info = next;
        }
    }
}

/// Name a hit the way the dataset names it.
///
/// Scatterbrain gives a point no identifier of its own: it is the nth row of
/// the columns of one octree node, so the node and that offset is the whole
/// address.
fn describe(
    hit: &Hit,
    streamer: &PointStreamer,
    source: &DataSource,
    properties: &CellProperties,
) -> HoverInfo {
    let node = &streamer.slide().nodes[hit.node];
    let mut info = HoverInfo::titled(format!("{}#{}", node.name, hit.index));

    if let Some(code) = hit.category {
        let (property, label) = properties.colour_label(code);
        info = info.row(property, label);
    }

    info.row(
        "at",
        format!(
            "{:.1}, {:.1} {}",
            hit.position.x, hit.position.y, source.unit
        ),
    )
}

fn report_status(
    streamers: Query<&PointStreamer>,
    mut sources: Query<&mut SourceStatus>,
    mut busy: Query<&mut SourceBusy>,
) {
    for streamer in &streamers {
        if let Ok(mut busy) = busy.get_mut(streamer.source) {
            busy.set_if_neq(SourceBusy(streamer.nodes.busy()));
        }
        report_for(streamer, &mut sources);
    }
}

fn report_for(streamer: &PointStreamer, sources: &mut Query<&mut SourceStatus>) {
    let Ok(mut status) = sources.get_mut(streamer.source) else {
        return;
    };
    let cloud = streamer.cloud();
    let colour = streamer
        .selection
        .colour_by
        .as_ref()
        .and_then(|name| {
            cloud
                .attributes
                .iter()
                .find(|a| &a.name == name)
                .map(|a| a.description.clone())
        })
        .unwrap_or_else(|| "none".into());

    status.0 = format!(
        "{} points in {} octree nodes, depth {}\n\
         showing depth {}, {}\n\
         colour by  {}",
        cloud.total_points(),
        cloud.node_count(),
        cloud.max_depth(),
        streamer.deepest,
        streamer.nodes.status(),
        colour,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> Scatterbrain {
        Scatterbrain::parse(include_str!("../../../testdata/scatterbrain.json")).unwrap()
    }

    fn slide_of(cloud: &Scatterbrain) -> &Slide {
        &cloud.slides[0]
    }

    /// Reproduce the selection walk without a running app.
    fn select(view: Rect, units_per_px: f32, budget: usize, cloud: &Scatterbrain) -> Vec<usize> {
        let slide = slide_of(cloud);
        let mut wanted = Vec::new();
        let mut budget = budget;
        let mut queue = vec![0usize];
        while let Some(index) = queue.pop() {
            let node = &slide.nodes[index];
            if !node.bounds.intersects(&view) || node.count as usize > budget {
                continue;
            }
            budget -= node.count as usize;
            wanted.push(index);
            if node.bounds.width() / units_per_px >= SUBDIVIDE_PX {
                queue.extend(node.children.iter().copied());
            }
        }
        wanted
    }

    #[test]
    fn zooming_in_selects_more_points_not_merely_different_ones() {
        let cloud = cloud();
        let full = slide_of(&cloud).bounds;
        let wide = select(full, full.width() / 1000.0, usize::MAX, &cloud);

        // Zoom into the middle tenth of the cloud.
        let (cx, cy) = full.centre();
        let span = full.width() / 20.0;
        let close = Rect {
            min_x: cx - span,
            min_y: cy - span,
            max_x: cx + span,
            max_y: cy + span,
        };
        let deep = select(close, span * 2.0 / 1000.0, usize::MAX, &cloud);

        let depth_of = |set: &[usize]| {
            set.iter()
                .map(|i| slide_of(&cloud).nodes[*i].depth)
                .max()
                .unwrap()
        };
        assert!(
            depth_of(&deep) > depth_of(&wide),
            "zooming in should reach deeper octree levels"
        );
    }

    #[test]
    fn the_whole_view_starts_from_the_root() {
        let cloud = cloud();
        let wanted = select(
            slide_of(&cloud).bounds,
            slide_of(&cloud).bounds.width() / 1000.0,
            usize::MAX,
            &cloud,
        );
        assert!(wanted.contains(&0), "the root node covers every view");
    }

    #[test]
    fn nodes_outside_the_view_are_skipped() {
        let cloud = cloud();
        let far = Rect {
            min_x: 1000.0,
            min_y: 1000.0,
            max_x: 1001.0,
            max_y: 1001.0,
        };
        assert!(select(far, 0.01, usize::MAX, &cloud).is_empty());
    }

    #[test]
    fn the_budget_caps_how_much_is_requested() {
        let cloud = cloud();
        let wanted = select(slide_of(&cloud).bounds, 0.0001, 200_000, &cloud);
        let total: u64 = wanted
            .iter()
            .map(|i| slide_of(&cloud).nodes[*i].count)
            .sum();
        assert!(total <= 200_000, "selection must respect the point budget");
        assert!(
            !wanted.is_empty(),
            "a small budget should still show something"
        );
    }

    /// A streamer with one node already resident, holding `points`.
    fn resident(points: &[[f32; 2]], categories: &[u16]) -> PointStreamer {
        let mut streamer = PointStreamer::new(Arc::new(cloud()), Entity::PLACEHOLDER);
        streamer
            .nodes
            .landed(0, NodeOutcome::Ready(points.to_vec(), categories.to_vec()));
        streamer
    }

    fn probe_at(world: Vec2, units_per_px: f32) -> HoverProbe {
        HoverProbe {
            panel: Entity::PLACEHOLDER,
            world,
            units_per_px,
        }
    }

    #[test]
    fn hovering_picks_the_nearest_point_and_the_value_it_is_coloured_by() {
        let cloud = cloud();
        let (cx, cy) = slide_of(&cloud).bounds.centre();
        let streamer = resident(&[[cx, cy], [cx + 100.0, cy]], &[3, 9]);

        // Display space negates y, so the probe mirrors the dataset coordinate.
        let hit = streamer
            .pick(&probe_at(Vec2::new(cx + 1.0, -cy), 1.0))
            .expect("the pointer is all but on the first point");
        assert_eq!(hit.index, 0);
        assert_eq!(hit.category, Some(3));
    }

    #[test]
    fn a_probe_that_forgot_to_flip_y_finds_nothing() {
        // The mistake this guards against is silent: the tooltip simply never
        // appears, which looks like picking not working at all.
        let cloud = cloud();
        let bounds = slide_of(&cloud).bounds;
        let (cx, _) = bounds.centre();
        // Well off the axis, so mirroring it lands far outside the pick radius.
        let y = bounds.max_y - bounds.height() * 0.1;
        let streamer = resident(&[[cx, y]], &[0]);

        assert!(streamer.pick(&probe_at(Vec2::new(cx, -y), 1.0)).is_some());
        assert!(streamer.pick(&probe_at(Vec2::new(cx, y), 1.0)).is_none());
    }

    #[test]
    fn the_pointer_has_to_come_close_to_pick() {
        let cloud = cloud();
        let (cx, cy) = slide_of(&cloud).bounds.centre();
        let streamer = resident(&[[cx, cy]], &[0]);
        assert!(
            streamer
                .pick(&probe_at(Vec2::new(cx + 50.0, -cy), 1.0))
                .is_none()
        );
    }

    #[test]
    fn how_far_a_pick_reaches_follows_the_zoom() {
        // Points are a pixel and a half wide whatever the zoom, so the reach
        // has to be measured in pixels or picking would get harder the further
        // out you went.
        let cloud = cloud();
        let (cx, cy) = slide_of(&cloud).bounds.centre();
        let streamer = resident(&[[cx, cy]], &[0]);
        let away = Vec2::new(cx + 50.0, -cy);
        assert!(streamer.pick(&probe_at(away, 1.0)).is_none());
        assert!(streamer.pick(&probe_at(away, 20.0)).is_some());
    }

    #[test]
    fn nodes_the_pointer_is_nowhere_near_are_not_searched() {
        // Every resident point on screen would be far too many to scan each
        // frame, so only the nodes whose regions reach the pointer are.
        let cloud = cloud();
        let bounds = slide_of(&cloud).bounds;
        let outside = [[bounds.max_x + 1000.0, bounds.max_y + 1000.0]];
        let streamer = resident(&outside, &[0]);
        let probe = probe_at(Vec2::new(outside[0][0], -outside[0][1]), 1.0);
        assert!(
            streamer.pick(&probe).is_none(),
            "a point outside its own node's bounds should never be reached"
        );
    }

    #[test]
    fn an_uncoloured_cloud_still_identifies_what_is_under_the_pointer() {
        let cloud = cloud();
        let (cx, cy) = slide_of(&cloud).bounds.centre();
        let streamer = resident(&[[cx, cy]], &[]);
        let hit = streamer.pick(&probe_at(Vec2::new(cx, -cy), 1.0)).unwrap();
        // Nothing to highlight, but the point still has an address.
        assert_eq!(hit.category, None);
    }

    #[test]
    fn a_hit_is_named_by_its_node_and_its_offset_within_it() {
        use crate::source::properties::{
            CellProperties, CellProperty, PropertyKind, PropertyValue,
        };

        let cloud = cloud();
        let (cx, cy) = slide_of(&cloud).bounds.centre();
        let streamer = resident(&[[cx, cy], [cx + 0.5, cy]], &[0, 2]);
        let hit = streamer
            .pick(&probe_at(Vec2::new(cx + 0.5, -cy), 1.0))
            .unwrap();

        let source = DataSource {
            name: "Cells".into(),
            unit: "um".into(),
            detail: String::new(),
            stat: String::new(),
            layer: 1,
        };
        let properties = CellProperties::ready(vec![CellProperty {
            id: "class".into(),
            name: "Class".into(),
            shown: true,
            kind: PropertyKind::Categorical(vec![PropertyValue {
                code: 2,
                label: "L2/3 IT".into(),
                selected: false,
            }]),
        }]);

        let info = describe(&hit, &streamer, &source, &properties);
        // Scatterbrain gives a point no id of its own: it is the nth row of one
        // node's columns, so that pair is the whole address.
        let node = &streamer.slide().nodes[hit.node];
        assert_eq!(info.title, format!("{}#1", node.name));
        assert_eq!(
            info.rows[0],
            ("Class".to_string(), "L2/3 IT".to_string()),
            "the tooltip should name the value, not its code"
        );
        assert!(info.rows[1].1.ends_with("um"));
    }
}
