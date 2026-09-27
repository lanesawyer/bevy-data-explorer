//! What the platform holds about the specimen picked out in a frame beyond
//! its row: pictures — plots of its recordings, its reconstruction, the
//! types it was assigned — and files, such as the OME-Zarr store of a
//! Genetic Tools Atlas brain and the Neuroglancer state beside it.
//!
//! Asked for one specimen at a time, when it is picked, in one request. The
//! platform sends each picture's PNG inline, so that one request brings all
//! of them — about 160KB for the seven a Patch-seq cell has — where asking
//! with the page would bring them for every row on it: 29MB for three
//! hundred.
//!
//! Pictures are asked for only by a project that lays them out. Its layout
//! names them in the order its own page shows them, which is the order here
//! too, with a modality's pictures kept together under it. Files are asked
//! for of every project: they are a few addresses, and nothing says ahead of
//! time which projects have them.

use base64::Engine;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::*;

const PICKED: &str = "query picked($filter: [Filter], $images: Boolean!) {
  aio_specimen(limit: 1, filter: $filter) {
    files { name type uri }
    images @include(if: $images) {
      featureType { title }
      modality { name }
      url
      bytes
    }
  }
}";

/// What pictures a project's specimens have, and which specimen's pictures
/// and files are shown.
#[derive(Component)]
pub struct PickedSpecimen {
    /// The pictures of a project that shows its kinds together, by title.
    /// One showing them apart has its kinds' own in their layouts.
    whole: Vec<String>,
    /// The specimen whose pictures and files are shown or being fetched.
    of: Option<String>,
    fetching: Option<Fetching<Result<Answer, String>>>,
}

impl PickedSpecimen {
    pub(super) fn new(whole: Vec<String>) -> Self {
        PickedSpecimen {
            whole,
            of: None,
            fetching: None,
        }
    }
}

/// What came back about a specimen.
pub(super) struct Answer {
    pictures: Vec<Decoded>,
    files: Vec<RecordFile>,
}

/// One picture, decoded off the task.
pub(super) struct Decoded {
    title: String,
    group: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Deserialize)]
struct PickedData {
    #[serde(default, deserialize_with = "maybe_list")]
    aio_specimen: Vec<Picked>,
}

#[derive(Deserialize)]
struct Picked {
    #[serde(default, deserialize_with = "maybe_list")]
    images: Vec<Picture>,
    #[serde(default, deserialize_with = "maybe_list")]
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Picture {
    feature_type: Option<Titled>,
    #[serde(default, deserialize_with = "maybe_list")]
    modality: Vec<Named>,
    url: Option<String>,
    bytes: Option<String>,
}

/// Fetch the pictures and files of the specimen picked out, and hand them
/// over when they land.
pub(super) fn serve_picked(
    mut sources: Query<(
        &SpecimenPages,
        &SelectedRecord,
        &mut PickedSpecimen,
        &mut RecordImages,
        &mut RecordFiles,
    )>,
    mut assets: ResMut<Assets<Image>>,
) {
    for (pages, record, mut picked, mut images, mut files) in &mut sources {
        let wanted = specimen_of(record);
        if wanted != picked.of {
            let order = match &pages.scope.kind {
                Some(kind) => pages
                    .layout(kind)
                    .map(|layout| layout.images)
                    .unwrap_or_default(),
                None => picked.whole.clone(),
            };
            images.0 = if wanted.is_some() && !order.is_empty() {
                RecordImagesState::Fetching
            } else {
                RecordImagesState::None
            };
            files.set_if_neq(RecordFiles::default());
            picked.fetching = wanted.clone().map(|specimen| {
                let (endpoint, scope) = (pages.endpoint.clone(), pages.scope.clone());
                fetching(async move { ask_picked(&endpoint, &scope, specimen, order).await })
            });
            picked.of = wanted;
        }

        let Some(answer) = picked.fetching.as_mut().and_then(Fetching::take) else {
            continue;
        };
        picked.fetching = None;
        match answer {
            Ok(answer) => {
                images.0 = if answer.pictures.is_empty() {
                    RecordImagesState::None
                } else {
                    RecordImagesState::Ready(
                        answer
                            .pictures
                            .into_iter()
                            .map(|picture| RecordImage {
                                title: picture.title,
                                group: picture.group,
                                size: UVec2::new(picture.width, picture.height),
                                image: assets.add(texture(
                                    picture.width,
                                    picture.height,
                                    picture.rgba,
                                )),
                            })
                            .collect(),
                    )
                };
                files.set_if_neq(RecordFiles(answer.files));
            }
            Err(e) => {
                warn!("reading the picked specimen: {e}");
                if matches!(images.0, RecordImagesState::Fetching) {
                    images.0 = RecordImagesState::Failed(e);
                }
            }
        }
    }
}

/// The specimen a picked record is of, once it has been read.
fn specimen_of(record: &SelectedRecord) -> Option<String> {
    record
        .0
        .as_ref()?
        .fields
        .as_ref()?
        .iter()
        .find(|(name, _)| name == SPECIMEN)
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
}

async fn ask_picked(
    endpoint: &str,
    scope: &Scope,
    specimen: String,
    order: Vec<String>,
) -> Result<Answer, String> {
    let terms = [TableFilterTerm::Is {
        field: SPECIMEN_FIELD.to_string(),
        value: specimen,
    }];
    let variables = json!({
        "filter": specimen_filters(scope, &terms),
        "images": !order.is_empty(),
    });
    let data: PickedData = graphql::ask(endpoint, PICKED, variables).await?;
    let Some(picked) = data.aio_specimen.into_iter().next() else {
        return Ok(Answer {
            pictures: Vec::new(),
            files: Vec::new(),
        });
    };
    let files = picked
        .files
        .into_iter()
        .filter_map(|file| {
            let address = file.uri.filter(|it| !it.trim().is_empty())?;
            Some(RecordFile {
                name: file.name.unwrap_or_default(),
                kind: file.kind.unwrap_or_default(),
                address,
            })
        })
        .collect();
    let pictures = picked.images;

    let mut decoded = Vec::with_capacity(pictures.len());
    for picture in pictures {
        let title = picture
            .feature_type
            .and_then(|it| it.title)
            .unwrap_or_default();
        let bytes = match (picture.bytes, picture.url) {
            (Some(inline), _) => base64::engine::general_purpose::STANDARD
                .decode(inline.trim())
                .map_err(|e| format!("{title}: {e}"))?,
            (None, Some(url)) => crate::app::net::read(&url).await?,
            (None, None) => continue,
        };
        let rgba = image::load_from_memory(&bytes)
            .map_err(|e| format!("decoding {title}: {e}"))?
            .into_rgba8();
        decoded.push(Decoded {
            group: group_of(&picture.modality),
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
            title,
        });
    }
    Ok(Answer {
        pictures: arranged(decoded, &order),
        files,
    })
}

/// What a picture is shown under: its modality, capitalized as a heading.
fn group_of(modality: &[Named]) -> String {
    let name = modality
        .iter()
        .find_map(|it| it.name.as_deref())
        .unwrap_or("Other");
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Put pictures in the layout's order, a group's together where its first
/// one falls. A picture the layout does not name goes after those it does.
fn arranged(mut pictures: Vec<Decoded>, order: &[String]) -> Vec<Decoded> {
    let place = |title: &str| {
        order
            .iter()
            .position(|it| it == title)
            .unwrap_or(order.len())
    };
    pictures.sort_by_key(|picture| place(&picture.title));
    let mut groups: Vec<String> = Vec::new();
    for picture in &pictures {
        if !groups.contains(&picture.group) {
            groups.push(picture.group.clone());
        }
    }
    pictures.sort_by_key(|picture| groups.iter().position(|it| *it == picture.group));
    pictures
}

fn texture(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(title: &str, group: &str) -> Decoded {
        Decoded {
            title: title.into(),
            group: group.into(),
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        }
    }

    fn titles(pictures: &[Decoded]) -> Vec<&str> {
        pictures.iter().map(|it| it.title.as_str()).collect()
    }

    #[test]
    fn pictures_follow_the_layout_with_a_groups_kept_together() {
        let order = ["T-Type", "Rheobase", "Reconstruction", "Action Potential"]
            .map(String::from)
            .to_vec();
        let pictures = vec![
            picture("Action Potential", "Electrophysiology"),
            picture("Unnamed", "Other"),
            picture("Reconstruction", "Morphology"),
            picture("Rheobase", "Electrophysiology"),
            picture("T-Type", "Transcriptomics"),
        ];
        assert_eq!(
            titles(&arranged(pictures, &order)),
            [
                "T-Type",
                "Rheobase",
                "Action Potential",
                "Reconstruction",
                "Unnamed"
            ]
        );
    }

    #[test]
    fn a_modality_reads_as_a_heading() {
        let named = |name: &str| Named {
            name: Some(name.into()),
        };
        assert_eq!(group_of(&[named("electrophysiology")]), "Electrophysiology");
        assert_eq!(group_of(&[]), "Other");
    }

    #[test]
    fn the_picked_query_reads_the_platforms_answer() {
        let text = r#"{"aio_specimen":[{"images":[{"featureType":{"title":"Action Potential"},
            "modality":[{"name":"electrophysiology"}],"url":"https://x/a.png","bytes":null}],
            "files":[{"name":"series","type":"zarr fileset","uri":"s3://b/a.zarr"}]}]}"#;
        let data: PickedData = serde_json::from_str(text).unwrap();
        let picture = &data.aio_specimen[0].images[0];
        assert_eq!(
            picture.modality[0].name.as_deref(),
            Some("electrophysiology")
        );
        assert!(picture.bytes.is_none());
        assert_eq!(
            data.aio_specimen[0].files[0].kind.as_deref(),
            Some("zarr fileset")
        );
        // Asked without pictures, the answer has none rather than failing.
        let text = r#"{"aio_specimen":[{"files":null}]}"#;
        let data: PickedData = serde_json::from_str(text).unwrap();
        assert!(data.aio_specimen[0].images.is_empty());
    }

    #[test]
    #[ignore = "reads the live Mouse Patch-seq VIS specimens"]
    fn a_patch_seq_cell_brings_its_pictures_in_the_projects_order() {
        const PROJECT: &str = "1HEYEW7GMUKWIQW37BO";
        let endpoint = "https://idf-api-prod.aibs-idk-prod.net/";
        crate::app::net::block_on(async {
            let specimens = read(endpoint, PROJECT).await.unwrap();
            assert_eq!(specimens.images.len(), 7, "{:?}", specimens.images);
            let at = specimens
                .table
                .rows
                .columns
                .iter()
                .position(|column| column.name == SPECIMEN)
                .unwrap();
            let specimen = specimens.table.rows.rows[0][at].clone();
            let pictures = ask_picked(endpoint, &specimens.scope, specimen, specimens.images)
                .await
                .unwrap()
                .pictures;
            assert!(pictures.len() >= 7);
            assert!(
                pictures
                    .iter()
                    .all(|it| it.width > 0 && it.rgba.len() == (it.width * it.height * 4) as usize)
            );
            assert_eq!(pictures[0].group, "Transcriptomics");
            // A modality's pictures together, wherever the platform put them.
            let groups: Vec<&str> = pictures.iter().map(|it| it.group.as_str()).collect();
            let mut seen: Vec<&str> = Vec::new();
            for group in &groups {
                if seen.last() != Some(group) {
                    assert!(!seen.contains(group), "{groups:?}");
                    seen.push(group);
                }
            }
        });
    }

    #[test]
    #[ignore = "reads the live Genetic Tools Atlas specimens"]
    fn a_genetic_tools_brain_brings_its_store_and_state_but_no_pictures() {
        const PROJECT: &str = "7CVKSF7QGAKIQ8LM5LC";
        let endpoint = "https://idf-api-prod.aibs-idk-prod.net/";
        crate::app::net::block_on(async {
            let specimens = read(endpoint, PROJECT).await.unwrap();
            assert!(specimens.images.is_empty());
            let at = specimens
                .table
                .rows
                .columns
                .iter()
                .position(|column| column.name == SPECIMEN)
                .unwrap();
            let specimen = specimens.table.rows.rows[0][at].clone();
            let answer = ask_picked(endpoint, &specimens.scope, specimen, Vec::new())
                .await
                .unwrap();
            assert!(answer.pictures.is_empty());
            let opened: Vec<&RecordFile> = answer
                .files
                .iter()
                .filter(|file| !crate::formats::discover::datasets_in(&file.address).is_empty())
                .collect();
            assert!(
                opened
                    .iter()
                    .any(|file| file.address.trim_end_matches('/').ends_with(".zarr")),
                "{:?}",
                answer.files
            );
        });
    }
}
