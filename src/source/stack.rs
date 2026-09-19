//! Sources that hold a stack of slices, and which one is showing.
//!
//! A volumetric image is a pile of sections with one on screen at a time. What
//! makes that worth a component of its own, rather than a field inside the
//! image streamer, is that nothing outside the format should have to know it is
//! an image to page through it: the sidebar's control and the frame's keys both
//! read [`SliceStack`] off the source entity, and a format that has one
//! advertises it the way a point cloud advertises a point size.

use bevy::prelude::*;

/// A source that shows one slice of a stack at a time.
///
/// Sources without one are flat, and nothing offers to page through them.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceStack {
    /// Which slice is showing, counted from zero.
    pub current: u64,
    /// How many there are. Never zero: a source with one slice is flat and
    /// should not carry this at all.
    pub count: u64,
}

impl SliceStack {
    /// A stack showing its middle slice.
    ///
    /// The ends of a specimen are where the block runs out — the first and last
    /// sections are usually empty or nearly so — and opening on an empty slice
    /// reads as a dataset that failed to load. The middle is where the tissue
    /// is. Anything that knows better, such as a slice named on the command
    /// line, says so with [`Self::go_to`].
    pub fn new(count: u64) -> Self {
        let count = count.max(1);
        SliceStack {
            current: count / 2,
            count,
        }
    }

    /// The last slice's index.
    pub fn last(&self) -> u64 {
        self.count.saturating_sub(1)
    }

    /// Move `delta` slices, stopping at either end.
    ///
    /// Clamped rather than wrapped: paging off the end of a specimen and
    /// arriving back at its other end reads as the control having lost its
    /// place.
    pub fn step(&mut self, delta: i64) {
        let wanted = self.current as i64 + delta;
        self.current = wanted.clamp(0, self.last() as i64) as u64;
    }

    /// Go to a slice named directly, as a control does.
    pub fn go_to(&mut self, slice: u64) {
        self.current = slice.min(self.last());
    }

    /// How it reads in a status line or beside a control: counted from one,
    /// because the first slice of a specimen is the first, not the zeroth.
    pub fn label(&self) -> String {
        format!("slice {} of {}", self.current + 1, self.count)
    }
}

/// A stack that can also be laid out with every slice at once, and whether it
/// is.
///
/// Sectioned point clouds offer it; a volumetric image does not, since its
/// slices all sit in one place. Carried beside [`SliceStack`] so the frame's
/// `G` key and the sidebar's checkbox switch it without knowing the format.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceGrid(pub bool);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_opens_in_the_middle_of_the_specimen() {
        // The first section of a block is usually empty, and opening on it
        // reads as a dataset that did not load.
        assert_eq!(SliceStack::new(142).current, 71);
        assert_eq!(SliceStack::new(3).current, 1);
        // With one slice there is nowhere else to be.
        assert_eq!(SliceStack::new(1).current, 0);
    }

    #[test]
    fn paging_stops_at_both_ends() {
        // Wrapping would read as the control losing its place in a specimen.
        let mut stack = SliceStack::new(142);
        stack.go_to(0);
        stack.step(-1);
        assert_eq!(stack.current, 0);

        stack.go_to(141);
        stack.step(1);
        assert_eq!(stack.current, 141);
    }

    #[test]
    fn paging_moves_by_as_much_as_it_is_asked_for() {
        let mut stack = SliceStack::new(142);
        stack.go_to(0);
        stack.step(10);
        assert_eq!(stack.current, 10);
        stack.step(-4);
        assert_eq!(stack.current, 6);
    }

    #[test]
    fn a_slice_beyond_the_end_lands_on_the_last_one() {
        let mut stack = SliceStack::new(10);
        stack.go_to(99);
        assert_eq!(stack.current, 9);
    }

    #[test]
    fn a_stack_counts_from_one_where_anyone_can_see_it() {
        // Zero-based inside, one-based on screen: the first section of a
        // specimen is the first.
        let mut stack = SliceStack::new(142);
        stack.go_to(0);
        assert_eq!(stack.label(), "slice 1 of 142");
        let mut last = stack;
        last.go_to(last.last());
        assert_eq!(last.label(), "slice 142 of 142");
    }

    #[test]
    fn a_stack_is_never_empty() {
        // A source with nothing in it would divide the control by zero.
        let stack = SliceStack::new(0);
        assert_eq!(stack.count, 1);
        assert_eq!(stack.last(), 0);
    }
}
