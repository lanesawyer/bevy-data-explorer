//! Writing to UI components only when the value differs.
//!
//! Chrome that follows the window or a frame is synced every frame, and a
//! plain write through `Mut` marks the component changed even when it lands
//! the same value. A changed `Node` sends its whole tree back through layout
//! and a changed `Text` back through shaping, so a sync that always writes
//! keeps the UI laying itself out when nothing on screen has moved.

use bevy::prelude::*;

/// Apply `edit` to a copy of the node and store it only if it came out
/// different.
pub fn patch_node(mut node: Mut<Node>, edit: impl FnOnce(&mut Node)) {
    let mut next = node.clone();
    edit(&mut next);
    node.set_if_neq(next);
}

/// Show or hide one entity's node, if it has one.
pub fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    if let Ok(node) = nodes.get_mut(entity) {
        patch_node(node, |node| node.display = display(shown));
    }
}

pub fn display(shown: bool) -> Display {
    if shown { Display::Flex } else { Display::None }
}

/// Set a text's string, leaving it untouched if it already reads that.
pub fn set_text(mut text: Mut<Text>, wanted: &str) {
    if text.0 != wanted {
        text.0 = wanted.to_string();
    }
}
