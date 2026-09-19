//! Reading what is on screen into a [`Bookmark`].

use bevy::prelude::*;

use super::snapshot::{
    Bookmark, FrameState, LayerState, OrbitState, SourceState, VERSION, ViewState, cells_of,
    channels_of,
};
use crate::render::points::SourcePointSize;
use crate::render::settings::SourceOpacity;
use crate::source::SourceUrl;
use crate::source::channels::SourceChannels;
use crate::source::properties::{CellProperties, FilteredPoints, PropertyState, Provenance};
use crate::source::stack::{SliceGrid, SliceStack};
use crate::view::{FrameArea, FrameLayers, LayerOpacity, Orbit, SelectedPanel, ShowsSource};
use crate::view::{Panel, View};

/// The address a dataset is saved under.
///
/// A local path is made absolute, so the bookmark opens from wherever the app
/// is started next time. Anything with a scheme is left as it was typed.
pub fn saved_address(url: &str) -> String {
    if is_remote(url) {
        return url.to_string();
    }
    std::fs::canonicalize(url).map_or_else(|_| url.to_string(), |path| path.display().to_string())
}

/// Whether someone on another machine could open this address.
pub fn is_remote(url: &str) -> bool {
    url.contains("://")
}

/// Everything on screen, as a bookmark named `name`.
///
/// Only datasets a frame or layer shows are saved: an open dataset nothing is
/// looking at would be fetched again on restore for nobody.
pub fn capture(world: &mut World, name: String) -> Result<Bookmark, String> {
    let area = *world.resource::<FrameArea>();
    let selected = world.resource::<SelectedPanel>().0;

    let mut panels = world.query_filtered::<(
        Entity,
        &Panel,
        &ShowsSource,
        &Transform,
        &Projection,
        Option<&Orbit>,
        Option<&FrameLayers>,
    ), With<Panel>>();
    let mut found: Vec<_> = panels
        .iter(world)
        .map(
            |(entity, panel, shows, transform, projection, orbit, layers)| {
                let flat = match (orbit, projection) {
                    (Some(orbit), _) => Some(orbit.flat),
                    (None, Projection::Orthographic(ortho)) => Some(View {
                        centre: transform.translation.truncate(),
                        scale: ortho.scale,
                    }),
                    (None, _) => None,
                };
                (
                    entity,
                    panel.index,
                    shows.0,
                    flat,
                    orbit.copied(),
                    layers.map(|layers| layers.cameras().to_vec()),
                )
            },
        )
        .collect();
    found.sort_by_key(|(_, index, ..)| *index);
    if found.is_empty() {
        return Err("nothing is open to bookmark".into());
    }

    let mut sources: Vec<(Entity, String)> = Vec::new();
    let mut index_of = |world: &World, source: Entity| -> Option<usize> {
        if let Some(at) = sources.iter().position(|(entity, _)| *entity == source) {
            return Some(at);
        }
        let url = world.get::<SourceUrl>(source)?;
        sources.push((source, saved_address(&url.0)));
        Some(sources.len() - 1)
    };

    let count = found.len();
    let mut frames = Vec::new();
    let mut selected_frame = None;
    for (position, (entity, _, source, flat, orbit, layers)) in found.into_iter().enumerate() {
        let Some(flat) = flat else { continue };
        let Some(source) = index_of(world, source) else {
            warn!("a frame's dataset has no address, so the bookmark leaves it out");
            continue;
        };
        let cell = area.cell(count, position).size();
        let layers = layers
            .unwrap_or_default()
            .into_iter()
            .filter_map(|camera| {
                let shows = world.get::<ShowsSource>(camera)?.0;
                let opacity = world.get::<LayerOpacity>(camera).map_or(1.0, |o| o.0);
                Some(LayerState {
                    source: index_of(world, shows)?,
                    opacity,
                })
            })
            .collect();
        if selected == Some(entity) {
            selected_frame = Some(frames.len());
        }
        frames.push(FrameState {
            source,
            view: ViewState::new(flat.centre.to_array(), flat.scale, cell.to_array()),
            orbit: orbit.map(|orbit| OrbitState {
                target: orbit.target.to_array(),
                yaw: orbit.yaw,
                pitch: orbit.pitch,
                distance: orbit.distance,
            }),
            layers,
        });
    }
    if frames.is_empty() {
        return Err("none of the open datasets has an address to save".into());
    }

    let sources = sources
        .into_iter()
        .map(|(entity, url)| source_state(world.entity(entity), url))
        .collect();

    Ok(Bookmark {
        version: VERSION,
        name,
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
        sources,
        frames,
        selected: selected_frame,
    })
}

fn source_state(source: EntityRef, url: String) -> SourceState {
    SourceState {
        url,
        slice: source.get::<SliceStack>().map(|stack| stack.current),
        slice_grid: source.get::<SliceGrid>().map(|grid| grid.0),
        opacity: source.get::<SourceOpacity>().map(|opacity| opacity.0),
        point_size: source.get::<SourcePointSize>().map(|size| size.0),
        filtered: source.get::<FilteredPoints>().map(FilteredPoints::saved),
        channels: source
            .get::<SourceChannels>()
            .map(channels_of)
            .unwrap_or_default(),
        // Properties a service is still being asked about are the format's
        // placeholders, about to be replaced, and nothing has been chosen
        // against them worth saving.
        cells: source
            .get::<CellProperties>()
            .filter(|properties| {
                properties.state == PropertyState::Ready
                    && !matches!(properties.provenance, Provenance::Fetching(_))
            })
            .map(cells_of),
    }
}
