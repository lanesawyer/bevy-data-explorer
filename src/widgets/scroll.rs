//! Areas that scroll on their own terms.
//!
//! A list that scrolls inside something that scrolls, and an area that scrolls
//! both ways.
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

/// Marks an area that scrolls both ways, where holding shift turns the wheel
/// sideways.
///
/// Not `bevy_ui_widgets`' `ScrollArea`, which is what an area that only scrolls
/// down wants: that widget's observer is registered with Feathers, before
/// anything here, and would already have moved the area down the page by the
/// time this one could notice the shift. So an area that wants the shift reads
/// the wheel itself rather than correcting it afterwards.
///
/// It is still a scrolling area as far as [`super::scrollbar`] is concerned,
/// and gets its bars the same way everything else does.
#[derive(Component, Clone, Default)]
#[require(ScrollPosition)]
pub struct ScrollBoth;

/// Scroll an area both ways, with shift turning the wheel sideways.
///
/// Shift is what a browser and a spreadsheet both use, and it is the only way
/// across for a wheel with no sideways axis of its own — which is most of
/// them.
pub fn on_both_scroll(
    mut scroll: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut areas: Query<(&ComputedNode, &mut ScrollPosition), With<ScrollBoth>>,
) {
    let Ok((computed, mut position)) = areas.get_mut(scroll.entity) else {
        return;
    };
    let wheel = Vec2::new(scroll.x, scroll.y)
        * match scroll.unit {
            MouseScrollUnit::Line => MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
            MouseScrollUnit::Pixel => 1.0,
        };
    let delta = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        Vec2::new(wheel.y, 0.0)
    } else {
        wheel
    };

    // The bars take a lane out of the area, so what is on screen is what is
    // left after them rather than the whole node.
    let scale = computed.inverse_scale_factor;
    let visible = (computed.size() - computed.scrollbar_size) * scale;
    let range = (computed.content_size() * scale - visible).max(Vec2::ZERO);
    position.x = scrolled(position.x, delta.x, range.x);
    position.y = scrolled(position.y, delta.y, range.y);
    scroll.propagate(false);
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

    /// Where the wheel leaves an area, given what shift does to it.
    fn wheel(shift: bool, wheel: Vec2) -> Vec2 {
        if shift {
            Vec2::new(wheel.y, 0.0)
        } else {
            wheel
        }
    }

    #[test]
    fn shift_turns_the_wheel_sideways() {
        // A wheel with no sideways axis of its own, which is most of them.
        assert_eq!(wheel(true, Vec2::new(0.0, -3.0)), Vec2::new(-3.0, 0.0));
        // And without it, straight down as ever.
        assert_eq!(wheel(false, Vec2::new(0.0, -3.0)), Vec2::new(0.0, -3.0));
    }

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
