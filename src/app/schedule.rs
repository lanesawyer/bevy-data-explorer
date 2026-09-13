//! The one place the frame's ordering is written down.
//!
//! Systems used to be ordered by the chain they happened to be listed in, which
//! meant the order lived in `main` and every plugin had to be registered there
//! to take part. Here each stage is a named set instead, so a plugin declares
//! which stage it belongs to and nothing needs to know what else is in it.
//!
//! The stages run in the order they are declared, and that is the only ordering
//! in the app: no system orders itself against another module's system. What
//! follows is why each boundary is where it is, because an ordering nobody can
//! explain is an ordering that gets broken.

use bevy::prelude::*;

use crate::source::hover::HoverProbing;

/// The stages of a frame, in the order they run.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Which control owns the keyboard is settled. First, because a text field
    /// with focus takes the keys away from the frame shortcuts, and several of
    /// those run before any dock or control does — typing a URL would otherwise
    /// reset a view on `r` and toggle channels on every digit.
    Focus,
    /// Docks read the pointer: dragging an edge, opening, collapsing. Their new
    /// sizes are settled before anything measures against them.
    DockInput,
    /// The area available to frames is reset to the whole window.
    FrameArea,
    /// Each dock subtracts the space it occupies. Between `FrameArea` and here
    /// is the only window in which the area means "the whole window", which is
    /// why the reset is a stage of its own rather than the first dock's job.
    DockReserve,
    /// Frames are opened, closed and renumbered.
    Frames,
    /// Chrome that exists per frame is spawned or despawned to match. After
    /// `Frames` because it is spawned against the frames that now exist.
    FrameChrome,
    /// Pointer input is routed to a frame, and the cameras take their viewports
    /// from what the docks left over.
    Viewports,
    /// Frame chrome and dock geometry are placed against the viewports that
    /// `Viewports` just settled.
    Chrome,
    /// Controls read the pointer and record their own state, before anything
    /// rebuilds them and throws that state away.
    ControlsRead,
    /// Controls are spawned and despawned to match what they are showing.
    ControlsBuild,
    /// Controls are positioned and synced to the values they display. After
    /// `ControlsBuild` so a control spawned this frame is placed this frame,
    /// rather than flashing unplaced for one frame.
    ControlsPlace,
    /// Controls write through to the source they act on. Last of the control
    /// stages so a source sees a whole frame's worth of edits at once.
    ControlsApply,
    /// The format plugins stream. After `ControlsApply` so a filter changed
    /// this frame is applied this frame rather than next.
    Sources,
    /// The grid works out where the pointer is and which frame it is over.
    HoverProbe,
    /// Everything that only reads what this frame decided: overlay text and
    /// tooltips. Last, so it never shows a value from the previous frame.
    Overlay,
}

/// The stages of startup, in the order they run.
///
/// Startup is ordered for a different reason than `Update`: each stage spawns
/// entities the next one queries for, and the sync point between stages is what
/// makes them visible.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Boot {
    /// The window takes its size, so everything laid out against it is laid out
    /// against the real one.
    Window,
    /// The shell: the UI camera, the docks, and the furniture drawn over the
    /// grid.
    Shell,
    /// One frame per registered source.
    Frames,
    /// Contents of the docks, which hang off shell entities spawned above.
    DockContent,
    /// Dock contents are put in their stated order, now that every plugin has
    /// contributed whatever sections it offers.
    DockOrder,
}

/// Declare the stage order.
///
/// [`HoverProbing`] sits between [`Stage::HoverProbe`] and [`Stage::Overlay`]
/// rather than being a variant here, because it is the surface a format plugin
/// implements and predates these stages.
pub fn configure(app: &mut App) {
    app.configure_sets(
        Startup,
        (
            Boot::Window,
            Boot::Shell,
            Boot::Frames,
            Boot::DockContent,
            Boot::DockOrder,
        )
            .chain(),
    )
    .configure_sets(
        Update,
        (
            Stage::Focus,
            Stage::DockInput,
            Stage::FrameArea,
            Stage::DockReserve,
            Stage::Frames,
            Stage::FrameChrome,
            Stage::Viewports,
            Stage::Chrome,
            Stage::ControlsRead,
            Stage::ControlsBuild,
            Stage::ControlsPlace,
            Stage::ControlsApply,
            Stage::Sources,
            Stage::HoverProbe,
            HoverProbing,
            Stage::Overlay,
        )
            .chain(),
    );
}
