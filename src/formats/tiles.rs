//! What every tile pyramid streams through: which tiles the frames want, in
//! what order, and a least-recently-wanted cache of the ones that have landed.
//!
//! OME-Zarr and Deep Zoom differ in how a tile is read and drawn, and in which
//! way their levels are numbered, but not in what is asked for or kept. Each
//! frame wants the overview and the level matching its zoom; nothing off
//! screen is asked for until every visible tile has landed; then a margin
//! round the view and the levels either side. Tiles that leave the view are
//! kept under a memory budget rather than dropped, so zooming back out or
//! panning back redraws from what is resident.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;

use crate::app::net::Fetching;
use crate::source::ShowsSource;

/// Threads reserved for fetching and decoding tiles.
///
/// A tile read is blocking from end to end — a blocking HTTP request followed
/// by a synchronous decode — so one tile occupies one thread for its whole
/// duration and concurrency is capped by the pool, not by any limit here.
/// Bevy's default async-compute pool is at most four threads, which on a
/// typical machine leaves only three tiles in flight; these threads sit blocked
/// on a socket rather than burning CPU, so a much larger pool is appropriate.
pub const TILE_FETCH_THREADS: usize = 24;

/// Tile requests in flight at once, per pyramid, matched to the pool so that
/// queued work stays short enough to remain cancellable.
pub const MAX_IN_FLIGHT: usize = TILE_FETCH_THREADS;

/// Upper bound on remembered "nothing here" and failed tiles. These hold no
/// texture, and keeping them avoids re-requesting known-empty regions.
const MAX_EMPTY_SLOTS: usize = 32_768;

/// Frames an empty slot is kept past its last use once there are too many.
const EMPTY_SLOT_FRAMES: u64 = 600;

/// What one frame is looking at, in display coordinates.
pub struct View {
    pub center: Vec2,
    pub half: Vec2,
    /// How far past the edges a pan is likely to go next.
    pub margin: Vec2,
}

impl View {
    pub fn new(transform: &GlobalTransform, ortho: &OrthographicProjection) -> Self {
        let half = Vec2::new(ortho.area.width(), ortho.area.height()) * 0.5;
        View {
            center: transform.translation().truncate(),
            half,
            margin: half * 0.15,
        }
    }
}

/// What every flat frame showing `source` looks at, with the world units one
/// of its logical pixels covers, which is what picks a level.
///
/// The union over every frame, so a duplicate zoomed somewhere else streams its
/// own detail. A frame looking in 3D is skipped: it draws the volume, not tiles.
pub fn frame_views(
    panels: &Query<(&Camera, &GlobalTransform, &Projection, &ShowsSource)>,
    source: Entity,
) -> Vec<(View, f32)> {
    panels
        .iter()
        .filter(|(_, _, _, shows)| shows.0 == source)
        .filter_map(|(camera, transform, projection, _)| {
            let Projection::Orthographic(ortho) = projection else {
                return None;
            };
            let viewport = camera.logical_viewport_size()?;
            let units_per_px = ortho.area.width() / viewport.x.max(1.0);
            Some((View::new(transform, ortho), units_per_px))
        })
        .collect()
}

/// Tiles in the order they were first named, each named once.
pub struct Tiers<K> {
    pub order: Vec<K>,
    pub seen: HashSet<K>,
}

impl<K> Default for Tiers<K> {
    fn default() -> Self {
        Tiers {
            order: Vec::new(),
            seen: HashSet::new(),
        }
    }
}

impl<K: Copy + Eq + Hash> Tiers<K> {
    /// A tier to follow this one, skipping anything already named in it. Its
    /// `seen` ends up holding both.
    pub fn followed_by(&self) -> Self {
        Tiers {
            order: Vec::new(),
            seen: self.seen.clone(),
        }
    }

    pub fn extend(&mut self, keys: impl IntoIterator<Item = K>) {
        for key in keys {
            if self.seen.insert(key) {
                self.order.push(key);
            }
        }
    }
}

/// The visible and prefetch tiers for a set of views, each paired with the
/// level it draws.
///
/// Visible is the overview and each view's own level; prefetch is a margin
/// round each view, then the coarser and the finer level beside it. Levels are
/// numbered whichever way the format numbers them, so `beside` names the
/// coarser neighbour and the finer one, if there are any.
pub fn tiers<K: Copy + Eq + Hash, L: Copy>(
    views: &[(View, L)],
    overview: L,
    beside: impl Fn(L) -> [Option<L>; 2],
    tiles_over: impl Fn(L, &View, Vec2) -> Vec<K>,
) -> (Tiers<K>, Tiers<K>) {
    let mut visible = Tiers::default();
    // The overview first, so a cold open shows the whole image at once while
    // each frame's own level loads.
    for (view, _) in views {
        visible.extend(tiles_over(overview, view, view.half));
    }
    for (view, level) in views {
        visible.extend(tiles_over(*level, view, view.half));
    }
    let mut prefetch = visible.followed_by();
    for (view, level) in views {
        prefetch.extend(tiles_over(*level, view, view.half + view.margin));
        for neighbour in beside(*level).into_iter().flatten() {
            prefetch.extend(tiles_over(neighbour, view, view.half));
        }
    }
    (visible, prefetch)
}

/// The tiles to ask for this frame: what the frames show, and — only once all
/// of that has landed — what they might show next.
///
/// A tile off screen shares the connection with the ones on it. Even the last
/// request to be cut off has been taking bandwidth from a visible tile until
/// then, so the margin and the levels either side are not started while
/// anything visible is still loading. Leaving them out of the wanted set is
/// also what abandons one already under way the moment the view moves.
fn request_order<K: Copy>(visible: &[K], prefetch: &[K], resolved: impl Fn(&K) -> bool) -> Vec<K> {
    let mut wanted = visible.to_vec();
    if visible.iter().all(resolved) {
        wanted.extend_from_slice(prefetch);
    }
    wanted
}

pub enum SlotState<O, M> {
    Loading(Fetching<O>),
    Ready {
        entity: Entity,
        /// Texture footprint, used to keep the cache inside its budget.
        bytes: usize,
        /// Whatever the format keeps to redraw a tile without reading it
        /// again, such as the material its channels are mixed in.
        material: M,
    },
    /// Nothing to draw: the tile is outside the image or entirely fill value.
    Blank,
    Failed,
}

struct Slot<O, M> {
    state: SlotState<O, M>,
    /// Frame on which this tile was last part of the wanted set. Drives
    /// eviction order.
    last_wanted: u64,
}

/// One pyramid's tiles: those being read, those resident, and those known to
/// hold nothing, keyed however the format names a tile.
///
/// `O` is what a read produces and `M` what a resident tile keeps beside its
/// entity.
pub struct TileCache<K, O, M = ()> {
    slots: HashMap<K, Slot<O, M>>,
    /// Tiles to request, in order: visible ones, then prefetch once idle.
    wanted: Vec<K>,
    /// Tiles worth keeping resident whether or not they are being requested:
    /// the visible set and the prefetch set.
    retained: HashSet<K>,
    /// Frame counter driving least-recently-wanted eviction.
    frame: u64,
    in_flight: usize,
    /// Texture memory currently held by resident tiles.
    resident_bytes: usize,
    pub budget_bytes: usize,
    /// Reads given up on because the view moved off them, for the status line:
    /// it is the number that says whether panning is costing anything.
    canceled: usize,
}

impl<K: Copy + Eq + Hash, O: Send + 'static, M> TileCache<K, O, M> {
    pub fn new(budget_bytes: usize) -> Self {
        TileCache {
            slots: HashMap::new(),
            wanted: Vec::new(),
            retained: HashSet::new(),
            frame: 0,
            in_flight: 0,
            resident_bytes: 0,
            budget_bytes,
            canceled: 0,
        }
    }

    /// Settle what the frames want this frame, and return it in request order.
    ///
    /// Everything worth keeping is touched, prefetch included, so eviction can
    /// tell live tiles from stale ones whether or not they are being asked for.
    pub fn want(&mut self, (visible, prefetch): (Tiers<K>, Tiers<K>)) -> &[K] {
        self.frame = self.frame.wrapping_add(1);
        for key in &prefetch.seen {
            if let Some(slot) = self.slots.get_mut(key) {
                slot.last_wanted = self.frame;
            }
        }
        self.wanted = request_order(&visible.order, &prefetch.order, |key| {
            self.slots
                .get(key)
                .is_some_and(|slot| !matches!(slot.state, SlotState::Loading(_)))
        });
        self.retained = prefetch.seen;
        &self.wanted
    }

    pub fn wanted(&self) -> &[K] {
        &self.wanted
    }

    /// Start a read for each wanted tile not held yet, in order, until
    /// [`MAX_IN_FLIGHT`] are under way.
    pub fn start(&mut self, mut read: impl FnMut(K) -> Fetching<O>) {
        for index in 0..self.wanted.len() {
            if self.in_flight >= MAX_IN_FLIGHT {
                break;
            }
            let key = self.wanted[index];
            if self.slots.contains_key(&key) {
                continue;
            }
            self.slots.insert(
                key,
                Slot {
                    state: SlotState::Loading(read(key)),
                    last_wanted: self.frame,
                },
            );
            self.in_flight += 1;
        }
    }

    /// Take every read that has finished. Each must be handed back to
    /// [`Self::settle`] or [`Self::forget`].
    pub fn finished(&mut self) -> Vec<(K, O)> {
        let mut finished = Vec::new();
        for (key, slot) in &mut self.slots {
            if let SlotState::Loading(task) = &mut slot.state
                && let Some(outcome) = task.take()
            {
                finished.push((*key, outcome));
            }
        }
        self.in_flight = self.in_flight.saturating_sub(finished.len());
        finished
    }

    /// Record what a finished read came to.
    pub fn settle(&mut self, key: K, state: SlotState<O, M>) {
        if let SlotState::Ready { bytes, .. } = state {
            self.resident_bytes += bytes;
        }
        self.slots.insert(
            key,
            Slot {
                state,
                last_wanted: self.frame,
            },
        );
    }

    /// Drop a read that gave up before doing any work, so the tile can be
    /// asked for again if the view comes back.
    pub fn forget(&mut self, key: K) {
        self.slots.remove(&key);
        self.canceled += 1;
    }

    /// Abandon reads the view has moved off, and keep resident tiles inside the
    /// budget, least recently wanted first.
    ///
    /// Tiles are deliberately *not* dropped as soon as they leave the viewport.
    /// Zooming in narrows the wanted set to a handful of fine tiles, and the
    /// coarse ones covering the surrounding area are exactly what is needed
    /// again a moment later when zooming back out. Holding them until memory
    /// runs short makes that round trip free.
    pub fn evict(&mut self, commands: &mut Commands) {
        let wanted: HashSet<K> = self.wanted.iter().copied().collect();

        // Dropping a slot aborts its read, which for an image tile is
        // megabytes of chunk fetched and decoded for a view nobody is looking
        // at any more.
        let mut canceled = 0usize;
        self.slots.retain(|key, slot| {
            let abandon = matches!(slot.state, SlotState::Loading(_)) && !wanted.contains(key);
            canceled += usize::from(abandon);
            !abandon
        });
        self.in_flight = self.in_flight.saturating_sub(canceled);
        self.canceled += canceled;

        // Nothing wanted means the view has left the image. Its reads are
        // abandoned above, but what is already drawn is kept: panning back
        // should not have to fetch it again.
        if wanted.is_empty() {
            return;
        }

        let candidates = self
            .slots
            .iter()
            .filter(|(key, _)| !self.retained.contains(*key))
            .filter_map(|(key, slot)| match slot.state {
                SlotState::Ready { bytes, .. } => Some(Candidate {
                    key: *key,
                    last_wanted: slot.last_wanted,
                    bytes,
                }),
                _ => None,
            })
            .collect();
        for key in plan_eviction(candidates, self.resident_bytes, self.budget_bytes) {
            if let Some(slot) = self.slots.remove(&key)
                && let SlotState::Ready { entity, bytes, .. } = slot.state
            {
                commands.entity(entity).despawn();
                self.resident_bytes = self.resident_bytes.saturating_sub(bytes);
            }
        }

        // Blank and failed slots cost no texture memory, but should not grow
        // without bound on a long pan across a large image.
        let empty = self.slots.len() - self.loaded();
        if empty > MAX_EMPTY_SLOTS {
            let cutoff = self.frame.saturating_sub(EMPTY_SLOT_FRAMES);
            self.slots.retain(|key, slot| {
                matches!(slot.state, SlotState::Ready { .. } | SlotState::Loading(_))
                    || self.retained.contains(key)
                    || slot.last_wanted > cutoff
            });
        }
    }

    /// Drop every tile and start over, e.g. after moving to another slice.
    pub fn clear(&mut self, commands: &mut Commands) {
        for (_, entity, _) in self.ready() {
            commands.entity(entity).despawn();
        }
        self.slots.clear();
        self.in_flight = 0;
        self.resident_bytes = 0;
    }

    /// Every resident tile, with its entity and what it keeps beside it.
    pub fn ready(&self) -> impl Iterator<Item = (&K, Entity, &M)> {
        self.slots
            .iter()
            .filter_map(|(key, slot)| match &slot.state {
                SlotState::Ready {
                    entity, material, ..
                } => Some((key, *entity, material)),
                _ => None,
            })
    }

    /// Show the resident tiles `shown` accepts and hide the rest.
    ///
    /// Walked per cache rather than over every tile in the world, so one
    /// image's level cannot hide another image's tiles.
    pub fn show(
        &self,
        tiles: &mut Query<&mut Visibility, impl QueryFilter>,
        shown: impl Fn(&K) -> bool,
    ) {
        for (key, entity, _) in self.ready() {
            if let Ok(mut visibility) = tiles.get_mut(entity) {
                visibility.set_if_neq(if shown(key) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }

    pub fn loaded(&self) -> usize {
        self.ready().count()
    }

    fn failed(&self) -> usize {
        self.slots
            .values()
            .filter(|slot| matches!(slot.state, SlotState::Failed))
            .count()
    }

    /// The cache's line of a source's status.
    pub fn status(&self) -> String {
        let mut line = format!(
            "tiles {} cached ({} MB / {} MB), {} loading",
            self.loaded(),
            self.resident_bytes / (1024 * 1024),
            self.budget_bytes / (1024 * 1024),
            self.in_flight,
        );
        if self.canceled > 0 {
            line.push_str(&format!(", {} canceled", self.canceled));
        }
        let failed = self.failed();
        if failed > 0 {
            line.push_str(&format!(", {failed} failed"));
        }
        line
    }
}

/// A resident tile considered for eviction.
struct Candidate<K> {
    key: K,
    last_wanted: u64,
    bytes: usize,
}

/// Choose which tiles to drop so that `resident` falls within `budget`,
/// least recently wanted first. Returns nothing while the cache fits.
fn plan_eviction<K>(mut candidates: Vec<Candidate<K>>, resident: usize, budget: usize) -> Vec<K> {
    if resident <= budget {
        return Vec::new();
    }
    candidates.sort_unstable_by_key(|c| c.last_wanted);

    let mut freed = 0usize;
    let mut evict = Vec::new();
    for candidate in candidates {
        if resident - freed <= budget {
            break;
        }
        freed += candidate.bytes;
        evict.push(candidate.key);
    }
    evict
}

#[cfg(test)]
mod tests {
    use super::*;

    type Key = (usize, u64);

    fn key(level: usize, ty: u64) -> Key {
        (level, ty)
    }

    fn candidate(level: usize, ty: u64, last_wanted: u64, bytes: usize) -> Candidate<Key> {
        Candidate {
            key: key(level, ty),
            last_wanted,
            bytes,
        }
    }

    #[test]
    fn nothing_is_evicted_while_the_cache_fits() {
        // This is what keeps a zoom out instant: tiles left the viewport but
        // stay resident because there is still room for them.
        let candidates = vec![candidate(0, 0, 1, 100), candidate(0, 1, 2, 100)];
        assert!(plan_eviction(candidates, 200, 1000).is_empty());
    }

    #[test]
    fn the_least_recently_wanted_tile_goes_first() {
        let candidates = vec![
            candidate(0, 0, 30, 100),
            candidate(0, 1, 10, 100),
            candidate(0, 2, 20, 100),
        ];
        assert_eq!(plan_eviction(candidates, 300, 250), vec![key(0, 1)]);
    }

    #[test]
    fn eviction_stops_as_soon_as_the_budget_is_met() {
        let candidates = vec![
            candidate(0, 0, 1, 100),
            candidate(0, 1, 2, 100),
            candidate(0, 2, 3, 100),
            candidate(0, 3, 4, 100),
        ];
        // Needs to free 150, so two tiles suffice and the rest stay cached.
        let evicted = plan_eviction(candidates, 400, 250);
        assert_eq!(evicted, vec![key(0, 0), key(0, 1)]);
    }

    #[test]
    fn an_empty_candidate_list_cannot_loop_forever() {
        // Every resident tile is in use, so nothing can be freed.
        assert!(plan_eviction::<Key>(Vec::new(), 500, 100).is_empty());
    }

    #[test]
    fn the_fetch_pool_is_large_enough_to_matter() {
        // Bevy's default async-compute pool caps at 4 threads, and one blocking
        // tile read occupies a thread for its whole duration.
        const {
            assert!(
                TILE_FETCH_THREADS > 4,
                "a pool this small would serialize tile loading"
            );
        };
        assert_eq!(MAX_IN_FLIGHT, TILE_FETCH_THREADS);
    }

    #[test]
    fn nothing_off_screen_is_requested_while_a_visible_tile_is_loading() {
        // One visible tile outstanding is enough to hold back every prefetch:
        // even the last to be cut off would have shared its bandwidth.
        let visible = [key(3, 0), key(0, 0), key(0, 1)];
        let prefetch = [key(0, 2), key(1, 0)];
        let wanted = request_order(&visible, &prefetch, |k| *k != key(0, 1));
        assert_eq!(wanted, visible);
    }

    #[test]
    fn prefetch_follows_once_every_visible_tile_has_landed() {
        let visible = [key(3, 0), key(0, 0)];
        let prefetch = [key(0, 2), key(1, 0)];
        let wanted = request_order(&visible, &prefetch, |_| true);
        assert_eq!(wanted, [key(3, 0), key(0, 0), key(0, 2), key(1, 0)]);
    }

    #[test]
    fn a_tile_named_twice_is_requested_once_where_first_named() {
        let mut tiers = Tiers::default();
        tiers.extend([key(3, 0), key(0, 0)]);
        tiers.extend([key(0, 0), key(0, 1)]);
        assert_eq!(tiers.order, [key(3, 0), key(0, 0), key(0, 1)]);

        // Nor is a visible tile named again as a prefetch.
        let mut prefetch = tiers.followed_by();
        prefetch.extend([key(0, 1), key(1, 0)]);
        assert_eq!(prefetch.order, [key(1, 0)]);
        assert_eq!(prefetch.seen.len(), 4);
    }

    #[test]
    fn the_overview_comes_first_and_neighbours_last() {
        let view = View {
            center: Vec2::ZERO,
            half: Vec2::ONE,
            margin: Vec2::ONE,
        };
        // A tile per level, and one more for the margin at the frame's own.
        let over = |level: usize, _: &View, half: Vec2| {
            let mut keys = vec![(level, 0)];
            if half.x > 1.0 {
                keys.push((level, 1));
            }
            keys
        };
        let beside = |level: usize| [Some(level + 1), level.checked_sub(1)];
        let (visible, prefetch) = tiers(&[(view, 2)], 5, beside, over);
        assert_eq!(visible.order, [(5, 0), (2, 0)]);
        assert_eq!(prefetch.order, [(2, 1), (3, 0), (1, 0)]);
    }
}
