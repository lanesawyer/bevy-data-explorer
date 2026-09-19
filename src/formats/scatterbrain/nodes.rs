//! Fetching, decoding and meshing octree nodes, and holding the ones resident.
//!
//! Shared by both Scatterbrain plugins: a single cloud and a sectioned dataset
//! differ in how they choose and lay out nodes, not in what a node is, how its
//! bytes become vertices, or how the resident set is kept. This lived in
//! `pointcloud` and was imported from `slices`, which made one of the two
//! consumers look like the owner.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use bevy::prelude::*;

use crate::app::net::Fetching;
use crate::render::points::build_point_mesh;
use crate::source::properties::CellSelection;

use super::{Node, Rect, Scatterbrain, decode_categories, decode_floats, decode_positions};

/// How close the pointer has to come to a point to pick it, in logical pixels.
///
/// Points draw at about a pixel and a half, so picking has to reach further
/// than a point is wide or nothing would ever be hit.
pub const PICK_PX: f32 = 7.0;

/// A resident node's points, kept on the CPU after its mesh is built.
///
/// The pointer has to be resolved against actual coordinates and a mesh cannot
/// be read back, so this is what makes hovering possible at all. Ten bytes a
/// point against the eighty each already costs on the GPU.
pub struct NodePoints {
    pub positions: Vec<[f32; 2]>,
    pub categories: Vec<u16>,
}

impl NodePoints {
    /// The point nearest `target` within `limit` squared units, as an offset
    /// into this node and its value in the coloured-by column.
    ///
    /// Shared with the sectioned streamer, which differs only in having to move
    /// the target into each slide's own coordinates first.
    pub fn nearest(&self, target: Vec2, limit: f32) -> Option<(f32, usize, Vec2, Option<u16>)> {
        let mut best: Option<(f32, usize, Vec2, Option<u16>)> = None;
        for (offset, point) in self.positions.iter().enumerate() {
            let point = Vec2::from(*point);
            let distance = (point - target).length_squared();
            if distance > limit {
                continue;
            }
            if best.is_none_or(|(nearest, ..)| distance < nearest) {
                best = Some((
                    distance,
                    offset,
                    point,
                    self.categories.get(offset).copied(),
                ));
            }
        }
        best
    }
}

/// The rectangle a probe can reach, in dataset coordinates.
pub fn pick_reach(target: Vec2, radius: f32) -> Rect {
    Rect {
        min_x: target.x - radius,
        min_y: target.y - radius,
        max_x: target.x + radius,
        max_y: target.y + radius,
    }
}

pub enum NodeOutcome {
    Ready(Vec<[f32; 2]>, Vec<u16>),
    Failed(String),
}

/// Fetch a node's points, the column they are coloured by, and any columns the
/// filters restrict.
///
/// Filtered-out points are dropped here rather than hidden later, so they cost
/// no vertices and no budget.
pub async fn load_node(
    cloud: &Scatterbrain,
    node: &Node,
    selection: &CellSelection,
) -> Result<(Vec<[f32; 2]>, Vec<u16>), String> {
    let mut positions = decode_positions(&fetch(&cloud.positions_url(node)).await?, node.count)?;

    let mut categories = match &selection.colour_by {
        Some(column) => {
            let bytes = fetch(&cloud.column_url(column, node)).await?;
            decode_categories(&bytes, node.count)?
        }
        None => Vec::new(),
    };

    if selection.filters.is_empty() {
        return Ok((positions, categories));
    }

    // A column is decoded to whatever its restriction compares against: codes
    // for a categorical filter, floats for a numeric one.
    let mut columns: Vec<Vec<f32>> = Vec::with_capacity(selection.filters.len());
    for (column, restriction) in &selection.filters {
        let bytes = fetch(&cloud.column_url(column, node)).await?;
        let values = if restriction.is_numeric() {
            decode_floats(&bytes, node.count)?
        } else {
            decode_categories(&bytes, node.count)?
                .into_iter()
                .map(f32::from)
                .collect()
        };
        columns.push(values);
    }

    let mut values = vec![0.0f32; columns.len()];
    let mut keep = Vec::with_capacity(positions.len());
    for index in 0..positions.len() {
        for (slot, column) in values.iter_mut().zip(&columns) {
            *slot = column.get(index).copied().unwrap_or_default();
        }
        keep.push(selection.admits(&values));
    }

    let mut index = 0;
    positions.retain(|_| {
        index += 1;
        keep[index - 1]
    });
    if !categories.is_empty() {
        let mut index = 0;
        categories.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }
    Ok((positions, categories))
}

/// Node reads in flight at once, per source.
const MAX_IN_FLIGHT: usize = 12;

enum Slot {
    Loading(Fetching<NodeOutcome>),
    Ready {
        entity: Entity,
        points: usize,
        resident: NodePoints,
    },
    /// Loaded, and the filters left nothing of it. Nothing is spawned for a
    /// node like this: see the note in [`NodeCache::collect`].
    Empty,
    Failed,
}

/// One source's octree nodes: those being read, those resident, and the
/// previous selection's, held on screen while a new one loads.
///
/// Keyed however the format names a node: an index into the one slide, or a
/// slide and an index into it.
pub struct NodeCache<K> {
    slots: HashMap<K, Slot>,
    wanted: Vec<K>,
    /// Nodes of the selection being replaced, kept on screen until the new one
    /// is built. Empty except while a swap is pending, and keyed so that a
    /// layout change moves them like any other node.
    retiring: Vec<(K, Entity)>,
    in_flight: usize,
    /// Reads given up on because the view moved off them, counted for the
    /// status line: it is the number that says whether panning is costing
    /// anything.
    cancelled: usize,
    resident_points: usize,
    pub budget: usize,
}

impl<K: Copy + Eq + Hash> NodeCache<K> {
    pub fn new(budget: usize) -> Self {
        NodeCache {
            slots: HashMap::new(),
            wanted: Vec::new(),
            retiring: Vec::new(),
            in_flight: 0,
            cancelled: 0,
            resident_points: 0,
            budget,
        }
    }

    /// The nodes the view wants resident, coarse ones first.
    pub fn want(&mut self, wanted: Vec<K>) {
        self.wanted = wanted;
    }

    /// Give up on reads the view has moved off, then start one for each wanted
    /// node not held yet, in order, until the queue is full.
    ///
    /// Dropping a slot aborts the request behind it, which is the whole reason
    /// the reads are asynchronous: a pan across a cloud used to pay for every
    /// node it crossed, because a blocking read could only be declined before
    /// it started. It also frees a place in the queue for a node that is wanted.
    pub fn start(&mut self, mut read: impl FnMut(K) -> Fetching<NodeOutcome>) {
        let keep: HashSet<K> = self.wanted.iter().copied().collect();
        let mut cancelled = 0usize;
        self.slots.retain(|key, slot| {
            let abandon = matches!(slot, Slot::Loading(_)) && !keep.contains(key);
            cancelled += usize::from(abandon);
            !abandon
        });
        self.in_flight = self.in_flight.saturating_sub(cancelled);
        self.cancelled += cancelled;

        for index in 0..self.wanted.len() {
            if self.in_flight >= MAX_IN_FLIGHT {
                break;
            }
            let key = self.wanted[index];
            if self.slots.contains_key(&key) {
                continue;
            }
            self.slots.insert(key, Slot::Loading(read(key)));
            self.in_flight += 1;
        }
    }

    /// Build what finished reads delivered, coloured as `selection` says,
    /// spawning each node with points through `spawn`, and return the reads
    /// that failed.
    pub fn collect(
        &mut self,
        selection: &CellSelection,
        mut spawn: impl FnMut(K, Mesh) -> Entity,
    ) -> Vec<(K, String)> {
        let mut finished = Vec::new();
        for (key, slot) in &mut self.slots {
            if let Slot::Loading(task) = slot
                && let Some(outcome) = task.take()
            {
                finished.push((*key, outcome));
            }
        }
        self.in_flight = self.in_flight.saturating_sub(finished.len());

        finished
            .into_iter()
            .filter_map(|(key, outcome)| self.land(key, outcome, selection, &mut spawn))
            .collect()
    }

    /// Record what one read came to, and its error if it failed.
    fn land(
        &mut self,
        key: K,
        outcome: NodeOutcome,
        selection: &CellSelection,
        spawn: impl FnOnce(K, Mesh) -> Entity,
    ) -> Option<(K, String)> {
        let mut failed = None;
        let slot = match outcome {
            // A filter can leave a node with nothing in it. Spawning it anyway
            // costs an entity and a draw call to draw no points, and Bevy's
            // mesh allocator skips allocating a zero-length vertex buffer while
            // still copying into it, which it reports as a use-after-free for
            // as long as the node stays resident.
            NodeOutcome::Ready(positions, _) if positions.is_empty() => Slot::Empty,
            NodeOutcome::Ready(positions, categories) => {
                let points = positions.len();
                let entity = spawn(key, build_mesh(&positions, &categories, selection));
                self.resident_points += points;
                Slot::Ready {
                    entity,
                    points,
                    resident: NodePoints {
                        positions,
                        categories,
                    },
                }
            }
            NodeOutcome::Failed(e) => {
                failed = Some((key, e));
                Slot::Failed
            }
        };
        self.slots.insert(key, slot);
        failed
    }

    /// Record a read as landed without spawning anything, for tests of what
    /// is done with resident nodes.
    #[cfg(test)]
    pub fn landed(&mut self, key: K, outcome: NodeOutcome) {
        self.land(key, outcome, &CellSelection::default(), |_, _| {
            Entity::PLACEHOLDER
        });
    }

    /// Drop resident nodes no longer wanted, deepest first, until the points
    /// fit the budget. Shallow nodes are cheap to keep and needed at every zoom.
    pub fn evict(&mut self, commands: &mut Commands, depth: impl Fn(&K) -> usize) {
        if self.wanted.is_empty() || self.resident_points <= self.budget {
            return;
        }
        let wanted: HashSet<K> = self.wanted.iter().copied().collect();
        let mut candidates: Vec<(usize, usize, K)> = self
            .slots
            .iter()
            .filter(|(key, _)| !wanted.contains(*key))
            .filter_map(|(key, slot)| match slot {
                Slot::Ready { points, .. } => Some((depth(key), *points, *key)),
                _ => None,
            })
            .collect();
        candidates.sort_unstable_by_key(|candidate| std::cmp::Reverse(candidate.0));

        for (_, points, key) in candidates {
            if self.resident_points <= self.budget {
                break;
            }
            if let Some(Slot::Ready { entity, .. }) = self.slots.remove(&key) {
                commands.entity(entity).despawn();
                self.resident_points = self.resident_points.saturating_sub(points);
            }
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
        for (key, slot) in &self.slots {
            if let Slot::Ready { entity, .. } = slot {
                if pending {
                    commands.entity(*entity).despawn();
                } else {
                    self.retiring.push((*key, *entity));
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
    pub fn generation_ready(&self) -> bool {
        self.in_flight == 0
            && self.wanted.iter().all(|key| {
                matches!(
                    self.slots.get(key),
                    Some(Slot::Ready { .. } | Slot::Empty | Slot::Failed)
                )
            })
    }

    /// Show the new selection, each node as `shown` says, and drop the one it
    /// replaces.
    pub fn reveal(&mut self, commands: &mut Commands, shown: impl Fn(&K) -> bool) {
        for (_, entity) in self.retiring.drain(..) {
            commands.entity(entity).despawn();
        }
        for (key, entity, _) in self.ready() {
            commands.entity(entity).insert(if shown(key) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }

    /// Every resident node, with its entity and its points.
    pub fn ready(&self) -> impl Iterator<Item = (&K, Entity, &NodePoints)> {
        self.slots.iter().filter_map(|(key, slot)| match slot {
            Slot::Ready {
                entity, resident, ..
            } => Some((key, *entity, resident)),
            _ => None,
        })
    }

    /// The previous selection's nodes, while a swap is pending.
    pub fn retiring(&self) -> &[(K, Entity)] {
        &self.retiring
    }

    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }

    fn loaded(&self) -> usize {
        self.slots
            .values()
            .filter(|slot| matches!(slot, Slot::Ready { .. } | Slot::Empty))
            .count()
    }

    /// The cache's lines of a source's status.
    pub fn status(&self) -> String {
        let cancelled = if self.cancelled > 0 {
            format!(", {} cancelled", self.cancelled)
        } else {
            String::new()
        };
        format!(
            "{} nodes loaded, {} loading{cancelled}\n\
             {} / {} points resident ({} MB)",
            self.loaded(),
            self.in_flight,
            self.resident_points,
            self.budget,
            crate::render::points::budget_megabytes(self.resident_points),
        )
    }
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    crate::app::net::fetch(url).await
}

/// Build a node's mesh, colouring each point by its category in the colours
/// `selection` gives them.
///
/// Each point is a quad for the point shader to size; see
/// [`build_point_mesh`].
pub fn build_mesh(positions: &[[f32; 2]], categories: &[u16], selection: &CellSelection) -> Mesh {
    let points: Vec<Vec2> = positions.iter().map(|p| Vec2::new(p[0], p[1])).collect();

    let coloured = categories.len() == positions.len();
    let colours: Vec<[f32; 4]> = if coloured {
        categories.iter().map(|c| selection.colour(*c)).collect()
    } else {
        vec![[0.8, 0.85, 0.9, 1.0]; positions.len()]
    };

    // Each vertex carries its point's category so that hovering one can enlarge
    // the rest sharing it. Without a colour-by column there are no groups to
    // pick out, and the mesh says so by carrying none.
    build_point_mesh(&points, &colours, if coloured { categories } else { &[] })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache with `asked` wanted and the first `built` of them resident.
    fn staged(asked: &[usize], built: usize) -> NodeCache<usize> {
        let mut cache = NodeCache::new(usize::MAX);
        cache.want(asked.to_vec());
        for index in &asked[..built] {
            cache.landed(*index, NodeOutcome::Ready(vec![[0.0, 0.0]], Vec::new()));
        }
        cache
    }

    #[test]
    fn a_selection_is_shown_only_once_every_node_it_asked_for_is_built() {
        // The whole point of holding the previous nodes on screen: revealing a
        // half-built set is the flash, drawn over the old picture instead of
        // replacing it.
        assert!(
            !staged(&[0, 1, 2], 2).generation_ready(),
            "one node is still missing"
        );
        assert!(staged(&[0, 1, 2], 3).generation_ready());
    }

    #[test]
    fn a_node_still_loading_holds_the_swap() {
        let mut cache = staged(&[0, 1], 2);
        cache.in_flight = 1;
        assert!(!cache.generation_ready());
    }

    #[test]
    fn a_node_that_cannot_be_read_does_not_hold_the_swap_for_good() {
        // Waiting on a node that will never arrive would leave the previous
        // selection on screen with no way back.
        let mut cache = staged(&[0, 1], 1);
        cache.landed(1, NodeOutcome::Failed(String::new()));
        assert!(cache.generation_ready());
    }

    #[test]
    fn a_node_the_filter_emptied_counts_as_arrived() {
        // Nothing is spawned for it, so the swap has nothing to wait for.
        let mut cache = staged(&[0, 1], 1);
        cache.landed(1, NodeOutcome::Ready(Vec::new(), Vec::new()));
        assert!(cache.generation_ready());
        assert_eq!(cache.loaded(), 2);
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
    fn categories_get_distinguishable_colours() {
        // Adjacent label indices are unrelated, so they must not look alike.
        let selection = CellSelection::default();
        let a = selection.colour(0);
        let b = selection.colour(1);
        let distance: f32 = (0..3).map(|i| (a[i] - b[i]).abs()).sum();
        assert!(distance > 0.2, "neighbouring categories look too similar");
        assert_eq!(a[3], 1.0);
    }

    #[test]
    fn a_mesh_without_categories_still_builds() {
        let mesh = build_mesh(&[[0.0, 0.0], [1.0, 1.0]], &[], &CellSelection::default());
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
        let mesh = build_mesh(&[[2.0, 3.0]], &[], &CellSelection::default());
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x3(values) = positions else {
            panic!("unexpected position format");
        };
        assert_eq!(values[0], [2.0, -3.0, 0.0]);
    }

    #[test]
    fn a_publisher_colour_is_the_one_drawn() {
        let selection = CellSelection {
            palette: vec![[1.0, 0.0, 0.0, 1.0]],
            ..default()
        };
        assert_eq!(selection.colour(0), [1.0, 0.0, 0.0, 1.0]);
        // A code past the palette falls back rather than going unpainted.
        assert_eq!(selection.colour(1), CellSelection::default().colour(1));
    }
}
