//! The layers section of the sidebar: what the selected frame draws over the
//! dataset it opened onto, how strongly, and what else could go on top.
//!
//! Every change is a [`PanelRequest`], the same ones a frame's own menu raises,
//! so the two routes cannot disagree about what may be layered. The transparency
//! of a layer is the layer's own [`LayerOpacity`] rather than its dataset's, so
//! the same dataset can be faint over one frame and fully shown in another.

use bevy::prelude::*;
use bevy_feathers::controls::FeathersToolButton;
use bevy_feathers::display::label;
use bevy_feathers::font_styles::InheritableFont;
use bevy_ui_widgets::{Activate, SliderValue};

use crate::app::schedule::{Boot, Stage};
use crate::formats::{EXAMPLES, Example, unopened_examples};
use crate::source::{DataSource, SourceUrl};
use crate::ui::addsource::CustomLoad;
use crate::ui::sidebar::{SectionOrder, SidebarContent};
use crate::view::grid::MAX_LAYERS;
use crate::view::layers::{can_add_layer, stacked_sources, unit_mismatch};
use crate::view::{
    BlocksFrameInput, DatasetRequest, DatasetTarget, FrameLayers, LayerOf, LayerOpacity, Panel,
    PanelRequest, SelectedPanel, ShowsSource,
};
use crate::widgets::{button_text, caption, spawn_accordion, spawn_slider};

/// Straight after View configuration, which acts on the bottom of the same
/// stack.
const SECTION_ORDER: u32 = 15;

/// The opacity sliders run 0..100, as View configuration's does.
const PERCENT: f32 = 100.0;

/// The body the rows are rebuilt into.
#[derive(Component, Clone, Default)]
pub struct LayersBody;

/// Anything spawned into the body, despawned wholesale on a rebuild.
#[derive(Component, Clone, Default)]
pub struct LayersContent;

/// Adds a source to the selected frame's stack, or takes it off.
#[derive(Component, Clone)]
pub struct LayerButton {
    pub panel: Entity,
    pub action: LayerAction,
}

#[derive(Clone, Copy)]
pub enum LayerAction {
    Add(Entity),
    Remove(Entity),
    /// Read a known dataset, by its place in [`EXAMPLES`], and layer it once
    /// it lands.
    AddExample(usize),
}

impl Default for LayerButton {
    fn default() -> Self {
        LayerButton {
            panel: Entity::PLACEHOLDER,
            action: LayerAction::Add(Entity::PLACEHOLDER),
        }
    }
}

/// Sets one layer's transparency.
#[derive(Component, Clone)]
pub struct LayerOpacitySlider {
    /// The layer camera, which is what carries the opacity.
    pub layer: Entity,
}

pub fn spawn_layers_section(mut commands: Commands, content: Query<Entity, With<SidebarContent>>) {
    let Ok(parent) = content.single() else { return };
    let accordion = spawn_accordion(&mut commands, "Layers", true);
    commands
        .entity(accordion.section)
        .insert(SectionOrder(SECTION_ORDER));
    commands.entity(accordion.body).insert(LayersBody);
    commands.entity(parent).add_child(accordion.section);
}

/// Rebuild the rows when the selection, its stack, or the sources on offer
/// change.
///
/// Wholesale, like the layout menu: a stack is at most eight deep and changes
/// only when someone asks it to.
pub fn rebuild_layers(
    mut commands: Commands,
    selected: Res<SelectedPanel>,
    panels: Query<(&ShowsSource, Option<&FrameLayers>), With<Panel>>,
    layer_cameras: Query<&ShowsSource, With<LayerOf>>,
    opacities: Query<(&ShowsSource, &LayerOpacity), With<LayerOf>>,
    sources: Query<(Entity, &DataSource)>,
    urls: Query<&SourceUrl>,
    load: Res<CustomLoad>,
    body: Query<Entity, With<LayersBody>>,
    existing: Query<Entity, With<LayersContent>>,
    mut shown: Local<
        Option<(
            Option<Entity>,
            Vec<Entity>,
            Vec<Entity>,
            usize,
            Option<Entity>,
        )>,
    >,
) {
    let Ok(body) = body.single() else { return };

    let frame = selected
        .0
        .and_then(|panel| panels.get(panel).ok().map(|found| (panel, found)));
    let stack = frame.map_or_else(Vec::new, |(_, (shows, layers))| {
        stacked_sources(shows, layers, &layer_cameras)
    });
    // The cameras as well as the sources: a layer taken off and put back is the
    // same source on a new camera, and a slider left bound to the old one would
    // move nothing.
    let cameras: Vec<Entity> = frame
        .and_then(|(_, (_, layers))| layers)
        .map(|layers| layers.cameras().to_vec())
        .unwrap_or_default();
    let fingerprint = (
        frame.map(|(panel, _)| panel),
        stack.clone(),
        cameras.clone(),
        sources.iter().count(),
        load.loading_onto(),
    );
    if shown.as_ref() == Some(&fingerprint) {
        return;
    }
    *shown = Some(fingerprint);

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let lookup = |entity: Entity| sources.get(entity).ok().map(|(_, data)| data);
    let mut rows = Vec::new();

    let Some((panel, _)) = frame else {
        rows.push(content_caption(&mut commands, "no frame selected"));
        commands.entity(body).add_children(&rows);
        return;
    };
    let Some(base) = stack.first().and_then(|base| lookup(*base)) else {
        return;
    };

    // Topmost first, the way layers are listed wherever layers are listed: the
    // first row is what is drawn over everything else.
    for camera in cameras.iter().rev() {
        let Ok((shows, opacity)) = opacities.get(*camera) else {
            continue;
        };
        let Some(data) = lookup(shows.0) else {
            continue;
        };
        rows.push(layer_row(
            &mut commands,
            panel,
            shows.0,
            data,
            unit_mismatch(base, data),
        ));
        let slider = spawn_slider(&mut commands, opacity.0 * PERCENT, (0.0, PERCENT), 0);
        commands
            .entity(slider)
            .insert((LayersContent, LayerOpacitySlider { layer: *camera }));
        rows.push(slider);
    }
    rows.push(content_caption(
        &mut commands,
        &format!("over {}", base.name),
    ));

    let mut candidates: Vec<(Entity, &DataSource)> = sources
        .iter()
        .filter(|(entity, _)| can_add_layer(&stack, *entity))
        .collect();
    candidates.sort_by_key(|(_, data)| data.layer);
    let opened: Vec<&str> = urls.iter().map(|url| url.0.as_str()).collect();
    let room = stack.len() < MAX_LAYERS;

    let heading = commands
        .spawn_scene(bsn! {
            LayersContent
            label("Add a layer")
            InheritableFont { font_size: { 12.0f32 } }
            Node { margin: { UiRect::top(Val::Px(8.0)) } }
        })
        .id();
    rows.push(heading);
    if load.loading_onto() == Some(panel) {
        rows.push(content_caption(
            &mut commands,
            "Reading a dataset to draw over this one\u{2026}",
        ));
    }
    if !room {
        rows.push(content_caption(
            &mut commands,
            "This frame holds all it can.",
        ));
    }
    for (source, data) in candidates {
        let details = details(&mut commands, data, unit_mismatch(base, data));
        rows.push(candidate_row(
            &mut commands,
            panel,
            LayerAction::Add(source),
            details,
        ));
    }
    // Known datasets not read yet: choosing one reads it and layers it once it
    // lands, so nothing has to be opened somewhere else first.
    for (index, example) in unopened_examples(&opened).filter(|_| room) {
        let details = example_details(&mut commands, example);
        rows.push(candidate_row(
            &mut commands,
            panel,
            LayerAction::AddExample(index),
            details,
        ));
    }

    commands.entity(body).add_children(&rows);
}

fn content_caption(commands: &mut Commands, text: &str) -> Entity {
    let entity = caption(commands, text.to_string());
    commands.entity(entity).insert(LayersContent);
    entity
}

/// A dataset's name, with what kind it is and any unit mismatch under it.
fn details(commands: &mut Commands, data: &DataSource, mismatch: Option<String>) -> Entity {
    let mut lines = vec![data.detail.clone()];
    if let Some(mismatch) = mismatch {
        // Drawn anyway, since nothing rescales a layer, so say why it may not
        // line up rather than leave the two to look aligned by coincidence.
        lines.push(format!("{mismatch}, not rescaled"));
    }
    name_column(commands, data.name.clone(), lines)
}

/// A known dataset's name and kind. What it is measured in is not known until
/// it has been read, so there is no mismatch to warn about yet.
fn example_details(commands: &mut Commands, example: &Example) -> Entity {
    name_column(
        commands,
        example.name.to_string(),
        vec![format!("{}, not loaded yet", example.kind)],
    )
}

/// A name with captions under it.
fn name_column(commands: &mut Commands, name: String, lines: Vec<String>) -> Entity {
    let column = commands
        .spawn_scene(bsn! {
            Node {
                flex_direction: { FlexDirection::Column },
                flex_grow: { 1.0_f32 },
                flex_shrink: { 1.0_f32 },
                row_gap: { Val::Px(1.0) },
            }
            Children [(
                label(name)
                InheritableFont { font_size: { 13.0f32 } }
            )]
        })
        .id();
    let captions: Vec<Entity> = lines
        .into_iter()
        .map(|line| caption(commands, line))
        .collect();
    commands.entity(column).add_children(&captions);
    column
}

fn row(commands: &mut Commands) -> Entity {
    commands
        .spawn_scene(bsn! {
            LayersContent
            Node {
                width: { Val::Percent(100.0) },
                align_items: { AlignItems::Center },
                column_gap: { Val::Px(6.0) },
                padding: { UiRect::vertical(Val::Px(4.0)) },
            }
        })
        .id()
}

fn button(commands: &mut Commands, glyph: &str, action: LayerButton) -> Entity {
    let glyph = glyph.to_string();
    commands
        .spawn_scene(bsn! {
            @FeathersToolButton {
                @caption: { bsn_list![button_text(glyph)] }
            }
            BlocksFrameInput
            template_value(action)
            Node { flex_shrink: { 0.0_f32 } }
        })
        .id()
}

fn layer_row(
    commands: &mut Commands,
    panel: Entity,
    source: Entity,
    data: &DataSource,
    mismatch: Option<String>,
) -> Entity {
    let row = row(commands);
    let details = details(commands, data, mismatch);
    let remove = button(
        commands,
        "Remove",
        LayerButton {
            panel,
            action: LayerAction::Remove(source),
        },
    );
    commands.entity(row).add_children(&[details, remove]);
    row
}

fn candidate_row(
    commands: &mut Commands,
    panel: Entity,
    action: LayerAction,
    details: Entity,
) -> Entity {
    let row = row(commands);
    let add = button(commands, "+", LayerButton { panel, action });
    commands.entity(row).add_children(&[add, details]);
    row
}

/// Feathers buttons trigger [`Activate`] rather than carrying an
/// `Interaction`, so this is an observer.
pub fn on_layer_button(
    activate: On<Activate>,
    buttons: Query<&LayerButton>,
    mut requests: MessageWriter<PanelRequest>,
    mut datasets: MessageWriter<DatasetRequest>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let panel = button.panel;
    match button.action {
        LayerAction::Add(source) => {
            requests.write(PanelRequest::AddLayer { panel, source });
        }
        LayerAction::Remove(source) => {
            requests.write(PanelRequest::RemoveLayer { panel, source });
        }
        LayerAction::AddExample(index) => {
            if let Some(example) = EXAMPLES.get(index) {
                datasets.write(DatasetRequest {
                    url: example.url.to_string(),
                    target: DatasetTarget::Layer(panel),
                });
            }
        }
    }
}

/// Write a dragged slider through to its layer's opacity.
///
/// One way only, unlike View configuration's slider: each slider is bound to
/// one layer and built with its value, and the rows are rebuilt whenever the
/// stack or the selection changes, so no selection moves underneath it.
pub fn apply_layer_opacity(
    sliders: Query<(&LayerOpacitySlider, &SliderValue), Changed<SliderValue>>,
    mut opacities: Query<&mut LayerOpacity>,
) {
    for (slider, value) in &sliders {
        let wanted = LayerOpacity((value.0 / PERCENT).clamp(0.0, 1.0));
        if let Ok(mut opacity) = opacities.get_mut(slider.layer) {
            opacity.set_if_neq(wanted);
        }
    }
}

pub struct LayersPlugin;

impl Plugin for LayersPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_layer_button)
            .add_systems(Update, rebuild_layers.in_set(Stage::ControlsBuild))
            .add_systems(Update, apply_layer_opacity.in_set(Stage::ControlsApply))
            .add_systems(Startup, spawn_layers_section.in_set(Boot::DockContent));
    }
}
