//! The sectioned point-cloud panel.
//!
//! A sectioned Scatterbrain dataset is many octrees — one per physical slice of
//! the specimen — that all share a coordinate system, so drawn raw they would
//! pile up on top of each other. This panel gives each slide a layout offset
//! instead, and offers two ways of arranging them: every slice at once in a
//! grid, or one at a time to scroll through.
//!
//! The offset lives on each node's transform, so switching modes only rewrites
//! transforms and visibility. Nothing is refetched.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::cellproperties::CellSelection;
use crate::datasource::{self, SourceExtent, SourceStatus};
use crate::panel::{ShowsSource, ViewLimits};
use crate::pointcloud::{NodeOutcome, build_mesh, load_node};
use crate::points_render::PointMaterial;
use crate::scatterbrain::{Rect, Scatterbrain};

/// Descend into a slide's octree while its region covers at least this many
/// screen pixels.
const SUBDIVIDE_PX: f32 = 420.0;

/// Gap between grid cells, as a fraction of the cell size.
const CELL_PADDING: f32 = 0.06;

const MAX_IN_FLIGHT: usize = 12;

/// Maximum points held on the GPU.
///
/// The grid shows every slice at once, and the root subsamples alone come to
/// about three million points on the reference dataset. A budget below that
/// does not thin the grid evenly — it starves whole slices at the end of the
/// list — so it has to clear the sum of the roots with room to spare.
pub const DEFAULT_SLICE_BUDGET: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliceMode {
    /// Every slice laid out in a grid.
    Grid,
    /// One slice at a time.
    Single,
}

/// Identifies a node within the dataset: which slide, and which node of it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SliceNode {
    pub slide: usize,
    pub node: usize,
}

#[derive(Component)]
pub struct SliceNodeTag(pub SliceNode);

enum Slot {
    Loading(Task<NodeOutcome>),
    Ready { entity: Entity, points: usize },
    Failed,
}

#[derive(Resource)]
pub struct SliceStreamer {
    /// The source entity this streamer serves.
    pub source: Entity,
    cloud: Arc<Scatterbrain>,
    /// What to colour by and what to filter out, mirrored from the source's
    /// properties so that workers can be handed a copy.
    pub selection: CellSelection,
    pub mode: SliceMode,
    /// Slice shown in [`SliceMode::Single`].
    pub current: usize,
    /// Where each slide sits, by mode.
    layout: Layout,
    slots: HashMap<SliceNode, Slot>,
    wanted: Vec<SliceNode>,
    pub in_flight: usize,
    pub resident_points: usize,
    pub budget: usize,
    /// Set when the mode or slice changed and the camera should refit.
    pub refit: bool,
}

/// Precomputed placement for every slide in both modes.
struct Layout {
    grid: Vec<Vec2>,
    columns: usize,
    cell: Vec2,
    grid_extent: Rect,
}

impl Layout {
    /// Lay the slides out in a roughly square grid of uniform cells.
    ///
    /// Slices vary in size, so each is centred in a cell sized to the largest
    /// of them. Packing each slide's own extent instead would misalign the
    /// anatomy from one row to the next.
    fn build(cloud: &Scatterbrain) -> Self {
        let count = cloud.slides.len().max(1);
        let columns = (count as f32).sqrt().ceil() as usize;
        let rows = count.div_ceil(columns);

        let (w, h) = cloud.max_slide_extent();
        let cell = Vec2::new(w * (1.0 + CELL_PADDING), h * (1.0 + CELL_PADDING));

        let grid = (0..count)
            .map(|i| {
                let col = i % columns;
                let row = i / columns;
                // Rows run downward in display space.
                Vec2::new(col as f32 * cell.x, -(row as f32) * cell.y)
            })
            .collect();

        let grid_extent = Rect {
            min_x: -cell.x * 0.5,
            min_y: -cell.y * (rows as f32 - 0.5),
            max_x: cell.x * (columns as f32 - 0.5),
            max_y: cell.y * 0.5,
        };

        Layout {
            grid,
            columns,
            cell,
            grid_extent,
        }
    }
}

impl SliceStreamer {
    pub fn new(cloud: Arc<Scatterbrain>, source: Entity) -> Self {
        let layout = Layout::build(&cloud);
        SliceStreamer {
            source,
            selection: CellSelection::default(),
            cloud,
            mode: SliceMode::Grid,
            current: 0,
            layout,
            slots: HashMap::new(),
            wanted: Vec::new(),
            in_flight: 0,
            resident_points: 0,
            budget: DEFAULT_SLICE_BUDGET,
            refit: true,
        }
    }

    /// Drop every resident node so they are built again.
    ///
    /// Colouring and filtering decide what a node's vertices are, and the raw
    /// columns are not kept once a node is built, so changing either means
    /// loading them afresh.
    pub fn reset(&mut self, commands: &mut Commands) {
        for slot in self.slots.values() {
            if let Slot::Ready { entity, .. } = slot {
                commands.entity(*entity).despawn();
            }
        }
        self.slots.clear();
        self.in_flight = 0;
        self.resident_points = 0;
    }

    pub fn cloud(&self) -> &Arc<Scatterbrain> {
        &self.cloud
    }

    pub fn columns(&self) -> usize {
        self.layout.columns
    }

    pub fn loaded_nodes(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s, Slot::Ready { .. }))
            .count()
    }

    /// Translation applied to a slide's points in the current mode.
    ///
    /// Each slide is re-centred on its own tight bounds first, so slices of
    /// different sizes line up rather than drifting with their raw coordinates.
    pub fn offset(&self, slide: usize) -> Vec2 {
        let bounds = self.cloud.slides[slide].tight_bounds;
        let (cx, cy) = bounds.centre();
        // World y is negated for display, matching the other panels.
        let centred = Vec2::new(-cx, cy);
        match self.mode {
            SliceMode::Grid => centred + self.layout.grid[slide],
            SliceMode::Single => centred,
        }
    }

    /// Whether a slide is drawn in the current mode.
    pub fn visible(&self, slide: usize) -> bool {
        match self.mode {
            SliceMode::Grid => true,
            SliceMode::Single => slide == self.current,
        }
    }

    /// View limits that frame whatever the current mode shows.
    /// How much world the current mode puts on screen.
    ///
    /// This is what a frame opening onto the sections should be framed to, and
    /// it changes with the mode: the grid spans every slice at once, a single
    /// slice spans one cell.
    pub fn extent(&self) -> SourceExtent {
        match self.mode {
            SliceMode::Grid => {
                let e = self.layout.grid_extent;
                let (cx, cy) = e.centre();
                SourceExtent {
                    centre: Vec2::new(cx, cy),
                    size: Vec2::new(e.width(), e.height()),
                    finest: e.width() / 200_000.0,
                }
            }
            SliceMode::Single => SourceExtent {
                centre: Vec2::ZERO,
                size: self.layout.cell,
                finest: self.layout.cell.x / 100_000.0,
            },
        }
    }

    pub fn limits(&self, viewport: Vec2) -> ViewLimits {
        self.extent().limits(viewport)
    }

    fn step(&mut self, delta: isize) {
        let count = self.cloud.slides.len() as isize;
        if count == 0 {
            return;
        }
        self.current = (((self.current as isize + delta) % count + count) % count) as usize;
    }
}

/// `G` switches layout; arrows, brackets and page keys step through slices.
pub fn slice_controls(keys: Res<ButtonInput<KeyCode>>, mut streamer: ResMut<SliceStreamer>) {
    if keys.just_pressed(KeyCode::KeyG) {
        streamer.mode = match streamer.mode {
            SliceMode::Grid => SliceMode::Single,
            SliceMode::Single => SliceMode::Grid,
        };
        streamer.refit = true;
    }

    let mut delta = 0isize;
    for (key, step) in [
        (KeyCode::BracketRight, 1),
        (KeyCode::BracketLeft, -1),
        (KeyCode::ArrowRight, 1),
        (KeyCode::ArrowLeft, -1),
        (KeyCode::PageDown, 1),
        (KeyCode::PageUp, -1),
    ] {
        if keys.just_pressed(key) {
            delta += step;
        }
    }
    if delta != 0 {
        streamer.step(delta);
        // Stepping in grid mode would be invisible, so show the slice instead.
        if streamer.mode == SliceMode::Grid {
            streamer.mode = SliceMode::Single;
            streamer.refit = true;
        }
    }
}

/// Decide which slide nodes should be resident.
pub fn select_slice_nodes(
    mut streamer: ResMut<SliceStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    let source = streamer.source;
    let cloud = streamer.cloud.clone();
    let mut seen = HashSet::new();
    // Collected with their depth so the budget can be spent shallowest first.
    // Walking slide by slide and paying as we went let the early slides spend
    // it all on their own detail, and the later ones never loaded at all.
    let mut candidates: Vec<(usize, SliceNode)> = Vec::new();

    // Every panel of this kind draws the same entities, so the resident set is
    // the union of what each of them needs.
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
        let units_per_px = ortho.area.width() / viewport.x.max(1.0);

        for slide_index in 0..cloud.slides.len() {
            if !streamer.visible(slide_index) {
                continue;
            }
            let offset = streamer.offset(slide_index);
            // Compare in the slide's own coordinates by moving the view rather
            // than the points, which keeps node bounds usable as they are.
            let view = Rect {
                min_x: centre.x - half.x - offset.x,
                min_y: -(centre.y + half.y - offset.y),
                max_x: centre.x + half.x - offset.x,
                max_y: -(centre.y - half.y - offset.y),
            };

            let slide = &cloud.slides[slide_index];
            // Cull the whole slide before walking it. Zoomed into one cell of a
            // 53-slice grid, this skips almost every slide outright.
            if !slide.bounds.intersects(&view) {
                continue;
            }
            let mut queue = vec![0usize];
            while let Some(node_index) = queue.pop() {
                let node = &slide.nodes[node_index];
                if !node.bounds.intersects(&view) {
                    continue;
                }
                let key = SliceNode {
                    slide: slide_index,
                    node: node_index,
                };
                if seen.insert(key) {
                    candidates.push((node.depth, key));
                }

                if node.bounds.width() / units_per_px.max(f32::MIN_POSITIVE) >= SUBDIVIDE_PX {
                    queue.extend(node.children.iter().copied());
                }
            }
        }
    }

    // Shallowest first, so a budget too small to hold everything gives up
    // detail rather than dropping whole slices out of the grid.
    candidates.sort_by_key(|(depth, _)| *depth);

    let mut budget = streamer.budget;
    let mut wanted = Vec::with_capacity(candidates.len());
    for (_, key) in candidates {
        let count = cloud.slides[key.slide].nodes[key.node].count as usize;
        if count > budget {
            continue;
        }
        budget -= count;
        wanted.push(key);
    }

    streamer.wanted = wanted;
}

pub fn spawn_slice_tasks(mut streamer: ResMut<SliceStreamer>) {
    let pool = AsyncComputeTaskPool::get();
    let cloud = streamer.cloud.clone();
    let selection = streamer.selection.clone();
    let wanted = std::mem::take(&mut streamer.wanted);

    for &key in &wanted {
        if streamer.in_flight >= MAX_IN_FLIGHT {
            break;
        }
        if streamer.slots.contains_key(&key) {
            continue;
        }

        let cloud = cloud.clone();
        let selection = selection.clone();
        let task = pool.spawn(async move {
            let node = &cloud.slides[key.slide].nodes[key.node];
            match load_node(&cloud, node, &selection) {
                Ok((positions, categories)) => NodeOutcome::Ready(positions, categories),
                Err(e) => NodeOutcome::Failed(e),
            }
        });
        streamer.slots.insert(key, Slot::Loading(task));
        streamer.in_flight += 1;
    }
    streamer.wanted = wanted;
}

pub fn collect_slice_tasks(
    mut commands: Commands,
    mut streamer: ResMut<SliceStreamer>,
    sources: Query<&datasource::DataSource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PointMaterial>>,
) {
    let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
        return;
    };
    let mut finished = Vec::new();
    for (key, slot) in streamer.slots.iter_mut() {
        let Slot::Loading(task) = slot else { continue };
        if let Some(outcome) = block_on(poll_once(task)) {
            finished.push((*key, outcome));
        }
    }

    for (key, outcome) in finished {
        streamer.in_flight = streamer.in_flight.saturating_sub(1);
        let slot = match outcome {
            NodeOutcome::Ready(positions, categories) => {
                let count = positions.len();
                let mesh = build_mesh(&positions, &categories);
                let offset = streamer.offset(key.slide);
                let entity = commands
                    .spawn((
                        Mesh2d(meshes.add(mesh)),
                        MeshMaterial2d(materials.add(PointMaterial::default())),
                        Transform::from_translation(offset.extend(0.0)),
                        RenderLayers::layer(layer),
                        SliceNodeTag(key),
                    ))
                    .id();
                streamer.resident_points += count;
                Slot::Ready {
                    entity,
                    points: count,
                }
            }
            NodeOutcome::Failed(e) => {
                warn!(
                    "slice node {}: {e}",
                    streamer.cloud.slides[key.slide].nodes[key.node].name
                );
                Slot::Failed
            }
        };
        streamer.slots.insert(key, slot);
    }
}

/// Move and show/hide slides after a mode or slice change.
///
/// Layout lives entirely in the transform, so this is all that a mode switch
/// costs — no node is refetched or rebuilt.
pub fn apply_slice_layout(
    streamer: Res<SliceStreamer>,
    mut nodes: Query<(&SliceNodeTag, &mut Transform, &mut Visibility)>,
) {
    if !streamer.is_changed() {
        return;
    }
    for (tag, mut transform, mut visibility) in &mut nodes {
        let offset = streamer.offset(tag.0.slide).extend(0.0);
        if transform.translation != offset {
            transform.translation = offset;
        }
        let wanted = if streamer.visible(tag.0.slide) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// Keep the source's advertised extent matching the current mode.
///
/// A frame opened onto the sections later is framed from this, so leaving it at
/// the value registered on startup left a new frame unable to zoom out past a
/// single slice until something happened to trigger a refit.
pub fn publish_extent(streamer: Res<SliceStreamer>, mut sources: Query<&mut SourceExtent>) {
    if !streamer.is_changed() {
        return;
    }
    let Ok(mut extent) = sources.get_mut(streamer.source) else {
        return;
    };
    *extent = streamer.extent();
}

/// Refit the panel camera when the mode or the selected slice changes.
pub fn refit_slice_camera(
    mut streamer: ResMut<SliceStreamer>,
    mut panels: Query<(
        &Camera,
        &mut Transform,
        &mut Projection,
        &mut ViewLimits,
        &ShowsSource,
    )>,
) {
    if !streamer.refit {
        return;
    }
    let source = streamer.source;
    for (camera, mut transform, mut projection, mut limits, shows) in &mut panels {
        if shows.0 != source {
            continue;
        }
        let Some(viewport) = camera.logical_viewport_size() else {
            return;
        };
        let Projection::Orthographic(ortho) = projection.as_mut() else {
            continue;
        };
        let fitted = streamer.limits(viewport);
        *limits = fitted;
        transform.translation = fitted.centre.extend(transform.translation.z);
        ortho.scale = fitted.fit_scale;
        streamer.refit = false;
    }
}

/// Drop nodes that are no longer wanted once the budget is exceeded.
pub fn evict_slice_nodes(mut commands: Commands, mut streamer: ResMut<SliceStreamer>) {
    let wanted: HashSet<SliceNode> = streamer.wanted.iter().copied().collect();
    if wanted.is_empty() || streamer.resident_points <= streamer.budget {
        return;
    }

    let cloud = streamer.cloud.clone();
    let mut candidates: Vec<(usize, usize, SliceNode)> = streamer
        .slots
        .iter()
        .filter(|(key, _)| !wanted.contains(*key))
        .filter_map(|(key, slot)| match slot {
            Slot::Ready { points, .. } => {
                Some((cloud.slides[key.slide].nodes[key.node].depth, *points, *key))
            }
            _ => None,
        })
        .collect();
    // Deepest first: shallow nodes are cheap and needed at every zoom level.
    candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0));

    for (_, points, key) in candidates {
        if streamer.resident_points <= streamer.budget {
            break;
        }
        if let Some(Slot::Ready { entity, .. }) = streamer.slots.remove(&key) {
            commands.entity(entity).despawn();
            streamer.resident_points = streamer.resident_points.saturating_sub(points);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sectioned() -> Arc<Scatterbrain> {
        Arc::new(Scatterbrain::parse(include_str!("../testdata/scatterbrain_slides.json")).unwrap())
    }

    /// The streamer only compares its source entity for equality, so tests do
    /// not need a live world to build one.
    fn streamer() -> SliceStreamer {
        SliceStreamer::new(sectioned(), Entity::PLACEHOLDER)
    }

    #[test]
    fn the_grid_covers_every_slide_without_overlap() {
        let streamer = streamer();
        let count = streamer.cloud.slides.len();
        assert_eq!(streamer.layout.grid.len(), count);

        // Cells are uniform, so distinct slides must land in distinct cells.
        let mut seen = HashSet::new();
        for offset in &streamer.layout.grid {
            let cell = (
                (offset.x / streamer.layout.cell.x).round() as i32,
                (offset.y / streamer.layout.cell.y).round() as i32,
            );
            assert!(seen.insert(cell), "two slides share a grid cell");
        }
        assert_eq!(seen.len(), count);
    }

    #[test]
    fn every_slide_is_drawn_in_grid_mode_and_one_in_single_mode() {
        let mut streamer = streamer();
        assert!((0..53).all(|i| streamer.visible(i)));

        streamer.mode = SliceMode::Single;
        streamer.current = 7;
        assert!(streamer.visible(7));
        assert!(!streamer.visible(6));
        assert_eq!((0..53).filter(|i| streamer.visible(*i)).count(), 1);
    }

    #[test]
    fn slides_are_recentred_so_differently_sized_slices_align() {
        let mut streamer = streamer();
        streamer.mode = SliceMode::Single;

        // In single mode each slide should sit at the origin regardless of
        // where its raw coordinates happen to fall.
        for slide in [0usize, 18, 52] {
            let bounds = streamer.cloud.slides[slide].tight_bounds;
            let (cx, cy) = bounds.centre();
            let offset = streamer.offset(slide);
            assert!((offset.x + cx).abs() < 1e-4);
            assert!((offset.y - cy).abs() < 1e-4);
        }
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let mut streamer = streamer();
        streamer.current = 52;
        streamer.step(1);
        assert_eq!(streamer.current, 0);
        streamer.step(-1);
        assert_eq!(streamer.current, 52);
        streamer.step(-53);
        assert_eq!(streamer.current, 52);
    }

    #[test]
    fn switching_mode_moves_slides_rather_than_reloading_them() {
        let mut streamer = streamer();
        let grid = streamer.offset(30);
        streamer.mode = SliceMode::Single;
        let single = streamer.offset(30);
        // The slide has to move, and the move is the whole cost of the switch.
        assert_ne!(grid, single);
    }

    /// The grid shows every slice, so the default budget has to hold every
    /// slice's root subsample. Falling short does not thin the grid evenly; it
    /// drops whole slices off the end of the list.
    #[test]
    fn the_default_budget_holds_every_slice_at_once() {
        let cloud = sectioned();
        let roots: u64 = cloud.slides.iter().map(|s| s.root().count).sum();
        assert!(
            (roots as usize) < DEFAULT_SLICE_BUDGET,
            "{roots} points of roots will not fit in {DEFAULT_SLICE_BUDGET}"
        );
    }

    #[test]
    fn the_advertised_extent_follows_the_mode() {
        // A frame opened onto the sections is framed from this, so showing
        // every slice at once has to advertise a wider extent than showing one.
        let mut streamer = streamer();
        let grid = streamer.extent();
        streamer.mode = SliceMode::Single;
        let single = streamer.extent();

        assert!(grid.size.x > single.size.x);
        assert!(grid.size.y > single.size.y);
        // One slice is centred on the origin; the grid is not.
        assert_eq!(single.centre, Vec2::ZERO);
    }

    #[test]
    fn the_grid_extent_covers_every_slice() {
        let streamer = streamer();
        let extent = streamer.extent();
        let columns = streamer.columns() as f32;
        // Wide enough for a full row of cells, or slices would fall outside the
        // view a frame opens onto.
        assert!(extent.size.x >= streamer.layout.cell.x * (columns - 1.0));
    }

    #[test]
    fn grid_limits_frame_all_the_slices() {
        let all = streamer();
        let viewport = Vec2::new(800.0, 600.0);
        let grid = all.limits(viewport);

        let mut single = streamer();
        single.mode = SliceMode::Single;
        let one = single.limits(viewport);

        // Showing 53 slices at once has to be a wider view than showing one.
        assert!(grid.fit_scale > one.fit_scale);
    }
}

/// Streams a sectioned Scatterbrain dataset.
pub struct SlicesPlugin {
    pub cloud: Arc<Scatterbrain>,
    pub budget: usize,
}

impl Plugin for SlicesPlugin {
    fn build(&self, app: &mut App) {
        let (w, h) = self.cloud.max_slide_extent();
        let source = datasource::register(
            app,
            datasource::SourceInfo {
                name: "Sections".into(),
                unit: self.cloud.unit.clone(),
                detail: format!("Scatterbrain, {} sections", self.cloud.slides.len()),
                stat: format!(
                    "{} CELLS",
                    datasource::compact_count(self.cloud.total_points())
                ),
            },
            // Replaced on the first update by `publish_extent`, once the grid
            // layout is known.
            SourceExtent {
                centre: Vec2::ZERO,
                size: Vec2::new(w, h),
                finest: w / 100_000.0,
            },
        );

        app.world_mut().entity_mut(source).insert((
            crate::points_render::SourcePointSize::default(),
            // Placeholder until a lookup service supplies the real value
            // labels; the column names and ids are the dataset's own.
            crate::cellproperties::placeholder_properties(&self.cloud.category_columns()),
        ));

        let mut streamer = SliceStreamer::new(self.cloud.clone(), source);
        streamer.budget = self.budget;

        app.insert_resource(streamer).add_systems(
            Update,
            (
                slice_controls,
                select_slice_nodes,
                spawn_slice_tasks,
                collect_slice_tasks,
                evict_slice_nodes,
                apply_slice_layout,
                refit_slice_camera,
                publish_extent,
                report_status,
            )
                .chain()
                .after(crate::panel::update_viewports),
        );
    }
}

fn report_status(streamer: Res<SliceStreamer>, mut sources: Query<&mut SourceStatus>) {
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

    let showing = match streamer.mode {
        SliceMode::Grid => format!(
            "grid of {} across {} columns",
            cloud.slides.len(),
            streamer.columns()
        ),
        SliceMode::Single => {
            let slide = &cloud.slides[streamer.current];
            format!(
                "slice {} of {}  [{}]  {} points",
                slide.index + 1,
                cloud.slides.len(),
                slide.id,
                slide.total_points,
            )
        }
    };

    status.0 = format!(
        "{} points in {} slices\n\
         {}\n\
         {} nodes loaded, {} loading\n\
         {} / {} points resident ({} MB)\n\
         colour by  {}\n\
         G grid/single · arrows or [ ] step slices",
        cloud.total_points(),
        cloud.slides.len(),
        showing,
        streamer.loaded_nodes(),
        streamer.in_flight,
        streamer.resident_points,
        streamer.budget,
        crate::points_render::budget_megabytes(streamer.resident_points),
        colour,
    );
}
