//! The reads a streaming cache has under way: started in the order the view
//! wants them, capped, and given up on as soon as the view moves off them.
//!
//! Tile pyramids and octrees keep different things once a read lands —
//! textures under a byte budget, point meshes under a point budget swapped in
//! a generation at a time — but ask for them the same way, and this is that
//! half. Dropping a read's handle aborts the request behind it, so giving up
//! on one is forgetting it.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::app::net::Fetching;

pub struct Reads<K, O> {
    loading: HashMap<K, Fetching<O>>,
    /// Reads at most under way at once.
    limit: usize,
    /// Reads given up on because the view moved off them, for the status
    /// line: it is the number that says whether panning is costing anything.
    canceled: usize,
}

impl<K: Copy + Eq + Hash, O: Send + 'static> Reads<K, O> {
    pub fn new(limit: usize) -> Self {
        Reads {
            loading: HashMap::new(),
            limit,
            canceled: 0,
        }
    }

    /// Start a read for each of `wanted`, in order, that is neither `held`
    /// already nor under way, until the limit is reached.
    pub fn start(
        &mut self,
        wanted: &[K],
        held: impl Fn(&K) -> bool,
        mut read: impl FnMut(K) -> Fetching<O>,
    ) {
        for &key in wanted {
            if self.loading.len() >= self.limit {
                break;
            }
            if held(&key) || self.loading.contains_key(&key) {
                continue;
            }
            self.loading.insert(key, read(key));
        }
    }

    /// Give up on every read not in `keep`.
    pub fn abandon(&mut self, keep: &HashSet<K>) {
        let before = self.loading.len();
        self.loading.retain(|key, _| keep.contains(key));
        self.canceled += before - self.loading.len();
    }

    /// Take every read that has finished.
    pub fn finished(&mut self) -> Vec<(K, O)> {
        let mut finished = Vec::new();
        self.loading.retain(|key, read| match read.take() {
            Some(outcome) => {
                finished.push((*key, outcome));
                false
            }
            None => true,
        });
        finished
    }

    /// Count a finished read as given up on: it stopped before doing any work.
    pub fn gave_up(&mut self) {
        self.canceled += 1;
    }

    /// Drop every read under way, as when starting over on another slice.
    pub fn clear(&mut self) {
        self.loading.clear();
    }

    #[cfg(test)]
    fn is_loading(&self, key: &K) -> bool {
        self.loading.contains_key(key)
    }

    pub fn in_flight(&self) -> usize {
        self.loading.len()
    }

    pub fn canceled(&self) -> usize {
        self.canceled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::net::fetching;

    fn never() -> Fetching<()> {
        fetching(std::future::pending())
    }

    #[test]
    fn reads_start_in_order_up_to_the_limit_skipping_what_is_held() {
        let mut reads = Reads::new(2);
        reads.start(&[0, 1, 2, 3], |key| *key == 0, |_| never());
        assert!(!reads.is_loading(&0), "already held");
        assert!(reads.is_loading(&1) && reads.is_loading(&2));
        assert!(!reads.is_loading(&3), "past the limit");

        // A read already under way is not started twice.
        reads.abandon(&HashSet::from([2]));
        reads.start(&[2, 3], |_| false, |_| never());
        assert_eq!(reads.in_flight(), 2);
    }

    #[test]
    fn a_read_the_view_moved_off_is_dropped_and_counted() {
        let mut reads = Reads::new(4);
        reads.start(&[0, 1, 2], |_| false, |_| never());
        reads.abandon(&HashSet::from([1]));
        assert_eq!(reads.in_flight(), 1);
        assert_eq!(reads.canceled(), 2);
    }
}
