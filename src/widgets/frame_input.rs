use bevy::prelude::*;

/// Marks interactive chrome that swallows pointer input before a frame sees it.
///
/// Needed because chrome can overlap the grid — the sidebar's drag handle
/// straddles its own edge — so position alone cannot decide who gets the drag.
#[derive(Component, Clone, Default)]
pub struct BlocksFrameInput;
