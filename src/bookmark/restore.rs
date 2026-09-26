//! Putting a [`Bookmark`] back on screen.
//!
//! Restoring goes through the same doors opening a dataset by hand does:
//! each address is recognized by `discover` and registered with
//! `spawn_discovered`, and a dataset already open is reused rather than read
//! again. The reads run at once rather than one after another, and the frames
//! are laid out only when every one has landed or failed, so a slow dataset
//! cannot leave the grid half built and then reshuffle it.
//!
//! What a dataset was showing is put back as it becomes possible, which is not
//! all at once: a slice can be set as soon as the source exists, but cell
//! filters have to wait for the catalog's service to replace the format's
//! placeholder properties, or the replacement would throw them away.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy::ecs::query::QueryData;
use bevy::prelude::*;

use super::BookmarkNotice;
use super::capture::saved_address;
use super::snapshot::{
    Bookmark, CellsState, SourceState, apply_cells, apply_channels, apply_slice, apply_table,
    ask_saved_spans, clamp_point_size, genes_missing,
};
use crate::app::net::{Fetching, fetching};
use crate::app::theme::Palette;
use crate::catalog::RegionFocus;
use crate::catalog::cells::Described;
use crate::catalog::genes::GeneService;
use crate::formats::discover::{self, Discovered};
use crate::formats::{LoadSettings, spawn_discovered};
use crate::render::points::SourcePointSize;
use crate::render::settings::SourceOpacity;
use crate::source::channels::SourceChannels;
use crate::source::genes::{GeneSearch, ReadsGenes};
use crate::source::properties::{CellColumns, CellProperties, ColorOverrides, PropertyState};
use crate::source::stack::{SliceGrid, SliceStack};
use crate::source::table::{ColumnWidths, HiddenColumns, TableFilters, TablePaging, TableSort};
use crate::source::volume::SourceVolume;
use crate::source::{DataSource, SourceExtent, SourceUrl};
use crate::view::grid::{MAX_LAYERS, MAX_PANELS};
use crate::view::layers::spawn_layer;
use crate::view::link::Linked;
use crate::view::{
    FrameArea, FrameRegion, LayerOpacity, Orbit, Panel, SelectMode, SelectedPanel, View,
    spawn_browse_panel, spawn_panel,
};

/// How long a dataset's cell settings wait for its properties before being
/// given up on. A service that has not answered by then is not going to.
const CELLS_PATIENCE_SECS: f32 = 120.0;

/// A bookmark being restored.
#[derive(Resource)]
pub struct Restoring {
    /// Tells a registration finishing late which restore it belonged to, so
    /// one replaced by a newer restore writes into nothing.
    id: u64,
    bookmark: Bookmark,
    slots: Vec<Slot>,
    begun: bool,
}

enum Slot {
    Reading(Fetching<Result<Discovered, String>>),
    /// Read, and queued to be registered as a source.
    Registering,
    Open(Entity),
    Failed(String),
}

impl Restoring {
    pub fn new(bookmark: Bookmark) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Restoring {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            bookmark,
            slots: Vec::new(),
            begun: false,
        }
    }

    fn open(&self, index: usize) -> Option<Entity> {
        match self.slots.get(index) {
            Some(Slot::Open(entity)) => Some(*entity),
            _ => None,
        }
    }
}

/// What a source still has to have put back.
#[derive(Component)]
pub struct PendingSettings {
    state: SourceState,
    /// When it was asked for, in seconds since startup.
    since: f32,
    /// Whether the bookmark's genes have been asked to be added.
    genes_asked: bool,
    /// Whether the table's saved spans have been asked to be worked out.
    spans_asked: bool,
}

/// What a restore needs of a source to put its table back.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct TableAccess {
    paging: Option<&'static mut TablePaging>,
    filters: Option<&'static mut TableFilters>,
    sort: Option<&'static mut TableSort>,
    hidden: Option<&'static mut HiddenColumns>,
    widths: Option<&'static mut ColumnWidths>,
}

/// What a restore needs of a source to put its genes back.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct GeneAccess {
    search: Option<&'static mut GeneSearch>,
    service: Option<&'static GeneService>,
    reads: Has<ReadsGenes>,
}

/// Open the bookmark's datasets, and lay out its frames once they are open.
pub fn drive_restore(
    mut commands: Commands,
    restoring: Option<ResMut<Restoring>>,
    time: Res<Time>,
    opened: Query<(Entity, &SourceUrl)>,
    panels: Query<Entity, With<Panel>>,
    sources: Query<(&DataSource, &SourceExtent, Option<&SourceVolume>)>,
    homes: Query<&crate::source::HomeView>,
    area: Res<FrameArea>,
    palette: Res<Palette>,
    mut selected: ResMut<SelectedPanel>,
    mut notice: ResMut<BookmarkNotice>,
) {
    let Some(mut restoring) = restoring else {
        return;
    };
    let now = time.elapsed_secs();

    if !restoring.begun {
        restoring.begun = true;
        info!("restoring bookmark {}", restoring.bookmark.name);
        *notice = BookmarkNotice::Working(format!("opening {}\u{2026}", restoring.bookmark.name));
        let open: Vec<(Entity, String)> = opened
            .iter()
            .map(|(entity, url)| (entity, saved_address(&url.0)))
            .collect();
        let slots = restoring
            .bookmark
            .sources
            .iter()
            .map(
                |state| match open.iter().find(|(_, url)| *url == state.url) {
                    Some((entity, _)) => {
                        commands.entity(*entity).insert(PendingSettings {
                            state: state.clone(),
                            since: now,
                            genes_asked: false,
                            spans_asked: false,
                        });
                        Slot::Open(*entity)
                    }
                    None => {
                        let url = state.url.clone();
                        Slot::Reading(fetching(async move { discover::discover(&url).await }))
                    }
                },
            )
            .collect();
        restoring.slots = slots;
    }

    let id = restoring.id;
    for index in 0..restoring.slots.len() {
        let Slot::Reading(reading) = &mut restoring.slots[index] else {
            continue;
        };
        let Some(outcome) = reading.take() else {
            continue;
        };
        let state = restoring.bookmark.sources[index].clone();
        restoring.slots[index] = match outcome {
            // A state from another viewer is a bookmark itself, and one
            // bookmark is not opened from inside another.
            Ok(Discovered::Scene(scene)) => {
                warn!(
                    "bookmark: {} is a scene of its own, not a dataset",
                    state.url
                );
                Slot::Failed(format!(
                    "{}: {} is several datasets, not one",
                    state.url, scene.name
                ))
            }
            Ok(discovered) => {
                commands.queue(move |world: &mut World| {
                    let settings = *world.resource::<LoadSettings>();
                    let Some(source) = spawn_discovered(world, discovered, settings) else {
                        return;
                    };
                    world.entity_mut(source).insert((
                        SourceUrl(state.url.clone()),
                        PendingSettings {
                            state,
                            since: now,
                            genes_asked: false,
                            spans_asked: false,
                        },
                    ));
                    match world.get_resource_mut::<Restoring>() {
                        Some(mut restoring) if restoring.id == id => {
                            restoring.slots[index] = Slot::Open(source);
                        }
                        // Replaced by another restore: the dataset stays open,
                        // offered in the menus like any other.
                        _ => {}
                    }
                });
                Slot::Registering
            }
            Err(error) => {
                warn!("bookmark: could not open {}: {error}", state.url);
                Slot::Failed(format!("{}: {error}", state.url))
            }
        };
    }

    if restoring
        .slots
        .iter()
        .any(|slot| matches!(slot, Slot::Reading(_) | Slot::Registering))
    {
        return;
    }

    let bookmark = &restoring.bookmark;
    let frames: Vec<_> = bookmark
        .frames
        .iter()
        .enumerate()
        .filter_map(|(at, frame)| {
            let base = restoring.open(frame.source)?;
            sources.get(base).ok().map(|found| (at, frame, base, found))
        })
        .take(MAX_PANELS)
        .collect();

    // With nothing to show, what is on screen stays rather than being swapped
    // for an empty window.
    if !frames.is_empty() {
        for panel in &panels {
            commands.entity(panel).despawn();
        }
    }

    // The empty frames go back where they were, and the others fill the
    // cells between them in order.
    let count = (frames.len() + bookmark.empty_frames.len()).min(MAX_PANELS);
    let empty: Vec<usize> = bookmark
        .empty_frames
        .iter()
        .copied()
        .filter(|position| *position < count)
        .collect();
    let mut cells = (0..count).filter(|position| !empty.contains(position));
    if !frames.is_empty() {
        for position in &empty {
            spawn_browse_panel(&mut commands, *position, palette.frame_bg);
        }
    }
    let mut spawned = Vec::new();
    for (at, frame, base, (data, extent, volume)) in frames {
        let Some(position) = cells.next() else {
            break;
        };
        let cell = area.cell(count, position).size();
        let limits = extent.limits_from(homes.get(base).ok(), cell);
        // A frame saved with no view, as one opened from another viewer's
        // state is, is fitted to its data as a frame opened by hand is.
        let flat = frame.view.map(|view| View {
            center: Vec2::from_array(view.center),
            scale: view
                .scale_in(cell.to_array())
                .clamp(limits.min_scale, limits.max_scale),
        });
        let panel = spawn_panel(
            &mut commands,
            base,
            data.layer,
            position,
            limits,
            flat,
            palette.frame_bg,
        );
        // A rectangle implies the tool was on when it was saved: turning the
        // tool off is what clears one, so a bookmark holding one was taken
        // with it on. Restoring the rectangle without the mode would put a
        // selection on screen that no drag could replace.
        if let Some(saved) = &frame.selection {
            commands.entity(panel).insert((
                SelectMode,
                FrameRegion {
                    from: Vec2::from_array(saved.min),
                    to: Vec2::from_array(saved.max),
                },
            ));
            if let Some(focus) = saved.focus.clone() {
                // A category the dataset no longer holds simply counts
                // nothing, which the dock reports rather than failing over.
                commands.entity(base).insert(RegionFocus {
                    column: focus.column,
                    label: focus.label,
                });
            }
        }
        if frame.linked {
            commands.entity(panel).insert(Linked::default());
        }
        // A frame drawing cross-sections is turned by an orbit of its own,
        // built once its slices arrive; restoring the saved one as a volume's
        // would ask a stack too large for one to be drawn whole.
        if frame.cross_sections {
            let sections = frame.orbit.map_or_else(Default::default, |saved| {
                crate::view::sections::CrossSections::turned(
                    Vec3::from_array(saved.target),
                    saved.yaw,
                    saved.pitch,
                    saved.distance,
                )
            });
            commands.entity(panel).insert(sections);
        }
        match (frame.orbit.filter(|_| !frame.cross_sections), volume) {
            (Some(saved), Some(volume)) => {
                let flat = flat.unwrap_or(View {
                    center: limits.center,
                    scale: limits.fit_scale,
                });
                let mut orbit = Orbit::fit(volume, flat);
                orbit.target = Vec3::from_array(saved.target);
                orbit.yaw = saved.yaw;
                orbit.pitch = saved.pitch;
                orbit.distance = saved.distance;
                commands.entity(panel).insert(orbit);
            }
            (Some(_), None) => warn!(
                "bookmark: {} is no longer a volume, so shown flat",
                data.name
            ),
            _ => {}
        }
        let mut stacked = vec![base];
        for layer in &frame.layers {
            if stacked.len() >= MAX_LAYERS {
                break;
            }
            let Some(source) = restoring.open(layer.source) else {
                continue;
            };
            let Ok((layered, ..)) = sources.get(source) else {
                continue;
            };
            if stacked.contains(&source) {
                continue;
            }
            stacked.push(source);
            spawn_layer(
                &mut commands,
                panel,
                source,
                layered.layer,
                LayerOpacity(layer.opacity.clamp(0.0, 1.0)),
            );
        }
        spawned.push((at, panel));
    }

    selected.0 = bookmark
        .selected
        .and_then(|wanted| spawned.iter().find(|(at, _)| *at == wanted))
        .or(spawned.first())
        .map(|(_, panel)| *panel);

    let failed: Vec<&str> = restoring
        .slots
        .iter()
        .filter_map(|slot| match slot {
            Slot::Failed(error) => Some(error.as_str()),
            _ => None,
        })
        .collect();
    *notice = match (spawned.len(), failed.is_empty()) {
        (0, _) => BookmarkNotice::Failed(format!(
            "could not open {}: {}",
            bookmark.name,
            failed.join("; ")
        )),
        (_, true) => BookmarkNotice::Done(format!("restored {}", bookmark.name)),
        (_, false) => BookmarkNotice::Failed(format!(
            "restored {} without {}",
            bookmark.name,
            failed.join("; ")
        )),
    };
    info!(
        "restored bookmark {}: {} frames, {} datasets could not be opened",
        bookmark.name,
        spawned.len(),
        failed.len()
    );
    commands.remove_resource::<Restoring>();
}

/// Put back each restored dataset's own settings once it can take them.
pub fn apply_pending_settings(
    mut commands: Commands,
    time: Res<Time>,
    mut sources: Query<(
        Entity,
        &DataSource,
        &mut PendingSettings,
        Option<&mut SliceStack>,
        Option<&mut SliceGrid>,
        Option<&mut SourcePointSize>,
        Option<&mut SourceChannels>,
        Option<&mut CellProperties>,
        Has<CellColumns>,
        Has<Described>,
        GeneAccess,
        TableAccess,
    )>,
) {
    for (
        entity,
        data,
        pending,
        stack,
        grid,
        point_size,
        channels,
        properties,
        has_columns,
        described,
        mut genes,
        mut table,
    ) in &mut sources
    {
        let pending = pending.into_inner();
        let since = pending.since;
        let state = &mut pending.state;
        if let (Some(slice), Some(mut stack)) = (state.slice.take(), stack) {
            apply_slice(&mut stack, slice);
        }
        if let (Some(on), Some(mut grid)) = (state.slice_grid.take(), grid) {
            grid.set_if_neq(SliceGrid(on));
        }
        if let (Some(size), Some(mut current)) = (state.point_size.take(), point_size) {
            current.0 = clamp_point_size(size);
        }
        if let Some(filtered) = state.filtered.take() {
            commands.entity(entity).insert(filtered.restored());
        }
        if let Some(scale) = state.scale.take() {
            commands.entity(entity).insert(scale);
        }
        // Kept by column and code rather than in the properties, so they need
        // not wait for a service to describe the cells.
        let colors = std::mem::take(&mut state.colors);
        if !colors.is_empty() {
            commands
                .entity(entity)
                .insert(ColorOverrides::restored(&colors));
        }
        if let Some(opacity) = state.opacity.take() {
            commands
                .entity(entity)
                .insert(SourceOpacity(opacity.clamp(0.0, 1.0)));
        }
        let saved_channels = std::mem::take(&mut state.channels);
        if let (false, Some(mut channels)) = (saved_channels.is_empty(), channels) {
            let missing = apply_channels(&mut channels, &saved_channels);
            if !missing.is_empty() {
                warn!(
                    "bookmark: {} has no channel {}",
                    data.name,
                    missing.join(", ")
                );
            }
        }

        let waiting = match (&state.cells, properties) {
            (None, _) => false,
            (Some(_), None) => {
                warn!("bookmark: {} has no cell properties to filter", data.name);
                false
            }
            // The labels the filters name never arrived, and waiting will not
            // bring them; a retry is the user's to ask for.
            (Some(_), Some(properties)) if matches!(properties.state, PropertyState::Failed(_)) => {
                warn!(
                    "bookmark: {}'s cell properties failed to load, so its filters were not restored",
                    data.name
                );
                false
            }
            // A service will replace these, so filters set now would be lost.
            (Some(_), Some(properties))
                if properties.state != PropertyState::Ready || (has_columns && !described) =>
            {
                if time.elapsed_secs() - since > CELLS_PATIENCE_SECS {
                    warn!(
                        "bookmark: gave up waiting for {}'s cell properties",
                        data.name
                    );
                    false
                } else {
                    true
                }
            }
            // Genes the bookmark adds have to be there before anything can be
            // set on them, and each is added only once its histogram is in.
            (Some(saved), Some(properties))
                if adding_genes(&properties, saved, &mut genes, &mut pending.genes_asked)
                    && time.elapsed_secs() - since <= CELLS_PATIENCE_SECS =>
            {
                true
            }
            (Some(saved), Some(mut properties)) => {
                let missing = apply_cells(&mut properties, saved);
                if !missing.is_empty() {
                    warn!(
                        "bookmark: {} has no cell property {}",
                        data.name,
                        missing.join(", ")
                    );
                }
                false
            }
        };
        let patient = time.elapsed_secs() - since <= CELLS_PATIENCE_SECS;
        let waiting =
            waiting | restore_table(data, state, &mut table, &mut pending.spans_asked, patient);
        if !waiting {
            commands.entity(entity).remove::<PendingSettings>();
        }
    }
}

/// Put a table back on its page, narrowed, sorted and drawing the columns it
/// drew, and say whether that is still waiting on the table.
///
/// The page is set with the filters, never before them: whatever produced
/// the rows fetches the two together, and a page set first would be fetched
/// from the unfiltered table.
fn restore_table(
    data: &DataSource,
    state: &mut SourceState,
    table: &mut TableAccessItem<'_, '_>,
    spans_asked: &mut bool,
    patient: bool,
) -> bool {
    let Some(saved) = state.table.take() else {
        return false;
    };
    let Some(paging) = table.paging.as_mut() else {
        warn!("bookmark: {} is no longer a table", data.name);
        return false;
    };
    if !saved.filters.is_empty() {
        let Some(filters) = table.filters.as_mut().filter(|filters| !filters.pending) else {
            if patient {
                state.table = Some(saved);
                return true;
            }
            warn!(
                "bookmark: gave up waiting for {}'s table filters",
                data.name
            );
            return false;
        };
        let ask = !*spans_asked;
        *spans_asked = true;
        if ask_saved_spans(filters, &saved.filters, ask) && patient {
            state.table = Some(saved);
            return true;
        }
        let missing = apply_table(filters, &saved.filters);
        if !missing.is_empty() {
            warn!(
                "bookmark: {} could not narrow {} as saved",
                data.name,
                missing.join(", ")
            );
        }
    }
    if let Some(sort) = table.sort.as_mut() {
        sort.set_if_neq(TableSort(saved.sort.clone()));
    }
    if let Some(hidden) = table.hidden.as_mut() {
        hidden.set_if_neq(HiddenColumns(saved.hidden.iter().cloned().collect()));
    }
    if let Some(widths) = table.widths.as_mut() {
        widths.set_if_neq(ColumnWidths(saved.widths.clone()));
    }
    // The total is the unfiltered table's until the narrowed one is counted,
    // so a page past its end is left for the format to bring back.
    let page = match paging.total {
        Some(_) if saved.filters.is_empty() => paging.clamped(saved.page),
        _ => saved.page,
    };
    if paging.page != page {
        paging.page = page;
    }
    false
}

/// Ask for the genes a bookmark adds, and say whether any are still coming.
///
/// Asked once. A gene the service could not describe leaves the search's
/// queue without arriving, and is then reported missing like any property the
/// dataset lacks rather than being waited on for good.
fn adding_genes(
    properties: &CellProperties,
    saved: &CellsState,
    genes: &mut GeneAccessItem<'_, '_>,
    asked: &mut bool,
) -> bool {
    let missing = genes_missing(properties, saved);
    if missing.is_empty() {
        return false;
    }
    match (&mut genes.search, genes.service) {
        (Some(search), _) => {
            if !*asked {
                *asked = true;
                for gene in &missing {
                    search.add(gene.clone());
                }
            }
            missing
                .iter()
                .any(|gene| search.adding.iter().any(|pending| pending.id == gene.id))
        }
        // Not yet looked at: whether its catalog knows its genes is decided
        // once its labels are in.
        (None, None) => genes.reads,
        (None, Some(service)) => {
            debug_assert!(!service.knows_genes());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmark::capture::capture;
    use crate::source::{SourceInfo, register_in};
    use crate::view::requests::apply_panel_requests;
    use crate::view::{FrameLayers, PanelRequest};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .add_message::<PanelRequest>()
        .add_message::<crate::view::AwaitDataset>()
        .add_message::<crate::view::DatasetRequest>()
        .init_resource::<FrameArea>()
        .init_resource::<SelectedPanel>()
        .init_resource::<BookmarkNotice>()
        .insert_resource(LoadSettings::default())
        .insert_resource(Palette::dark())
        .add_systems(
            Update,
            (apply_panel_requests, drive_restore, apply_pending_settings).chain(),
        );
        app
    }

    fn source(app: &mut App, name: &str, url: &str) -> Entity {
        let source = register_in(
            app.world_mut(),
            SourceInfo {
                name: name.into(),
                unit: "px".into(),
                detail: String::new(),
                stat: String::new(),
                category: crate::source::Category::Image,
            },
            SourceExtent {
                center: Vec2::ZERO,
                size: Vec2::splat(100.0),
                finest: 0.1,
            },
        );
        app.world_mut()
            .entity_mut(source)
            .insert(SourceUrl(url.into()));
        source
    }

    fn panels(app: &mut App) -> Vec<Entity> {
        let mut query = app.world_mut().query_filtered::<Entity, With<Panel>>();
        query.iter(app.world()).collect()
    }

    #[test]
    fn a_frame_comes_back_as_it_was_saved() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "https://store/slide.dzi");
        let outlines = source(&mut app, "Outlines", "https://store/outlines.svg");
        app.world_mut()
            .entity_mut(slide)
            .insert(SliceStack::new(10));

        app.world_mut().write_message(PanelRequest::Open(slide));
        app.update();
        let panel = panels(&mut app)[0];
        app.world_mut().write_message(PanelRequest::AddLayer {
            panel,
            source: outlines,
        });
        app.update();
        let layer = app.world().get::<FrameLayers>(panel).unwrap().cameras()[0];
        app.world_mut().entity_mut(layer).insert(LayerOpacity(0.4));
        app.world_mut().entity_mut(panel).insert((
            Transform::from_xyz(5.0, 6.0, 1000.0),
            Projection::Orthographic(OrthographicProjection {
                scale: 0.5,
                ..OrthographicProjection::default_2d()
            }),
        ));
        app.world_mut()
            .get_mut::<SliceStack>(slide)
            .unwrap()
            .current = 3;

        let bookmark = capture(app.world_mut(), "test".into()).unwrap();
        assert_eq!(bookmark.sources.len(), 2);

        app.world_mut()
            .get_mut::<SliceStack>(slide)
            .unwrap()
            .current = 7;
        app.world_mut().write_message(PanelRequest::Close(panel));
        app.update();
        assert!(panels(&mut app).is_empty());

        app.insert_resource(Restoring::new(bookmark));
        app.update();
        app.update();

        let restored = panels(&mut app);
        assert_eq!(restored.len(), 1);
        let panel = restored[0];
        let world = app.world();
        assert_eq!(
            world.get::<crate::source::ShowsSource>(panel).unwrap().0,
            slide
        );
        let translation = world.get::<Transform>(panel).unwrap().translation;
        assert_eq!(translation.truncate(), Vec2::new(5.0, 6.0));
        let Projection::Orthographic(ortho) = world.get::<Projection>(panel).unwrap() else {
            panic!("a flat frame came back in 3D");
        };
        assert!((ortho.scale - 0.5).abs() < 1e-5);
        let layer = world.get::<FrameLayers>(panel).unwrap().cameras()[0];
        assert_eq!(world.get::<LayerOpacity>(layer), Some(&LayerOpacity(0.4)));
        assert_eq!(world.get::<SliceStack>(slide).unwrap().current, 3);
        assert_eq!(world.resource::<SelectedPanel>().0, Some(panel));
        assert!(world.get_resource::<Restoring>().is_none());
        assert!(world.get::<PendingSettings>(slide).is_none());
        assert_eq!(
            *world.resource::<BookmarkNotice>(),
            BookmarkNotice::Done("restored test".into())
        );
    }

    #[test]
    fn an_empty_frame_comes_back_in_its_cell() {
        let mut app = app();
        let slide = source(&mut app, "Slide", "https://store/slide.dzi");
        app.world_mut().write_message(PanelRequest::Browse(None));
        app.update();
        app.world_mut().write_message(PanelRequest::Open(slide));
        app.update();

        let bookmark = capture(app.world_mut(), "test".into()).unwrap();
        assert_eq!(bookmark.frames.len(), 1);
        assert_eq!(bookmark.empty_frames, [0]);

        for panel in panels(&mut app) {
            app.world_mut().write_message(PanelRequest::Close(panel));
        }
        app.update();
        assert!(panels(&mut app).is_empty());

        app.insert_resource(Restoring::new(bookmark));
        app.update();
        app.update();

        let mut query = app
            .world_mut()
            .query::<(&Panel, Option<&crate::source::ShowsSource>)>();
        let mut restored: Vec<(usize, Option<Entity>)> = query
            .iter(app.world())
            .map(|(panel, shows)| (panel.index, shows.map(|shows| shows.0)))
            .collect();
        restored.sort_unstable();
        assert_eq!(restored, [(0, None), (1, Some(slide))]);
    }

    fn cells(genes: &[(&str, u32)]) -> CellProperties {
        use crate::source::properties::{CellProperty, NumericRange, PropertyKind, PropertyValue};
        let mut properties = CellProperties::ready(vec![CellProperty {
            id: "class".into(),
            name: "Class".into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Categorical(vec![PropertyValue {
                code: 0,
                label: "A".into(),
                reference: None,
                color: None,
                count: None,
                selected: false,
            }]),
        }]);
        for (id, index) in genes {
            properties.add_gene(CellProperty {
                id: id.to_string(),
                name: id.to_uppercase(),
                shown: true,
                gene: Some(*index),
                kind: PropertyKind::Numeric(NumericRange::full(0.0, 8.0, vec![1, 1])),
            });
        }
        properties
    }

    /// A source whose bookmark colors by a gene and filters it, restored onto
    /// a copy of the dataset without that gene.
    fn restoring_a_gene(app: &mut App) -> Entity {
        let source = source(app, "Cells", "https://store/cells.json");
        let mut saved = cells(&[("gad1", 7)]);
        saved.color_by_id("gad1");
        saved.properties[1].range_mut().unwrap().from = 2.0;
        app.world_mut().entity_mut(source).insert((
            cells(&[]),
            GeneSearch::default(),
            PendingSettings {
                state: SourceState {
                    cells: Some(crate::bookmark::snapshot::cells_of(&saved)),
                    ..default()
                },
                since: 0.0,
                genes_asked: false,
                spans_asked: false,
            },
        ));
        app.update();
        source
    }

    #[test]
    fn a_bookmarks_genes_are_added_before_their_settings_are_applied() {
        let mut app = app();
        let source = restoring_a_gene(&mut app);

        let adding: Vec<String> = app
            .world()
            .get::<GeneSearch>(source)
            .unwrap()
            .adding
            .iter()
            .map(|gene| gene.id.clone())
            .collect();
        assert_eq!(adding, ["gad1"]);
        assert!(
            app.world().get::<PendingSettings>(source).is_some(),
            "nothing can be set on a gene that is not there yet"
        );

        // The service answers.
        let mut world = app.world_mut().entity_mut(source);
        world.get_mut::<GeneSearch>().unwrap().adding.clear();
        world
            .get_mut::<CellProperties>()
            .unwrap()
            .add_gene(cells(&[("gad1", 7)]).properties[1].clone());
        app.update();

        let world = app.world();
        assert!(world.get::<PendingSettings>(source).is_none());
        let properties = world.get::<CellProperties>(source).unwrap();
        assert_eq!(properties.color_by, Some(1));
        assert_eq!(properties.properties[1].range().unwrap().from, 2.0);
    }

    #[test]
    fn a_tables_page_waits_for_the_filters_it_was_saved_with() {
        use crate::bookmark::snapshot::{ColumnFilter, TableFilterState, TableState};
        use crate::source::table::{SortKey, TableFilter, TableFilterValue};

        let mut app = app();
        let source = source(&mut app, "Specimens", "https://store/specimens");
        app.world_mut().entity_mut(source).insert((
            TablePaging::new(100, Some(1000)),
            TableFilters::pending(),
            TableSort::default(),
            HiddenColumns::default(),
            ColumnWidths::default(),
            PendingSettings {
                state: SourceState {
                    table: Some(TableState {
                        page: 2,
                        filters: vec![ColumnFilter {
                            id: "sex".into(),
                            filter: TableFilterState::Values {
                                values: vec!["F".into()],
                            },
                        }],
                        sort: vec![SortKey {
                            column: "Age".into(),
                            descending: true,
                        }],
                        hidden: vec!["Donor ID".into()],
                        widths: [("Donor ID".to_string(), 180.0)].into(),
                    }),
                    ..default()
                },
                since: 0.0,
                genes_asked: false,
                spans_asked: false,
            },
        ));
        app.update();
        assert_eq!(app.world().get::<TablePaging>(source).unwrap().page, 0);
        // Sorted with the filters rather than before them, for the same
        // reason the page is.
        assert!(app.world().get::<TableSort>(source).unwrap().0.is_empty());
        assert!(app.world().get::<PendingSettings>(source).is_some());

        // The columns land.
        let value = |label: &str| TableFilterValue {
            label: label.into(),
            count: 1,
            chosen: false,
        };
        app.world_mut()
            .entity_mut(source)
            .insert(TableFilters::ready(vec![TableFilter::values(
                "sex",
                "Sex",
                vec![value("F"), value("M")],
            )]));
        app.update();

        let world = app.world();
        assert!(world.get::<PendingSettings>(source).is_none());
        assert_eq!(world.get::<TablePaging>(source).unwrap().page, 2);
        let filters = world.get::<TableFilters>(source).unwrap();
        assert!(filters.columns[0].listed()[0].chosen);
        let sort = &world.get::<TableSort>(source).unwrap().0;
        assert_eq!(sort[0].column, "Age");
        assert!(sort[0].descending);
        assert!(
            world
                .get::<HiddenColumns>(source)
                .unwrap()
                .0
                .contains("Donor ID")
        );
        assert_eq!(
            world.get::<ColumnWidths>(source).unwrap().0.get("Donor ID"),
            Some(&180.0)
        );
    }

    #[test]
    fn a_gene_the_service_could_not_add_does_not_hold_the_restore() {
        let mut app = app();
        let source = restoring_a_gene(&mut app);
        app.world_mut()
            .get_mut::<GeneSearch>(source)
            .unwrap()
            .adding
            .clear();
        app.update();

        let world = app.world();
        assert!(world.get::<PendingSettings>(source).is_none());
        assert_eq!(
            world.get::<CellProperties>(source).unwrap().color_by,
            Some(0)
        );
    }
}
