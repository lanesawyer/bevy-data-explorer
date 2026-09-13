//! The one place the frame's ordering is written down.
//!
//! Systems used to be ordered by the chain they happened to be listed in, which
//! meant the order lived in `main` and every plugin had to be registered there
//! to take part. Here each stage is a named set instead, so a plugin declares
//! which stage it belongs to and nothing needs to know what else is in it.
//!
//! The stages run in the order they are declared. What follows is why each
//! boundary is where it is; an ordering nobody can explain is an ordering that
//! gets broken.

use bevy::prelude::*;

use crate::source::hover::HoverProbing;

/// The stages of a frame, in the order they run.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Stage {
    /// The docks resize and claim their space. Everything that places a frame
    /// or its chrome measures against what is left, so this is first.
    Docks,
    /// Frames are opened, closed and renumbered, and the cameras take their
    /// viewports from whatever the docks left over.
    Layout,
    /// Chrome that sits over a frame — the overlay, the source menus, the dock
    /// contents — placed against the geometry `Layout` just settled.
    Chrome,
    /// Controls in the docks, which read the selection and write through to the
    /// source. After `Chrome` because some of them position menus that `Chrome`
    /// builds.
    Controls,
    /// The format plugins stream. After `Controls` so a filter changed this
    /// frame is applied this frame rather than next.
    Sources,
    /// The grid works out where the pointer is and on which frame.
    HoverProbe,
    /// The overlay reads what the sources and the hover probe reported. Last,
    /// because it only ever reads.
    Overlay,
}

/// Declare the stage order.
///
/// [`HoverProbing`] sits between [`Stage::HoverProbe`] and [`Stage::Overlay`]
/// rather than being a variant here, because it is part of the surface a format
/// plugin implements and predates these stages.
pub fn configure(app: &mut App) {
    app.configure_sets(
        Update,
        (
            Stage::Docks,
            Stage::Layout,
            Stage::Chrome,
            Stage::Controls,
            Stage::Sources,
            Stage::HoverProbe,
            HoverProbing,
            Stage::Overlay,
        )
            .chain(),
    );
}
