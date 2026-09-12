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

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::datasource::{self, SourceExtent, SourceStatus};
use crate::panel::ShowsSource;
use crate::scatterbrain::{self, Node, Rect, Scatterbrain, Slide};

/// Descend into a node's children while its region covers at least this many
/// screen pixels. Lower values load deeper, denser detail sooner.
const SUBDIVIDE_PX: f32 = 420.0;

/// Ceiling on points held on the GPU. Reached only when zoomed into a dense
/// region; nodes beyond it are simply not requested.
pub const DEFAULT_POINT_BUDGET: usize = 4_000_000;

const MAX_IN_FLIGHT: usize = 12;

#[derive(Component)]
/// Marks a spawned point-cloud node. Which node it is lives in the
/// streamer's slot map.
pub struct PointNode;

enum Slot {
    Loading(Task<NodeOutcome>),
    Ready { entity: Entity, points: usize },
    Failed,
}

pub enum NodeOutcome {
    Ready(Vec<[f32; 2]>, Vec<u16>),
    Failed(String),
}

#[derive(Resource)]
pub struct PointStreamer {
    /// The source entity this streamer serves.
    pub source: Entity,
    cloud: Arc<Scatterbrain>,
    /// Column used to colour points, and its palette.
    pub colour_column: Option<String>,
    /// Which slide this panel draws. Single-cloud datasets have only one.
    pub slide: usize,
    slots: HashMap<usize, Slot>,
    wanted: Vec<usize>,
    pub in_flight: usize,
    pub resident_points: usize,
    pub budget: usize,
    pub deepest: usize,
}

impl PointStreamer {
    pub fn new(cloud: Arc<Scatterbrain>, source: Entity) -> Self {
        let colour_column = cloud.category_columns().first().map(|c| c.name.clone());
        PointStreamer {
            source,
            cloud,
            colour_column,
            slide: 0,
            slots: HashMap::new(),
            wanted: Vec::new(),
            in_flight: 0,
            resident_points: 0,
            budget: DEFAULT_POINT_BUDGET,
            deepest: 0,
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
            .filter(|s| matches!(s, Slot::Ready { .. }))
            .count()
    }
}

/// Walk the octree and decide which nodes should be resident.
pub fn select_nodes(
    mut streamer: ResMut<PointStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
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
pub fn spawn_node_tasks(mut streamer: ResMut<PointStreamer>) {
    let pool = AsyncComputeTaskPool::get();
    let cloud = streamer.cloud.clone();
    let colour_column = streamer.colour_column.clone();
    let slide = streamer.slide;
    let wanted = std::mem::take(&mut streamer.wanted);

    for &index in &wanted {
        if streamer.in_flight >= MAX_IN_FLIGHT {
            break;
        }
        if streamer.slots.contains_key(&index) {
            continue;
        }

        let cloud = cloud.clone();
        let colour_column = colour_column.clone();
        let task = pool.spawn(async move {
            let node = &cloud.slides[slide].nodes[index];
            match load_node(&cloud, node, colour_column.as_deref()) {
                Ok((positions, categories)) => NodeOutcome::Ready(positions, categories),
                Err(e) => NodeOutcome::Failed(e),
            }
        });
        streamer.slots.insert(index, Slot::Loading(task));
        streamer.in_flight += 1;
    }
    streamer.wanted = wanted;
}

pub fn load_node(
    cloud: &Scatterbrain,
    node: &Node,
    colour_column: Option<&str>,
) -> Result<(Vec<[f32; 2]>, Vec<u16>), String> {
    let positions =
        scatterbrain::decode_positions(&fetch(&cloud.positions_url(node))?, node.count)?;

    let categories = match colour_column {
        Some(column) => {
            let bytes = fetch(&cloud.column_url(column, node))?;
            scatterbrain::decode_categories(&bytes, node.count)?
        }
        None => Vec::new(),
    };
    Ok((positions, categories))
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::blocking::get(url)
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("fetching {url}: {e}"))?;
    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| format!("reading {url}: {e}"))
}

/// Turn finished fetches into meshes.
pub fn collect_node_tasks(
    mut commands: Commands,
    mut streamer: ResMut<PointStreamer>,
    sources: Query<&datasource::DataSource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
        return;
    };
    let mut finished = Vec::new();
    for (index, slot) in streamer.slots.iter_mut() {
        let Slot::Loading(task) = slot else { continue };
        if let Some(outcome) = block_on(poll_once(task)) {
            finished.push((*index, outcome));
        }
    }

    for (index, outcome) in finished {
        streamer.in_flight = streamer.in_flight.saturating_sub(1);
        let slot = match outcome {
            NodeOutcome::Ready(positions, categories) => {
                let count = positions.len();
                let mesh = build_mesh(&positions, &categories);
                let entity = commands
                    .spawn((
                        Mesh2d(meshes.add(mesh)),
                        MeshMaterial2d(materials.add(ColorMaterial::default())),
                        Transform::default(),
                        RenderLayers::layer(layer),
                        PointNode,
                    ))
                    .id();
                streamer.resident_points += count;
                Slot::Ready {
                    entity,
                    points: count,
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

/// Build a point-list mesh, colouring each point by its category.
///
/// One vertex per point keeps a multi-million point cloud affordable; the
/// trade-off is that the hardware draws each as a single pixel, so there is no
/// point-size control without a custom shader.
pub fn build_mesh(positions: &[[f32; 2]], categories: &[u16]) -> Mesh {
    let vertices: Vec<[f32; 3]> = positions
        // Negate y so the cloud shares the image panel's top-down convention.
        .iter()
        .map(|p| [p[0], -p[1], 0.0])
        .collect();

    let colours: Vec<[f32; 4]> = if categories.len() == positions.len() {
        categories.iter().map(|c| category_colour(*c)).collect()
    } else {
        vec![[0.8, 0.85, 0.9, 1.0]; positions.len()]
    };

    let mut mesh = Mesh::new(
        PrimitiveTopology::PointList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh
}

/// A repeating categorical palette. Categories here are label indices with no
/// inherent order, so hues are spread by a golden-ratio step to keep
/// neighbouring indices visually distinct.
pub fn category_colour(category: u16) -> [f32; 4] {
    let hue = (category as f32 * 137.507_76) % 360.0;
    let colour = Color::hsl(hue, 0.72, 0.62).to_linear();
    [colour.red, colour.green, colour.blue, 1.0]
}

/// Drop nodes that are no longer wanted once the budget is exceeded.
pub fn evict_nodes(mut commands: Commands, mut streamer: ResMut<PointStreamer>) {
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
    candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0));

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

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> Scatterbrain {
        Scatterbrain::parse(include_str!("../testdata/scatterbrain.json")).unwrap()
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

    #[test]
    fn categories_get_distinguishable_colours() {
        // Adjacent label indices are unrelated, so they must not look alike.
        let a = category_colour(0);
        let b = category_colour(1);
        let distance: f32 = (0..3).map(|i| (a[i] - b[i]).abs()).sum();
        assert!(distance > 0.2, "neighbouring categories look too similar");
        assert_eq!(a[3], 1.0);
    }

    #[test]
    fn a_mesh_without_categories_still_builds() {
        let mesh = build_mesh(&[[0.0, 0.0], [1.0, 1.0]], &[]);
        assert_eq!(mesh.count_vertices(), 2);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
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

/// Streams a single Scatterbrain point cloud.
pub struct PointCloudPlugin {
    pub cloud: Arc<Scatterbrain>,
    pub budget: usize,
}

impl Plugin for PointCloudPlugin {
    fn build(&self, app: &mut App) {
        let bounds = self.cloud.slides[0].tight_bounds;
        let (cx, cy) = bounds.centre();
        let source = datasource::register(
            app,
            "Point cloud",
            self.cloud.unit.clone(),
            SourceExtent {
                // World y is negated for display, matching the image panel.
                centre: Vec2::new(cx, -cy),
                size: Vec2::new(bounds.width(), bounds.height()),
                finest: bounds.width() / 100_000.0,
            },
        );

        let mut streamer = PointStreamer::new(self.cloud.clone(), source);
        streamer.budget = self.budget;

        app.insert_resource(streamer).add_systems(
            Update,
            (
                select_nodes,
                spawn_node_tasks,
                collect_node_tasks,
                evict_nodes,
                report_status,
            )
                .chain()
                .after(crate::panel::update_viewports),
        );
    }
}

fn report_status(streamer: Res<PointStreamer>, mut sources: Query<&mut SourceStatus>) {
    let Ok(mut status) = sources.get_mut(streamer.source) else {
        return;
    };
    let cloud = streamer.cloud();
    let colour = streamer
        .colour_column
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
         showing depth {}, {} nodes loaded, {} loading\n\
         {} / {} points resident\n\
         colour by  {}",
        cloud.total_points(),
        cloud.node_count(),
        cloud.max_depth(),
        streamer.deepest,
        streamer.loaded_nodes(),
        streamer.in_flight,
        streamer.resident_points,
        streamer.budget,
        colour,
    );
}
