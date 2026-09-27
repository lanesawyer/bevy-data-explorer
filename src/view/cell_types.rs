//! Cells of one type picked out in every frame at once.
//!
//! Hovering a cell draws every cell sharing its value — of whatever its
//! points are colored by — larger, across every frame showing that dataset.
//! It does the same in every other dataset that places its cells in the
//! same taxonomy: cells measured in tissue and cells sequenced apart are
//! never the same cells, but a publisher names the same types by the same
//! ids in both, so pointing at an astrocyte in a section picks out the
//! astrocytes of a UMAP beside it.
//!
//! Every frame takes part, linked or not: the highlight changes nothing but
//! how large points are drawn, and lasts only while the pointer is there.

use bevy::prelude::*;

use crate::app::schedule::Stage;
use crate::render::points::SourceHighlight;
use crate::source::hover::HoveredCategory;
use crate::source::properties::CellProperties;

/// Highlight the hovered value in its own dataset, and the same type of cell
/// in every other.
fn link_cell_types(
    hovered: Query<(Entity, &HoveredCategory, &CellProperties)>,
    mut highlights: Query<(Entity, Option<&CellProperties>, &mut SourceHighlight)>,
) {
    // The pointer is over one frame, so at most one source says what it is
    // over; two frames of one source say it together.
    let over = hovered
        .iter()
        .find_map(|(source, category, properties)| Some((source, category.0?, properties)));
    for (source, properties, mut highlight) in &mut highlights {
        let wanted = match over {
            Some((from, code, _)) if from == source => Some(code),
            Some((_, code, other)) => {
                properties.and_then(|properties| properties.same_type_as(other, code))
            }
            None => None,
        };
        highlight.set_if_neq(SourceHighlight(wanted));
    }
}

pub struct CellTypePlugin;

impl Plugin for CellTypePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, link_cell_types.in_set(Stage::LinkHover));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::properties::{CellProperty, PropertyKind, PropertyValue};

    fn colored_by(id: &str, values: &[(u16, &str)]) -> CellProperties {
        CellProperties::ready(vec![CellProperty {
            id: id.into(),
            name: id.into(),
            shown: true,
            gene: None,
            kind: PropertyKind::Categorical(
                values
                    .iter()
                    .map(|(code, reference)| PropertyValue {
                        code: *code,
                        label: reference.to_string(),
                        reference: Some(reference.to_string()),
                        color: None,
                        count: None,
                        selected: false,
                    })
                    .collect(),
            ),
        }])
    }

    #[test]
    fn hovering_a_cell_picks_out_its_type_here_and_in_every_dataset_that_names_it() {
        let mut app = App::new();
        app.add_systems(Update, link_cell_types);
        let tissue = app
            .world_mut()
            .spawn((
                colored_by("SUBCLASS", &[(1, "astro"), (2, "oligo")]),
                HoveredCategory(Some(2)),
                SourceHighlight::default(),
            ))
            .id();
        let umap = app
            .world_mut()
            .spawn((
                colored_by("SUBCLASS", &[(7, "oligo"), (8, "astro")]),
                HoveredCategory::default(),
                SourceHighlight::default(),
            ))
            .id();
        let unrelated = app
            .world_mut()
            .spawn((
                colored_by("REGION", &[(2, "cortex")]),
                HoveredCategory::default(),
                SourceHighlight::default(),
            ))
            .id();
        app.update();
        let highlight = |app: &App, source| app.world().get::<SourceHighlight>(source).unwrap().0;
        assert_eq!(highlight(&app, tissue), Some(2));
        assert_eq!(
            highlight(&app, umap),
            Some(7),
            "the oligodendrocytes, by id"
        );
        assert_eq!(
            highlight(&app, unrelated),
            None,
            "the same code means nothing here"
        );

        app.world_mut()
            .get_mut::<HoveredCategory>(tissue)
            .unwrap()
            .0 = None;
        app.update();
        assert_eq!(highlight(&app, tissue), None);
        assert_eq!(highlight(&app, umap), None);
    }
}
