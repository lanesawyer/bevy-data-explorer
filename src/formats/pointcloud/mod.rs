//! Streaming the Scatterbrain octree into the points panel.
//!
//! Node selection mirrors the image tiles: walk down from the root, keep any
//! node whose region is on screen, and descend while that region is still
//! large enough in screen terms to be worth more detail. Because the format is
//! additive — a node holds its own subsample and its children add more — every
//! node visited is drawn, so zooming in genuinely increases point density
//! rather than swapping one level for another.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::app::net::{Fetching, fetching};
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::Mesh;
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::formats::scatterbrain::nodes::{
    NodeOutcome, NodePoints, PICK_PX, build_mesh, load_node, pick_reach,
};
use crate::formats::scatterbrain::{Rect, Scatterbrain, Slide};
use crate::render::points::{PointMaterial, SourceHighlight};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::properties::{CellProperties, CellSelection};
use crate::source::{self, DataSource, SourceExtent, SourceStatus};
use crate::view::ShowsSource;

/// Descend into a node's children while its region covers at least this many
/// screen pixels. Lower values load deeper, denser detail sooner.
const SUBDIVIDE_PX: f32 = 420.0;

/// Ceiling on points held on the GPU. Reached only when zoomed into a dense
/// region; nodes beyond it are simply not requested.
/// Maximum points held on the GPU.
///
/// Each point is a quad so it can be given a size: four vertices of position,
/// packed colour and corner, or [`crate::render::points::BYTES_PER_POINT`].
pub const DEFAULT_POINT_BUDGET: usize = 3_000_000;

const MAX_IN_FLIGHT: usize = 12;

#[derive(Component)]
/// Marks a spawned point-cloud node. Which node it is lives in the
/// streamer's slot map.
pub struct PointNode;

enum Slot {
    Loading(Fetching<NodeOutcome>),
    Ready {
        entity: Entity,
        points: usize,
        resident: NodePoints,
    },
    /// Loaded, and the filters left nothing of it. Nothing is spawned for a
    /// node like this: see the note where it is built.
    Empty,
    Failed,
}

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
    slots: HashMap<usize, Slot>,
    wanted: Vec<usize>,
    /// Nodes of the selection being replaced, kept on screen until the new one
    /// is built. Empty except while a swap is pending.
    retiring: Vec<Entity>,
    pub in_flight: usize,
    /// Reads given up on because the view moved off them, counted for the
    /// status line: it is the number that says whether panning is costing
    /// anything.
    pub cancelled: usize,
    pub resident_points: usize,
    pub budget: usize,
    pub deepest: usize,
}

impl PointStreamer {
    pub fn new(cloud: Arc<Scatterbrain>, source: Entity) -> Self {
        PointStreamer {
            source,
            selection: CellSelection::default(),
            cloud,
            slide: 0,
            slots: HashMap::new(),
            wanted: Vec::new(),
            retiring: Vec::new(),
            in_flight: 0,
            cancelled: 0,
            resident_points: 0,
            budget: DEFAULT_POINT_BUDGET,
            deepest: 0,
        }
    }

    /// Start every node again, keeping what is on screen until the new set is
    /// built.
    ///
    /// Colouring and filtering decide what a node's vertices are, and the raw
    /// columns are not kept once a node is built, so changing either means
    /// loading them afresh. Despawning them here is what made the cloud blink
    /// away for as long as that took; instead they are handed to
    /// [`Self::reveal`], which drops them only once their replacements are all
    /// resident.
    pub fn retire(&mut self, commands: &mut Commands) {
        // A second change while a swap is pending: what is in the slots this
        // time has never been shown and is already out of date, while the nodes
        // retired earlier are still the last complete picture there was.
        let pending = self.swapping();
        for slot in self.slots.values() {
            if let Slot::Ready { entity, .. } = slot {
                if pending {
                    commands.entity(*entity).despawn();
                } else {
                    self.retiring.push(*entity);
                }
            }
        }
        self.slots.clear();
        self.in_flight = 0;
        self.resident_points = 0;
    }

    /// Whether nodes are being held on screen while their replacements load.
    pub fn swapping(&self) -> bool {
        !self.retiring.is_empty()
    }

    /// Whether everything the current selection asked for has been built.
    ///
    /// Failed nodes count as done: a node that cannot be read is not going to
    /// arrive, and waiting on it would hold the previous selection on screen
    /// for good.
    fn generation_ready(&self) -> bool {
        self.in_flight == 0
            && self.wanted.iter().all(|index| {
                matches!(
                    self.slots.get(index),
                    Some(Slot::Ready { .. } | Slot::Empty | Slot::Failed)
                )
            })
    }

    /// Show the new selection and drop the one it replaces.
    fn reveal(&mut self, commands: &mut Commands) {
        for entity in self.retiring.drain(..) {
            commands.entity(entity).despawn();
        }
        for slot in self.slots.values() {
            if let Slot::Ready { entity, .. } = slot {
                commands.entity(*entity).insert(Visibility::Inherited);
            }
        }
    }

    pub fn cloud(&self) -> &Arc<Scatterbrain> {
        &self.cloud
    }

    fn slide(&self) -> &Slide {
        &self.cloud.slides[self.slide]
    }

    pub fn loaded_nodes(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s, Slot::Ready { .. } | Slot::Empty))
            .count()
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
        for (index, slot) in &self.slots {
            let Slot::Ready { resident, .. } = slot else {
                continue;
            };
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
    let mut budget = streamer.budget;
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
    streamer.wanted = wanted;
}

/// Fetch the coordinates and colour column for nodes that are not loaded yet.
pub fn spawn_node_tasks(mut streamers: Query<&mut PointStreamer>) {
    for mut streamer in &mut streamers {
        spawn_for(&mut streamer);
    }
}

fn spawn_for(streamer: &mut PointStreamer) {
    let cloud = streamer.cloud.clone();
    let selection = streamer.selection.clone();
    let slide = streamer.slide;
    let wanted = std::mem::take(&mut streamer.wanted);

    // Give up on nodes the view has moved off. Dropping the slot aborts the
    // request behind it, which is the whole reason the reads are asynchronous:
    // a pan across a cloud used to pay for every node it crossed, because a
    // blocking read could only be declined before it started. It also frees a
    // place in the queue below for a node that is wanted.
    let keep: HashSet<usize> = wanted.iter().copied().collect();
    let mut cancelled = 0usize;
    streamer.slots.retain(|index, slot| {
        let loading = matches!(slot, Slot::Loading(_));
        if loading && !keep.contains(index) {
            cancelled += 1;
            return false;
        }
        true
    });
    streamer.in_flight = streamer.in_flight.saturating_sub(cancelled);
    streamer.cancelled += cancelled;

    for &index in &wanted {
        if streamer.in_flight >= MAX_IN_FLIGHT {
            break;
        }
        if streamer.slots.contains_key(&index) {
            continue;
        }

        let cloud = cloud.clone();
        let selection = selection.clone();
        let task = fetching(async move {
            let node = &cloud.slides[slide].nodes[index];
            match load_node(&cloud, node, &selection).await {
                Ok((positions, categories)) => NodeOutcome::Ready(positions, categories),
                Err(e) => NodeOutcome::Failed(e),
            }
        });
        streamer.slots.insert(index, Slot::Loading(task));
        streamer.in_flight += 1;
    }
    streamer.wanted = wanted;
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
        collect_for(
            &mut commands,
            &mut streamer,
            &sources,
            &mut meshes,
            &mut materials,
        );
    }
}

fn collect_for(
    commands: &mut Commands,
    streamer: &mut PointStreamer,
    sources: &Query<&DataSource>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PointMaterial>,
) {
    let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
        return;
    };
    let mut finished = Vec::new();
    for (index, slot) in &mut streamer.slots {
        let Slot::Loading(task) = slot else { continue };
        if let Some(outcome) = task.take() {
            finished.push((*index, outcome));
        }
    }

    for (index, outcome) in finished {
        streamer.in_flight = streamer.in_flight.saturating_sub(1);
        let slot = match outcome {
            // A filter can leave a node with nothing in it. Spawning it anyway
            // costs an entity and a draw call to draw no points, and Bevy's mesh
            // allocator skips allocating a zero-length vertex buffer while still
            // copying into it, which it reports as a use-after-free for as long
            // as the node stays resident.
            NodeOutcome::Ready(positions, _) if positions.is_empty() => Slot::Empty,
            NodeOutcome::Ready(positions, categories) => {
                let count = positions.len();
                let mesh = build_mesh(&positions, &categories);
                let entity = commands
                    .spawn((
                        Mesh2d(meshes.add(mesh)),
                        MeshMaterial2d(materials.add(PointMaterial::default())),
                        Transform::default(),
                        RenderLayers::layer(layer),
                        PointNode,
                        // Held back while the selection it replaces is still on
                        // screen: showing each node as it arrived would draw
                        // the new picture half-built over the old one.
                        if streamer.swapping() {
                            Visibility::Hidden
                        } else {
                            Visibility::Inherited
                        },
                    ))
                    .id();
                streamer.resident_points += count;
                Slot::Ready {
                    entity,
                    points: count,
                    resident: NodePoints {
                        positions,
                        categories,
                    },
                }
            }
            NodeOutcome::Failed(e) => {
                warn!("point node {}: {e}", streamer.slide().nodes[index].name);
                Slot::Failed
            }
        };
        streamer.slots.insert(index, slot);
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
        if !streamer.swapping() || !streamer.generation_ready() {
            continue;
        }
        streamer.reveal(&mut commands);
    }
}

/// Drop nodes that are no longer wanted once the budget is exceeded.
pub fn evict_nodes(mut commands: Commands, mut streamers: Query<&mut PointStreamer>) {
    for mut streamer in &mut streamers {
        evict_for(&mut commands, &mut streamer);
    }
}

fn evict_for(commands: &mut Commands, streamer: &mut PointStreamer) {
    let wanted: HashSet<usize> = streamer.wanted.iter().copied().collect();
    if wanted.is_empty() || streamer.resident_points <= streamer.budget {
        return;
    }

    // Shallow nodes are cheap to keep and are needed at every zoom level, so
    // discard the deepest unwanted nodes first.
    let cloud = streamer.cloud.clone();
    let slide = streamer.slide;
    let mut candidates: Vec<(usize, usize, usize)> = streamer
        .slots
        .iter()
        .filter(|(index, _)| !wanted.contains(*index))
        .filter_map(|(index, slot)| match slot {
            Slot::Ready { points, .. } => {
                Some((cloud.slides[slide].nodes[*index].depth, *points, *index))
            }
            _ => None,
        })
        .collect();
    candidates.sort_unstable_by_key(|candidate| std::cmp::Reverse(candidate.0));

    for (_, points, index) in candidates {
        if streamer.resident_points <= streamer.budget {
            break;
        }
        if let Some(Slot::Ready { entity, .. }) = streamer.slots.remove(&index) {
            commands.entity(entity).despawn();
            streamer.resident_points = streamer.resident_points.saturating_sub(points);
        }
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
            streamer.retire(&mut commands);
        }
    }
}

/// Streams a single Scatterbrain point cloud.
pub struct PointCloudPlugin {
    /// Shown in the overlay and in listings. Passed in because a dataset's own
    /// metadata does not name itself, and two clouds are open at once.
    pub name: String,
    pub cloud: Arc<Scatterbrain>,
    pub budget: usize,
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

impl Plugin for PointCloudPlugin {
    /// Each cloud is its own instance of this plugin, so Bevy must not treat a
    /// second one as a duplicate.
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<PointCloudSystems>() {
            app.add_plugins(PointCloudSystems);
        }
        spawn_source(
            app.world_mut(),
            self.name.clone(),
            self.cloud.clone(),
            self.budget,
        );
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
    streamer.budget = budget;
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

fn report_status(streamers: Query<&PointStreamer>, mut sources: Query<&mut SourceStatus>) {
    for streamer in &streamers {
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

    let given_up = if streamer.cancelled > 0 {
        format!(", {} cancelled", streamer.cancelled)
    } else {
        String::new()
    };

    status.0 = format!(
        "{} points in {} octree nodes, depth {}\n\
         showing depth {}, {} nodes loaded, {} loading{given_up}\n\
         {} / {} points resident ({} MB)\n\
         colour by  {}",
        cloud.total_points(),
        cloud.node_count(),
        cloud.max_depth(),
        streamer.deepest,
        streamer.loaded_nodes(),
        streamer.in_flight,
        streamer.resident_points,
        streamer.budget,
        crate::render::points::budget_megabytes(streamer.resident_points),
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

    /// A streamer with `nodes` asked for and the first `built` of them resident.
    fn staged(asked: &[usize], built: usize) -> PointStreamer {
        let mut streamer = PointStreamer::new(Arc::new(cloud()), Entity::PLACEHOLDER);
        streamer.wanted = asked.to_vec();
        for index in &asked[..built] {
            streamer.slots.insert(
                *index,
                Slot::Ready {
                    entity: Entity::PLACEHOLDER,
                    points: 1,
                    resident: NodePoints {
                        positions: vec![[0.0, 0.0]],
                        categories: Vec::new(),
                    },
                },
            );
        }
        streamer
    }

    #[test]
    fn a_selection_is_shown_only_once_every_node_it_asked_for_is_built() {
        // The whole point of holding the previous nodes on screen: revealing a
        // half-built set is the flash, drawn over the old picture instead of
        // replacing it.
        let mut streamer = staged(&[0, 1, 2], 2);
        assert!(!streamer.generation_ready(), "one node is still missing");

        streamer = staged(&[0, 1, 2], 3);
        assert!(streamer.generation_ready());
    }

    #[test]
    fn a_node_still_loading_holds_the_swap() {
        let mut streamer = staged(&[0, 1], 2);
        streamer.in_flight = 1;
        assert!(!streamer.generation_ready());
    }

    #[test]
    fn a_node_that_cannot_be_read_does_not_hold_the_swap_for_good() {
        // Waiting on a node that will never arrive would leave the previous
        // selection on screen with no way back.
        let mut streamer = staged(&[0, 1], 1);
        streamer.slots.insert(1, Slot::Failed);
        assert!(streamer.generation_ready());
    }

    #[test]
    fn a_node_the_filter_emptied_counts_as_arrived() {
        // Nothing is spawned for it, so the swap has nothing to wait for.
        let mut streamer = staged(&[0, 1], 1);
        streamer.slots.insert(1, Slot::Empty);
        assert!(streamer.generation_ready());
        assert_eq!(streamer.loaded_nodes(), 2);
    }

    #[test]
    fn a_selection_that_asks_for_nothing_is_ready_at_once() {
        // Panned off the data there is nothing to wait for, and nothing to show.
        assert!(staged(&[], 0).generation_ready());
    }

    #[test]
    fn nothing_is_held_back_when_there_was_nothing_on_screen() {
        // The first selection has no previous picture to protect, so its nodes
        // are drawn as they arrive rather than waiting for the whole set.
        assert!(!staged(&[0, 1], 1).swapping());
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
        streamer.slots.insert(
            0,
            Slot::Ready {
                entity: Entity::PLACEHOLDER,
                points: points.len(),
                resident: NodePoints {
                    positions: points.to_vec(),
                    categories: categories.to_vec(),
                },
            },
        );
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

    #[test]
    fn a_mesh_without_categories_still_builds() {
        let mesh = build_mesh(&[[0.0, 0.0], [1.0, 1.0]], &[]);
        // Four vertices per point: each is drawn as a quad so that it can be
        // given a size.
        assert_eq!(mesh.count_vertices(), 8);
        assert!(
            mesh.attribute(crate::render::points::ATTRIBUTE_POINT_COLOR)
                .is_some()
        );
    }

    #[test]
    fn meshes_flip_y_to_match_the_image_panel() {
        let mesh = build_mesh(&[[2.0, 3.0]], &[]);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x3(values) = positions else {
            panic!("unexpected position format");
        };
        assert_eq!(values[0], [2.0, -3.0, 0.0]);
    }
}
