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
//!
//! Which slice the single mode shows is the source's [`SliceStack`], as it is
//! for a volumetric image, so the frame's paging keys and the sidebar's slider
//! step through sections without knowing they are not an image.

use std::collections::HashSet;
use std::sync::Arc;

use crate::app::net::fetching;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::formats::scatterbrain::nodes::{NodeCache, NodeOutcome, PICK_PX, load_node, pick_reach};
use crate::formats::scatterbrain::{Rect, Scatterbrain};
use crate::render::points::{PointMaterial, SourceHighlight};
use crate::source::ViewLimits;
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::properties::{CellProperties, CellSelection};
use crate::source::stack::SliceStack;
use crate::source::{self, DataSource, SourceBusy, SourceExtent, SourceStatus};
use crate::view::ShowsSource;

/// Descend into a slide's octree while its region covers at least this many
/// screen pixels.
const SUBDIVIDE_PX: f32 = 420.0;

/// Gap between grid cells, as a fraction of the cell size.
const CELL_PADDING: f32 = 0.06;

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

/// Marks a spawned slice node.
///
/// Which node it draws is not recorded here: the streamer's own slots say that,
/// and reading it from there is what keeps one sectioned source from laying out
/// another's nodes.
#[derive(Component)]
pub struct SliceNodeTag;

/// Whether a node is drawn, as the component that says so.
fn visibility_of(shown: bool) -> Visibility {
    if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// A point found under the pointer, and the slice it belongs to.
pub struct SliceHit {
    pub key: SliceNode,
    /// Its offset within that node's column files.
    pub index: usize,
    pub position: Vec2,
    pub category: Option<u16>,
}

#[derive(Component)]
pub struct SliceStreamer {
    /// The source entity this streamer serves.
    pub source: Entity,
    cloud: Arc<Scatterbrain>,
    /// What to colour by and what to filter out, mirrored from the source's
    /// properties so that workers can be handed a copy.
    pub selection: CellSelection,
    pub mode: SliceMode,
    /// Slice shown in [`SliceMode::Single`], followed from the source's
    /// [`SliceStack`].
    pub current: usize,
    /// Where each slide sits, by mode.
    layout: Layout,
    nodes: NodeCache<SliceNode>,
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
            nodes: NodeCache::new(DEFAULT_SLICE_BUDGET),
            refit: true,
        }
    }

    pub fn cloud(&self) -> &Arc<Scatterbrain> {
        &self.cloud
    }

    pub fn columns(&self) -> usize {
        self.layout.columns
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

    /// The resident point nearest the probe, within a few pixels of it.
    ///
    /// Slices share one coordinate system and are pulled apart by a per-slide
    /// offset held in each node's transform, so the pointer is moved into a
    /// slide's own coordinates rather than the points being moved out of them —
    /// the same trick node selection uses, and what keeps node bounds usable as
    /// they are.
    pub fn pick(&self, probe: &HoverProbe) -> Option<SliceHit> {
        let radius = probe.radius(PICK_PX);
        let limit = radius * radius;

        let mut best: Option<(f32, SliceHit)> = None;
        for (key, _, resident) in self.nodes.ready() {
            if !self.visible(key.slide) {
                continue;
            }
            let offset = self.offset(key.slide);
            let local = probe.world - offset;
            // World y is negated for display; node bounds are in slide space.
            let target = Vec2::new(local.x, -local.y);

            let node = &self.cloud.slides[key.slide].nodes[key.node];
            if !node.bounds.intersects(&pick_reach(target, radius)) {
                continue;
            }
            let Some((distance, index, position, category)) = resident.nearest(target, limit)
            else {
                continue;
            };
            if best.as_ref().is_none_or(|(nearest, _)| distance < *nearest) {
                best = Some((
                    distance,
                    SliceHit {
                        key: *key,
                        index,
                        position,
                        category,
                    },
                ));
            }
        }
        best.map(|(_, hit)| hit)
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
}

/// `G` switches layout.
pub fn slice_controls(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<crate::view::TextEntryFocused>,
    selected: Res<crate::view::SelectedPanel>,
    panels: Query<&ShowsSource>,
    mut streamers: Query<&mut SliceStreamer>,
) {
    if typing.0 || !keys.just_pressed(KeyCode::KeyG) {
        return;
    }
    // The selected frame's sections and no other: two sectioned datasets open
    // at once are switched one at a time, and the outline says which.
    let Some(source) = crate::view::selected_source(&selected, &panels) else {
        return;
    };
    if let Ok(mut streamer) = streamers.get_mut(source) {
        streamer.mode = match streamer.mode {
            SliceMode::Grid => SliceMode::Single,
            SliceMode::Single => SliceMode::Grid,
        };
        streamer.refit = true;
        info!(
            "sections: {}",
            match streamer.mode {
                SliceMode::Grid => "showing every slice".to_string(),
                SliceMode::Single => format!("showing slice {}", streamer.current + 1),
            }
        );
    }
}

/// Show whichever slice the source's stack is on.
///
/// Stepping in grid mode would be invisible, so a move there switches to the
/// single slice it moved to.
pub fn follow_slice_stack(
    mut streamers: Query<(&SliceStack, &mut SliceStreamer), Changed<SliceStack>>,
) {
    for (stack, mut streamer) in &mut streamers {
        let wanted = stack.current as usize;
        if streamer.current == wanted {
            continue;
        }
        streamer.current = wanted;
        if streamer.mode == SliceMode::Grid {
            streamer.mode = SliceMode::Single;
            streamer.refit = true;
        }
        info!("sections: showing {}", stack.label());
    }
}

/// Decide which slide nodes should be resident.
pub fn select_slice_nodes(
    mut streamers: Query<&mut SliceStreamer>,
    panels: Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
) {
    for mut streamer in &mut streamers {
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

        let mut budget = streamer.nodes.budget;
        let mut wanted = Vec::with_capacity(candidates.len());
        for (_, key) in candidates {
            let count = cloud.slides[key.slide].nodes[key.node].count as usize;
            if count > budget {
                continue;
            }
            budget -= count;
            wanted.push(key);
        }

        streamer.nodes.want(wanted);
    }
}

pub fn spawn_slice_tasks(mut streamers: Query<&mut SliceStreamer>) {
    for mut streamer in &mut streamers {
        let cloud = streamer.cloud.clone();
        let selection = streamer.selection.clone();
        // Stepping to another slice abandons a whole grid's worth at once.
        streamer.nodes.start(|key| {
            let cloud = cloud.clone();
            let selection = selection.clone();
            fetching(async move {
                let node = &cloud.slides[key.slide].nodes[key.node];
                match load_node(&cloud, node, &selection).await {
                    Ok((positions, categories)) => NodeOutcome::Ready(positions, categories),
                    Err(e) => NodeOutcome::Failed(e),
                }
            })
        });
    }
}

pub fn collect_slice_tasks(
    mut commands: Commands,
    mut streamers: Query<&mut SliceStreamer>,
    sources: Query<&DataSource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PointMaterial>>,
) {
    for mut streamer in &mut streamers {
        let Ok(layer) = sources.get(streamer.source).map(|s| s.layer) else {
            continue;
        };
        // Where each slide sits and whether it is drawn, worked out before the
        // nodes are borrowed to build into.
        let placed: Vec<(Vec2, bool)> = (0..streamer.cloud.slides.len())
            .map(|slide| (streamer.offset(slide), streamer.visible(slide)))
            .collect();
        let swapping = streamer.nodes.swapping();
        let failed = streamer.nodes.collect(|key, mesh| {
            let (offset, visible) = placed[key.slide];
            commands
                .spawn((
                    Mesh2d(meshes.add(mesh)),
                    MeshMaterial2d(materials.add(PointMaterial::default())),
                    Transform::from_translation(offset.extend(0.0)),
                    RenderLayers::layer(layer),
                    SliceNodeTag,
                    // Held back while the selection it replaces is still on
                    // screen: showing each node as it arrived would draw the
                    // new picture half-built over the old one.
                    visibility_of(!swapping && visible),
                ))
                .id()
        });
        for (key, e) in failed {
            warn!(
                "slice node {}: {e}",
                streamer.cloud.slides[key.slide].nodes[key.node].name
            );
        }
    }
}

/// Show a new selection once all of it has arrived, and drop the one it
/// replaces.
///
/// The two sets are on the GPU together for as long as the swap takes, which is
/// what buys the picture staying put. Nothing is revealed early, so a filter
/// either applies to every section or to none of them.
pub fn swap_slice_generations(mut commands: Commands, mut streamers: Query<&mut SliceStreamer>) {
    for mut streamer in &mut streamers {
        // Read before touching it: taking the streamer mutably every frame would
        // mark it changed for everything watching.
        if !streamer.nodes.swapping() || !streamer.nodes.generation_ready() {
            continue;
        }
        // Through the same rule as the layout, so a slide hidden by the current
        // mode stays hidden.
        let shown: Vec<bool> = (0..streamer.cloud.slides.len())
            .map(|slide| streamer.visible(slide))
            .collect();
        streamer.nodes.reveal(&mut commands, |key| shown[key.slide]);
    }
}

/// Move and show/hide slides after a mode or slice change.
///
/// Layout lives entirely in the transform, so this is all that a mode switch
/// costs — no node is refetched or rebuilt.
pub fn apply_slice_layout(
    streamers: Query<Ref<SliceStreamer>>,
    mut nodes: Query<(&mut Transform, &mut Visibility), With<SliceNodeTag>>,
) {
    // Walked per streamer rather than over every node in the world, so one
    // sectioned source cannot lay out another's nodes.
    for streamer in &streamers {
        if !streamer.is_changed() {
            continue;
        }
        // The nodes being replaced are laid out like any other: a mode change
        // mid-swap has to move what is on screen, not what is waiting behind it.
        let showable = !streamer.nodes.swapping();
        let staged = streamer
            .nodes
            .ready()
            .map(|(key, entity, _)| (key.slide, entity, showable));
        let retiring = streamer
            .nodes
            .retiring()
            .iter()
            .map(|(key, entity)| (key.slide, *entity, true));

        for (slide, entity, showable) in staged.chain(retiring) {
            let Ok((mut transform, mut visibility)) = nodes.get_mut(entity) else {
                continue;
            };
            let offset = streamer.offset(slide).extend(0.0);
            if transform.translation != offset {
                transform.translation = offset;
            }
            let wanted = visibility_of(showable && streamer.visible(slide));
            if *visibility != wanted {
                *visibility = wanted;
            }
        }
    }
}

/// Keep the source's advertised extent matching the current mode.
///
/// A frame opened onto the sections later is framed from this, so leaving it at
/// the value registered on startup left a new frame unable to zoom out past a
/// single slice until something happened to trigger a refit.
pub fn publish_extent(streamers: Query<Ref<SliceStreamer>>, mut sources: Query<&mut SourceExtent>) {
    for streamer in &streamers {
        if !streamer.is_changed() {
            continue;
        }
        let Ok(mut extent) = sources.get_mut(streamer.source) else {
            continue;
        };
        *extent = streamer.extent();
    }
}

/// Refit the panel camera when the mode or the selected slice changes.
pub fn refit_slice_camera(
    mut streamers: Query<&mut SliceStreamer>,
    mut panels: Query<(
        &Camera,
        &mut Transform,
        &mut Projection,
        &mut ViewLimits,
        &ShowsSource,
    )>,
) {
    for mut streamer in &mut streamers {
        if !streamer.refit {
            continue;
        }
        let source = streamer.source;
        for (camera, mut transform, mut projection, mut limits, shows) in &mut panels {
            if shows.0 != source {
                continue;
            }
            let Some(viewport) = camera.logical_viewport_size() else {
                continue;
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
}

/// Drop nodes that are no longer wanted once the budget is exceeded.
pub fn evict_slice_nodes(mut commands: Commands, mut streamers: Query<&mut SliceStreamer>) {
    for mut streamer in &mut streamers {
        let cloud = streamer.cloud.clone();
        streamer.nodes.evict(&mut commands, |key| {
            cloud.slides[key.slide].nodes[key.node].depth
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
    mut streamers: Query<(&CellProperties, &mut SliceStreamer), Changed<CellProperties>>,
) {
    for (properties, mut streamer) in &mut streamers {
        let selection = properties.selection();
        if streamer.selection != selection {
            streamer.selection = selection;
            streamer.nodes.retire(&mut commands);
        }
    }
}

/// The systems every sectioned dataset shares, registered once however many
/// are open.
pub struct SlicesSystems;

impl Plugin for SlicesSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                apply_selection,
                slice_controls,
                follow_slice_stack,
                select_slice_nodes,
                spawn_slice_tasks,
                collect_slice_tasks,
                swap_slice_generations,
                evict_slice_nodes,
                apply_slice_layout,
                refit_slice_camera,
                publish_extent,
                report_status,
            )
                .chain()
                .in_set(Stage::Sources),
        )
        // Resolving the pointer reads the nodes that are resident now, and
        // which slide is where; the schedule already puts `HoverProbing`
        // after the layout has settled.
        .add_systems(Update, resolve_hover.in_set(source::hover::HoverProbing));
    }
}

/// Register a parsed sectioned dataset as a source, and bind a streamer to it.
pub fn spawn_source(world: &mut World, cloud: Arc<Scatterbrain>, budget: usize) -> Entity {
    let (w, h) = cloud.max_slide_extent();
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: "Sections".into(),
            unit: cloud.unit.clone(),
            detail: format!("Scatterbrain, {} sections", cloud.slides.len()),
            stat: format!("{} CELLS", source::compact_count(cloud.total_points())),
        },
        // Replaced on the first update by `publish_extent`, once the grid
        // layout is known.
        SourceExtent {
            centre: Vec2::ZERO,
            size: Vec2::new(w, h),
            finest: w / 100_000.0,
        },
    );

    world.entity_mut(source).insert((
        crate::render::points::SourcePointSize::default(),
        // Both start empty, and are carried from registration so the hover
        // systems can write through a query rather than through commands.
        SourceHighlight::default(),
        HoverInfo::default(),
        // Placeholder until a lookup service supplies the real value
        // labels; the column names and ids are the dataset's own.
        crate::formats::scatterbrain::placeholder_properties(
            &cloud.category_columns(),
            &cloud.numeric_columns(),
        ),
    ));

    // A dataset of one section has nothing to page through, and offers
    // nothing to page with.
    let stack = SliceStack::new(cloud.slides.len() as u64);
    if stack.count > 1 {
        world.entity_mut(source).insert(stack);
    }

    let mut streamer = SliceStreamer::new(cloud, source);
    streamer.current = stack.current as usize;
    streamer.nodes.budget = budget;
    world.entity_mut(source).insert(streamer);
    source
}

/// Answer the pointer: which cell is under it, and which cells share its value.
pub fn resolve_hover(
    streamers: Query<&SliceStreamer>,
    probes: Query<&HoverProbe>,
    sources: Query<(&DataSource, &CellProperties)>,
    mut answers: Query<(&mut HoverInfo, &mut SourceHighlight)>,
) {
    for streamer in &streamers {
        let Ok((mut info, mut highlight)) = answers.get_mut(streamer.source) else {
            continue;
        };
        let Ok((source, properties)) = sources.get(streamer.source) else {
            continue;
        };

        let hit = probes
            .get(streamer.source)
            .ok()
            .and_then(|probe| streamer.pick(probe));

        let category = hit.as_ref().and_then(|hit| hit.category);
        if highlight.0 != category {
            highlight.0 = category;
        }

        // Both are left alone when unchanged: the highlight drives a uniform upload
        // per resident node, and the tooltip a text layout.
        let next = hit
            .map(|hit| describe(&hit, streamer, source, properties))
            .unwrap_or_default();
        if *info != next {
            *info = next;
        }
    }
}

/// Name a hit. A sectioned dataset addresses a point by slice as well as by
/// node, since every slide shares one reference id.
fn describe(
    hit: &SliceHit,
    streamer: &SliceStreamer,
    source: &DataSource,
    properties: &CellProperties,
) -> HoverInfo {
    let node = &streamer.cloud.slides[hit.key.slide].nodes[hit.key.node];
    let mut info = HoverInfo::titled(format!("{}#{}", node.name, hit.index));

    info = info.row(
        "slice",
        format!(
            "{} of {}",
            streamer.cloud.slides[hit.key.slide].index,
            streamer.cloud.slides.len()
        ),
    );
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
    streamers: Query<&SliceStreamer>,
    mut sources: Query<&mut SourceStatus>,
    mut busy: Query<&mut SourceBusy>,
) {
    for streamer in &streamers {
        if let Ok(mut busy) = busy.get_mut(streamer.source) {
            busy.set_if_neq(SourceBusy(streamer.nodes.busy()));
        }
        let Ok(mut status) = sources.get_mut(streamer.source) else {
            continue;
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
         {}\n\
         colour by  {}\n\
         G grid/single · arrows or [ ] step slices",
            cloud.total_points(),
            cloud.slides.len(),
            showing,
            streamer.nodes.status(),
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sectioned() -> Arc<Scatterbrain> {
        Arc::new(
            Scatterbrain::parse(include_str!("../../../testdata/scatterbrain_slides.json"))
                .unwrap(),
        )
    }

    /// The streamer only compares its source entity for equality, so tests do
    /// not need a live world to build one.
    fn streamer() -> SliceStreamer {
        SliceStreamer::new(sectioned(), Entity::PLACEHOLDER)
    }

    /// Make one node of `slide` resident, holding a single point.
    fn resident(streamer: &mut SliceStreamer, slide: usize, point: [f32; 2], category: u16) {
        streamer.nodes.landed(
            SliceNode { slide, node: 0 },
            NodeOutcome::Ready(vec![point], vec![category]),
        );
    }

    fn probe_at(world: Vec2) -> HoverProbe {
        HoverProbe {
            panel: Entity::PLACEHOLDER,
            world,
            units_per_px: 1.0,
        }
    }

    /// Where a point of `slide` is actually drawn: the mesh negates y and the
    /// node's transform carries the layout offset.
    fn drawn_at(streamer: &SliceStreamer, slide: usize, point: [f32; 2]) -> Vec2 {
        Vec2::new(point[0], -point[1]) + streamer.offset(slide)
    }

    #[test]
    fn hovering_a_laid_out_slice_picks_the_point_drawn_there() {
        // Picking has to undo exactly what the layout did, and a sign slipped
        // in either transform shows up only as a tooltip that never appears.
        let mut streamer = streamer();
        let slide = 3;
        let point = streamer.cloud.slides[slide].tight_bounds.centre();
        let point = [point.0, point.1];
        resident(&mut streamer, slide, point, 5);

        let hit = streamer
            .pick(&probe_at(drawn_at(&streamer, slide, point)))
            .expect("the pointer is on the point");
        assert_eq!(hit.key.slide, slide);
        assert_eq!(hit.category, Some(5));
    }

    #[test]
    fn a_slices_own_cell_is_where_it_is_picked_from() {
        // Slides share one coordinate system, so using raw coordinates would
        // pick from whichever slide happened to be at the origin.
        let mut streamer = streamer();
        let point = streamer.cloud.slides[4].tight_bounds.centre();
        let point = [point.0, point.1];
        resident(&mut streamer, 4, point, 0);

        assert!(
            streamer
                .pick(&probe_at(drawn_at(&streamer, 4, point)))
                .is_some()
        );
        // The same data coordinates, but in a different slide's cell.
        assert!(
            streamer
                .pick(&probe_at(drawn_at(&streamer, 0, point)))
                .is_none()
        );
    }

    #[test]
    fn a_hidden_slice_cannot_be_hovered() {
        // Single-slice mode leaves every other slide resident but not drawn,
        // and what is not drawn must not answer the pointer.
        let mut streamer = streamer();
        let point = streamer.cloud.slides[2].tight_bounds.centre();
        let point = [point.0, point.1];
        resident(&mut streamer, 2, point, 0);

        let world = drawn_at(&streamer, 2, point);
        assert!(streamer.pick(&probe_at(world)).is_some());

        streamer.mode = SliceMode::Single;
        streamer.current = 0;
        // Re-measured, since single-slice mode drops the grid offsets.
        let world = drawn_at(&streamer, 2, point);
        assert!(streamer.pick(&probe_at(world)).is_none());
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
