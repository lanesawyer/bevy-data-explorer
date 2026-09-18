//! A frame described ahead of time: a dataset to open, the datasets to stack
//! over it, and how each is drawn.
//!
//! Opening a dataset by address is asynchronous, and a layer can only be added
//! to a frame that exists, so a preset cannot be opened in one go. The base is
//! asked for as a frame of its own, and the preset waits for that frame to
//! appear before asking for each layer onto it — by the same requests a person
//! choosing them from the frame's menu would raise. What it says about each
//! dataset beyond its address waits in the same way, for that dataset to open.

use bevy::prelude::*;

use super::{DatasetRequest, DatasetTarget, Panel, ShowsSource};
use crate::source::sections::{FollowsSections, SectionPlacement};
use crate::source::{SourceExtent, SourceUrl, ViewLimits};

#[derive(Clone, Debug)]
pub struct Preset {
    /// What the frame opens onto, at the bottom of the stack.
    pub base: PresetDataset,
    /// Stacked over the base, bottom first.
    pub layers: Vec<PresetDataset>,
    /// The dataset the frame is framed on once it is open, if not the base.
    pub frame_on: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PresetDataset {
    pub url: String,
    /// Drawn under some of another dataset's sections rather than where it
    /// lies.
    pub follows: Option<FollowsSections>,
}

/// Open a preset in a frame of its own.
#[derive(Message, Clone, Debug)]
pub struct OpenPreset(pub Preset);

/// What presets are still waiting on.
#[derive(Resource, Default)]
pub struct PendingPresets {
    /// Presets whose base has been asked for and has no frame yet.
    frames: Vec<Preset>,
    /// Sections to follow, by the address of the dataset that follows them.
    follows: Vec<(String, FollowsSections)>,
    /// Frames to frame on a dataset, by its address, once it is open.
    fits: Vec<(Entity, String)>,
}

/// Ask for each preset's base, and remember the rest.
pub fn open_presets(
    mut opened: MessageReader<OpenPreset>,
    mut pending: ResMut<PendingPresets>,
    mut datasets: MessageWriter<DatasetRequest>,
) {
    for OpenPreset(preset) in opened.read() {
        datasets.write(DatasetRequest {
            url: preset.base.url.clone(),
            target: DatasetTarget::NewFrame,
        });
        if let Some(follows) = &preset.base.follows {
            pending
                .follows
                .push((preset.base.url.clone(), follows.clone()));
        }
        pending.frames.push(preset.clone());
    }
}

/// Layer a preset onto its frame once the frame exists.
///
/// Matched by a newly opened frame showing the base's address, so a frame
/// already onto the same dataset is left alone.
pub fn layer_presets(
    mut pending: ResMut<PendingPresets>,
    opened: Query<(Entity, &ShowsSource), Added<Panel>>,
    urls: Query<&SourceUrl>,
    mut datasets: MessageWriter<DatasetRequest>,
) {
    if pending.frames.is_empty() {
        return;
    }
    for (panel, shows) in &opened {
        let Ok(url) = urls.get(shows.0) else {
            continue;
        };
        let Some(at) = pending.frames.iter().position(|p| p.base.url == url.0) else {
            continue;
        };
        let preset = pending.frames.remove(at);
        for layer in preset.layers {
            if let Some(follows) = layer.follows {
                pending.follows.push((layer.url.clone(), follows));
            }
            datasets.write(DatasetRequest {
                url: layer.url,
                target: DatasetTarget::Layer(panel),
            });
        }
        if let Some(url) = preset.frame_on {
            pending.fits.push((panel, url));
        }
    }
}

/// Hand each dataset the sections it follows once it is open.
pub fn attach_follows(
    mut commands: Commands,
    mut pending: ResMut<PendingPresets>,
    sources: Query<(Entity, &SourceUrl)>,
) {
    if pending.follows.is_empty() {
        return;
    }
    pending.follows.retain(|(url, follows)| {
        let Some((source, _)) = sources.iter().find(|(_, opened)| opened.0 == *url) else {
            return true;
        };
        commands.entity(source).insert(follows.clone());
        false
    });
}

/// Frame a preset's frame on the dataset it names once that dataset has
/// settled on its extent.
///
/// A sectioned dataset is registered before it has laid its sections out, so
/// it is waited for until it says where they are.
pub fn fit_presets(
    mut pending: ResMut<PendingPresets>,
    sources: Query<(&SourceUrl, &SourceExtent, Option<&SectionPlacement>)>,
    mut panels: Query<(&Camera, &mut Transform, &mut Projection, &mut ViewLimits), With<Panel>>,
) {
    if pending.fits.is_empty() {
        return;
    }
    pending.fits.retain(|(panel, url)| {
        let Ok((camera, mut transform, mut projection, mut limits)) = panels.get_mut(*panel) else {
            // Closed before it could be framed.
            return false;
        };
        let Some((_, extent, _)) = sources.iter().find(|(opened, _, placement)| {
            opened.0 == *url && placement.is_none_or(|placement| !placement.0.is_empty())
        }) else {
            return true;
        };
        let (Some(viewport), Projection::Orthographic(ortho)) =
            (camera.logical_viewport_size(), projection.as_mut())
        else {
            return true;
        };
        let fitted = extent.limits(viewport);
        *limits = fitted;
        transform.translation = fitted.centre.extend(transform.translation.z);
        ortho.scale = fitted.fit_scale;
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceInfo, register_in};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<OpenPreset>()
            .add_message::<DatasetRequest>()
            .init_resource::<PendingPresets>()
            .add_systems(Update, (open_presets, layer_presets, attach_follows));
        app
    }

    fn source(app: &mut App, url: &str) -> Entity {
        let source = register_in(
            app.world_mut(),
            SourceInfo {
                name: url.into(),
                unit: "mm".into(),
                detail: String::new(),
                stat: String::new(),
            },
            SourceExtent {
                centre: Vec2::ZERO,
                size: Vec2::ONE,
                finest: 0.1,
            },
        );
        app.world_mut()
            .entity_mut(source)
            .insert(SourceUrl(url.into()));
        source
    }

    fn requests(app: &mut App) -> Vec<DatasetRequest> {
        app.world_mut()
            .resource_mut::<Messages<DatasetRequest>>()
            .drain()
            .collect()
    }

    fn at(url: &str) -> PresetDataset {
        PresetDataset {
            url: url.into(),
            follows: None,
        }
    }

    fn preset() -> Preset {
        Preset {
            base: at("image"),
            layers: vec![
                PresetDataset {
                    url: "slab".into(),
                    follows: Some(FollowsSections {
                        sections: "points".into(),
                        ids: vec!["a".into()],
                    }),
                },
                at("points"),
            ],
            frame_on: Some("points".into()),
        }
    }

    #[test]
    fn layers_wait_for_the_base_frame_and_then_go_onto_it() {
        let mut app = app();
        app.world_mut().write_message(OpenPreset(preset()));
        app.update();
        let asked = requests(&mut app);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].url, "image");
        assert_eq!(asked[0].target, DatasetTarget::NewFrame);

        // A frame onto something else is not the preset's.
        let other = source(&mut app, "other");
        app.world_mut()
            .spawn((Panel { index: 0 }, ShowsSource(other)));
        app.update();
        assert!(requests(&mut app).is_empty());

        let image = source(&mut app, "image");
        let panel = app
            .world_mut()
            .spawn((Panel { index: 1 }, ShowsSource(image)))
            .id();
        app.update();
        let asked: Vec<(String, DatasetTarget)> = requests(&mut app)
            .into_iter()
            .map(|request| (request.url, request.target))
            .collect();
        assert_eq!(
            asked,
            vec![
                ("slab".into(), DatasetTarget::Layer(panel)),
                ("points".into(), DatasetTarget::Layer(panel)),
            ]
        );
        assert_eq!(
            app.world().resource::<PendingPresets>().fits,
            vec![(panel, "points".to_string())]
        );
    }

    #[test]
    fn a_layer_follows_its_sections_once_it_is_open() {
        let mut app = app();
        app.world_mut().write_message(OpenPreset(preset()));
        app.update();
        let image = source(&mut app, "image");
        app.world_mut()
            .spawn((Panel { index: 0 }, ShowsSource(image)));
        app.update();

        let slab = source(&mut app, "slab");
        app.update();
        let follows = app.world().get::<FollowsSections>(slab).unwrap();
        assert_eq!(follows.sections, "points");
        assert!(app.world().resource::<PendingPresets>().follows.is_empty());
    }
}
