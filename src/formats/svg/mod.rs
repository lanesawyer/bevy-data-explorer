//! SVG annotations: outlines drawn over a slide in that slide's own pixels.
//!
//! A source like any other, so it can be opened into a frame of its own, but
//! what it is for is being layered onto the image it annotates. Its unit is
//! the pixel, which is what lets it share a frame with a Deep Zoom image of the
//! same size.
//!
//! Nothing streams. An annotation document is a few hundred kilobytes at most,
//! so the whole of it is one mesh, built once the source has a render layer.

pub mod parse;

use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::render::lines::{LineMaterial, Stroke, build_line_mesh};
use crate::source::hover::{HoverInfo, HoverProbe};
use crate::source::{self, DataSource, SourceExtent, SourceStatus};
use parse::Svg;

/// How near an outline the pointer has to be to name it, in screen pixels.
const PICK_PX: f32 = 6.0;

/// The outlines a source draws, and whether they have been drawn yet.
#[derive(Component)]
pub struct SvgOutlines {
    svg: Arc<Svg>,
    built: bool,
}

/// Marks the mesh holding a document's outlines.
#[derive(Component)]
pub struct SvgMesh;

/// Build each document's mesh on the layer its source was given.
fn build_outlines(
    mut commands: Commands,
    mut outlines: Query<(&mut SvgOutlines, &DataSource, &mut SourceStatus)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<LineMaterial>>,
) {
    for (mut outlines, source, mut status) in &mut outlines {
        if outlines.built {
            continue;
        }
        let svg = outlines.svg.clone();
        // Display coordinates negate y, as every other source does.
        let flipped: Vec<Vec<Vec2>> = svg
            .shapes
            .iter()
            .map(|shape| {
                shape
                    .points
                    .iter()
                    .map(|[x, y]| Vec2::new(*x, -*y))
                    .collect()
            })
            .collect();
        let mesh = build_line_mesh(
            svg.shapes
                .iter()
                .zip(&flipped)
                .map(|(shape, points)| Stroke {
                    points,
                    closed: shape.closed,
                    color: shape.color,
                    width: shape.width,
                    dash: shape.dash,
                }),
        );
        commands.spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(materials.add(LineMaterial::default())),
            Transform::IDENTITY,
            RenderLayers::layer(source.layer),
            SvgMesh,
        ));
        outlines.built = true;

        status.0 = format!("{} outlines in {} groups", svg.shapes.len(), svg.groups);
        if svg.skipped > 0 {
            status.0 += &format!(", {} skipped (transformed or curved)", svg.skipped);
        }
    }
}

/// Name the outline under the pointer: the one whose stroke it is on, or else
/// the innermost region it is inside.
fn resolve_hover(
    outlines: Query<(Entity, &SvgOutlines)>,
    probes: Query<&HoverProbe>,
    mut infos: Query<&mut HoverInfo>,
) {
    for (source, outlines) in &outlines {
        let Ok(mut info) = infos.get_mut(source) else {
            continue;
        };
        let next = probes
            .get(source)
            .ok()
            .and_then(|probe| describe(&outlines.svg, probe))
            .unwrap_or_default();
        info.set_if_neq(next);
    }
}

fn describe(svg: &Svg, probe: &HoverProbe) -> Option<HoverInfo> {
    let at = [probe.world.x, -probe.world.y];
    let reach = probe.radius(PICK_PX);

    let on_stroke = svg
        .shapes
        .iter()
        .map(|shape| (shape.distance_to_outline(at) - shape.width * 0.5, shape))
        .filter(|(distance, _)| *distance <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, shape)| shape);
    let shape = on_stroke.or_else(|| {
        svg.shapes
            .iter()
            .filter(|shape| shape.contains(at))
            .min_by(|a, b| a.area().total_cmp(&b.area()))
    })?;

    let title = shape.title().unwrap_or("outline").to_string();
    let mut info = HoverInfo::titled(title.clone());
    for (key, value) in &shape.labels {
        if *value != title {
            info = info.row(key.trim_end_matches("-label"), value);
        }
    }
    Some(info)
}

/// The systems every SVG document shares, registered once however many are
/// open.
pub struct SvgSystems;

impl Plugin for SvgSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, build_outlines.in_set(Stage::Sources))
            .add_systems(Update, resolve_hover.in_set(source::hover::HoverProbing));
    }
}

/// Register a parsed document as a source.
pub fn spawn_source(world: &mut World, svg: Arc<Svg>) -> Entity {
    let (width, height) = (svg.width, svg.height);
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: svg.name.clone(),
            unit: "px".into(),
            detail: format!("SVG annotations, {} x {} px", width, height),
            stat: format!("{} OUTLINES", svg.shapes.len()),
        },
        SourceExtent {
            centre: Vec2::new(width * 0.5, -height * 0.5),
            size: Vec2::new(width, height),
            finest: 1.0 / 8.0,
        },
    );
    world
        .entity_mut(source)
        .insert((HoverInfo::default(), SvgOutlines { svg, built: false }));
    source
}

/// Name a document after where it came from, stepping up past a file name
/// that says only that it is an annotation.
pub fn label_for(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let mut segments = path.rsplit('/').filter(|segment| !segment.is_empty());
    let stem = segments.next().map_or("Annotations", |file| {
        file.trim_end_matches(".svg").trim_end_matches(".SVG")
    });
    if ["annotation", "annotations"].contains(&stem.to_ascii_lowercase().as_str())
        && let Some(parent) = segments.next()
    {
        return format!("{parent} annotations");
    }
    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(x: f32, y: f32) -> HoverProbe {
        HoverProbe {
            panel: Entity::PLACEHOLDER,
            world: Vec2::new(x, -y),
            units_per_px: 1.0,
        }
    }

    fn fixture() -> Svg {
        parse::parse(
            "annotation",
            include_str!("../../../testdata/annotation.svg"),
        )
        .unwrap()
    }

    #[test]
    fn hovering_a_region_names_its_structure_and_region() {
        let svg = fixture();
        // Inside the small DG-R outline, which starts at 7179.25, 4301.5.
        let info = describe(&svg, &probe(7150.0, 4300.0)).unwrap();
        assert_eq!(info.title, "DG-R");
        assert!(
            info.rows
                .contains(&("region".to_string(), "HIP-MEC".to_string()))
        );
    }

    #[test]
    fn nothing_is_named_away_from_every_outline() {
        assert!(describe(&fixture(), &probe(100.0, 100.0)).is_none());
    }

    #[test]
    fn a_generic_file_name_is_named_after_its_folder() {
        assert_eq!(
            label_for("https://bucket/H20.33.040-A12-I6-primary/annotation.svg"),
            "H20.33.040-A12-I6-primary annotations"
        );
        assert_eq!(label_for("/data/regions.svg"), "regions");
    }
}
