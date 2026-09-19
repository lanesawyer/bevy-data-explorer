//! A list that scrolls inside something that scrolls.
//!
//! `bevy_ui_widgets`' `ScrollArea` keeps every wheel turn over it, including
//! the ones it has no room left to use. Nested in the sidebar, that made the
//! sidebar stop scrolling wherever the pointer crossed a list, even a list
//! that was already at its end. A [`ScrollList`] takes the wheel only while
//! it can move, and passes it on to whatever scrolls around it otherwise.

use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::events::{Pointer, Scroll};
use bevy::prelude::*;

/// Marks a node that scrolls vertically up to its `max_height`, and hands the
/// wheel on at either end.
#[derive(Component, Clone, Default)]
#[require(ScrollPosition)]
pub struct ScrollList;

/// A column that grows to `max_px` and scrolls beyond it.
pub fn scroll_list(max_px: f32) -> impl Scene {
    bsn! {
        ScrollList
        Node {
            flex_direction: { FlexDirection::Column },
            width: { Val::Percent(100.0) },
            max_height: { Val::Px(max_px) },
            row_gap: { Val::Px(4.0) },
            overflow: { Overflow::scroll_y() },
        }
    }
}

pub fn on_list_scroll(
    mut scroll: On<Pointer<Scroll>>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<ScrollList>>,
) {
    let Ok((computed, mut position)) = lists.get_mut(scroll.entity) else {
        return;
    };
    let delta = scroll.y
        * match scroll.unit {
            MouseScrollUnit::Line => MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
            MouseScrollUnit::Pixel => 1.0,
        };
    let visible = computed.size().y * computed.inverse_scale_factor;
    let content = computed.content_size().y * computed.inverse_scale_factor;
    let wanted = scrolled(position.y, delta, content - visible);
    if wanted != position.y {
        position.y = wanted;
        scroll.propagate(false);
    }
}

/// Where a wheel turn of `delta` leaves a list at `from` that can move as far
/// as `range`.
fn scrolled(from: f32, delta: f32, range: f32) -> f32 {
    (from - delta).clamp(0.0, range.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_that_fits_never_moves() {
        assert_eq!(scrolled(0.0, -40.0, -12.0), 0.0);
    }

    #[test]
    fn a_list_at_its_end_stays_there() {
        // So the wheel is handed on rather than kept.
        assert_eq!(scrolled(100.0, -40.0, 100.0), 100.0);
        assert_eq!(scrolled(0.0, 40.0, 100.0), 0.0);
    }

    #[test]
    fn a_list_moves_as_far_as_its_end_and_no_further() {
        assert_eq!(scrolled(80.0, -40.0, 100.0), 100.0);
    }
}
