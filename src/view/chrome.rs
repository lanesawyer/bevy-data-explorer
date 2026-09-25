//! The furniture drawn over the grid: the rules between cells, the outline
//! round the selected frame, and each frame's row of buttons.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy_feathers::controls::{ButtonVariant, FeathersToolButton};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};

use crate::app::theme::token;
use crate::widgets::space;
use crate::widgets::{BlocksFrameInput, Icon, button_icon, patch_node};
use bevy_ui_widgets::Activate;

use super::grid::{MAX_COLUMNS, MAX_PANELS, MAX_ROWS};
use super::{FrameArea, Panel, PanelRequest, SelectedPanel};

/// Width of the rule drawn between panels, in logical pixels.
pub(super) const DIVIDER_PX: f32 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Axis {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Component, Clone, Default)]
pub struct PanelDivider {
    pub(super) axis: Axis,
    pub(super) ordinal: usize,
}

/// The outline drawn around the selected frame.
#[derive(Component, Clone, Default)]
pub struct SelectionBorder;

pub(super) fn spawn_dividers(mut commands: Commands) {
    let mut rule = |axis: Axis, ordinal: usize| {
        commands.spawn_scene(bsn! {
            Node {
                position_type: { PositionType::Absolute },
                display: { Display::None },
            }
            ThemeBackgroundColor({ token::DIVIDER })
            // Decoration, like the selection outline: it must not swallow
            // pointer events along a frame's edge.
            template_value(Pickable::IGNORE)
            PanelDivider { axis: { axis }, ordinal: { ordinal } }
        });
    };
    for ordinal in 0..MAX_COLUMNS - 1 {
        rule(Axis::Vertical, ordinal);
    }
    for ordinal in 0..MAX_ROWS - 1 {
        rule(Axis::Horizontal, ordinal);
    }
}

/// Spawn the full set of rules the grid can ever need, and let
/// [`update_viewports`] show only the ones the current layout uses.
/// Spawn the outline that marks the selected frame.
pub(super) fn spawn_selection_border(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        SelectionBorder
        // Decoration only. It covers the whole cell and draws above the
        // frame's own chrome, and picking blocks by default — which swallowed
        // every pointer event over the selected frame. `Interaction` passes
        // through by default, so the buttons that use it went on working and
        // only the picking-driven ones appeared dead.
        template_value(Pickable::IGNORE)
        Node {
            position_type: { PositionType::Absolute },
            border: { UiRect::all(Val::Px(SELECTION_PX)) },
            display: { Display::None },
        }
        GlobalZIndex({ SELECTION_Z })
        ThemeBorderColor({ token::SELECTION })
    });
}

/// Keep the outline over the selected frame, and pick one if none is selected.
pub fn update_selection_border(
    mut selected: ResMut<SelectedPanel>,
    area: Res<FrameArea>,
    panels: Query<(Entity, &Panel)>,
    mut border: Query<&mut Node, With<SelectionBorder>>,
) {
    // A closed frame leaves the selection dangling, so it falls back to the
    // first frame left — or to nothing, once the last one is closed.
    let still_there = selected.0.is_some_and(|entity| panels.get(entity).is_ok());
    if !still_there {
        selected.0 = panels
            .iter()
            .min_by_key(|(_, panel)| panel.index)
            .map(|(entity, _)| entity);
    }

    // The outline says which of several frames the controls and the keys are
    // talking to. With one frame there is nothing to say: it is drawn round the
    // whole grid, where it reads as a border on the window rather than as an
    // answer to a question nobody asked.
    let several = panels.iter().count() > 1;

    for node in &mut border {
        let Some(panel) = selected
            .0
            .and_then(|e| panels.get(e).ok())
            .filter(|_| several)
        else {
            patch_node(node, |node| node.display = Display::None);
            continue;
        };
        let cell = area.cell(panels.iter().count(), panel.1.index);
        patch_node(node, |node| {
            node.display = Display::Flex;
            node.left = Val::Px(cell.min.x);
            node.top = Val::Px(cell.min.y);
            node.width = Val::Px(cell.width());
            node.height = Val::Px(cell.height());
        });
    }
}

pub(super) const SELECTION_PX: f32 = 2.0;

/// Draw order for the selection outline.
///
/// A cell's left and top borders fall exactly on the rules between cells, since
/// a border is drawn inside the node while the rule sits just outside it. The
/// outline and the rules are separate UI roots, so nothing orders them
/// implicitly and the rule would cover the shared edges — leaving every frame
/// except the top-left one outlined on two sides only.
pub(super) const SELECTION_Z: i32 = 1;

pub(super) const BUTTON_PX: f32 = 22.0;
/// Space between the buttons over a frame, and between them and the status
/// under them.
pub(super) const CHROME_GAP: f32 = space::CONTROLS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelAction {
    Duplicate,
    Close,
}

impl PanelAction {
    /// Only the two that manage the frame itself, left to right. Inspecting
    /// sits with the dataset's name in the header, where it reads as being
    /// about the data rather than about the frame.
    const ALL: [PanelAction; 2] = [PanelAction::Duplicate, PanelAction::Close];

    fn icon(self) -> Icon {
        match self {
            PanelAction::Duplicate => Icon::CopyPlus,
            PanelAction::Close => Icon::X,
        }
    }
}

/// A button in a panel's corner.
#[derive(Component, Clone)]
pub struct PanelButton {
    pub panel: Entity,
    pub action: PanelAction,
}

impl Default for PanelButton {
    fn default() -> Self {
        // Scenes are patches over defaults, so this only has to be a value the
        // real one is written over.
        PanelButton {
            panel: Entity::PLACEHOLDER,
            action: PanelAction::Duplicate,
        }
    }
}

/// The chrome every corner button shares.
///
/// Frame controls sit over imagery, so they take the plain variant and let the
/// header's own panel supply the contrast.
fn variant_for(_action: PanelAction) -> ButtonVariant {
    ButtonVariant::Normal
}

/// The buttons that manage a frame, for the end of its header row.
pub(super) fn spawn_corner_buttons(commands: &mut Commands, panel: Entity) -> Vec<Entity> {
    PanelAction::ALL
        .into_iter()
        .map(|action| {
            let icon = action.icon();
            commands
                .spawn_scene(bsn! {
                    @FeathersToolButton {
                        @caption: { bsn_list![button_icon(icon)] },
                        @variant: { variant_for(action) }
                    }
                    BlocksFrameInput
                    Node { flex_shrink: { 0.0_f32 } }
                    PanelButton { panel: { panel }, action: { action } }
                })
                .id()
        })
        .collect()
}

/// Hold the duplicate buttons while the grid has no room for another frame.
pub fn sync_duplicate_buttons(
    mut commands: Commands,
    panels: Query<(), With<Panel>>,
    buttons: Query<(Entity, &PanelButton, Has<InteractionDisabled>)>,
) {
    let full = panels.iter().count() >= MAX_PANELS;
    for (entity, button, disabled) in &buttons {
        if button.action != PanelAction::Duplicate || disabled == full {
            continue;
        }
        if full {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

/// Duplicate a panel when its button is pressed.
///
/// The copy inherits the source panel's current center and zoom rather than its
/// fitted defaults, so a duplicate starts as the same view and can then be
/// driven somewhere else.
pub fn panel_buttons(
    activate: On<Activate>,
    buttons: Query<&PanelButton>,
    mut requests: MessageWriter<PanelRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    requests.write(match button.action {
        PanelAction::Duplicate => PanelRequest::Duplicate(button.panel),
        PanelAction::Close => PanelRequest::Close(button.panel),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Span a cell's left border occupies, and the rule drawn at that column.
    fn left_border_span(col: usize, cell_x: f32) -> (f32, f32) {
        let left = cell_x * col as f32;
        (left, left + SELECTION_PX)
    }

    fn rule_span(ordinal: usize, cell_x: f32) -> (f32, f32) {
        let left = cell_x * (ordinal + 1) as f32;
        (left, left + DIVIDER_PX)
    }

    fn overlaps(a: (f32, f32), b: (f32, f32)) -> bool {
        a.0 < b.1 && b.0 < a.1
    }

    #[test]
    fn a_cells_left_border_lands_on_the_rule_beside_it() {
        // This is why the outline needs an explicit draw order: for every
        // column but the first, the border and the rule occupy the same pixels.
        let cell = 400.0;
        assert!(overlaps(left_border_span(1, cell), rule_span(0, cell)));
        assert!(overlaps(left_border_span(2, cell), rule_span(1, cell)));
        // The first column has no rule to its left, which is why that frame
        // looked correctly outlined while the others did not.
        assert!(!overlaps(left_border_span(0, cell), rule_span(0, cell)));
    }

    #[test]
    fn the_outline_draws_above_the_rules() {
        // Rules carry no explicit index, so they sit at zero.
        const { assert!(SELECTION_Z > 0) };
    }

    #[test]
    fn every_action_has_its_own_icon() {
        let icons: std::collections::HashSet<Icon> =
            PanelAction::ALL.iter().map(|a| a.icon()).collect();
        assert_eq!(icons.len(), PanelAction::ALL.len());
    }
}
