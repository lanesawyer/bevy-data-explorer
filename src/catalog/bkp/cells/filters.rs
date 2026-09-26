//! What narrows a count: a rectangle dragged over a frame, a category drilled
//! into, and the values and spans the cell panel leaves ticked.

use super::*;

/// A selected rectangle as the counting queries take it: one condition every
/// counted cell must satisfy.
///
/// The API's only spatial filter is this box. Its field names the
/// visualization whose coordinates the rectangle is in — the same id the
/// Scatterbrain metadata carries, which is what [`SelectedRegion::key`] holds
/// — and its value is the two corners, the far one first. Corner order does
/// not actually matter to the service, but it is written the way the platform
/// writes it so the two can be compared when one of them changes.
pub(super) fn point_filter(region: &SelectedRegion) -> Value {
    let (min, max) = region.corners();
    json!([{
        "type": "POINT",
        "field": format!("{}_point", region.key),
        "operator": "CONTAINED_IN",
        "value": format!("[{},{},{},{}]", max[0], max[1], min[0], min[1]),
    }])
}

/// One category as the counting queries take it, for drilling into a region.
///
/// Named by label, as every metadata condition here is: the API matches the
/// value the platform shows rather than the code the files store.
pub(super) fn facet_filter(within: &RegionFocus) -> Value {
    json!([{
        "type": "METADATA",
        "field": within.column,
        "operator": "EQ",
        "value": within.label,
    }])
}

/// A property's filter as the counting queries take it: the conditions any
/// one of which admits a cell, or nothing if it admits every cell.
///
/// Values are named by label rather than code. A tree names each ticked node
/// at its own level, which is far fewer conditions than the finest codes
/// those ticks admit.
pub(super) fn cell_filter(property: &CellProperty) -> Option<Value> {
    if !property.restricts() {
        return None;
    }
    let condition = |field: &str, operator: &str, value: String| {
        let kind = if property.gene.is_some() {
            "GENE"
        } else {
            "METADATA"
        };
        json!({ "type": kind, "field": field, "operator": operator, "value": value })
    };
    let conditions: Vec<Value> = match &property.kind {
        PropertyKind::Categorical(values) => values
            .iter()
            .filter(|value| value.selected)
            .map(|value| condition(&property.id, "EQ", value.label.clone()))
            .collect(),
        PropertyKind::Tree(tree) => tree
            .nodes
            .iter()
            .filter(|node| node.value.selected)
            .filter_map(|node| {
                let level = tree.levels.get(node.level)?;
                Some(condition(&level.id, "EQ", node.value.label.clone()))
            })
            .collect(),
        PropertyKind::Numeric(range) => {
            // Ranges here leave out their high end, and the one on screen
            // keeps it.
            let to = f64::from(range.to);
            let past = to + (to.abs() * 1e-6).max(1e-9);
            let field = property
                .gene
                .map_or_else(|| property.id.clone(), |index| index.to_string());
            vec![condition(
                &field,
                "BETWEEN",
                format!("[{},{past}]", range.from),
            )]
        }
    };
    Some(json!(conditions))
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;
    use crate::source::properties::RangeEnd;

    #[test]
    fn a_region_is_asked_for_as_the_box_the_api_takes() {
        // The API rejects anything but `[x,y,x,y]`, and names the field after
        // the visualization rather than the dataset; both were found by asking
        // it, so a change here has to be checked against it again.
        let region = SelectedRegion {
            key: "MGA5LUTH4ETM859L5IM".into(),
            min: Vec2::new(20.0, 30.0),
            max: Vec2::new(40.0, 50.0),
        };
        let filter = point_filter(&region);
        let condition = &filter[0];
        assert_eq!(condition["type"], "POINT");
        assert_eq!(condition["operator"], "CONTAINED_IN");
        assert_eq!(condition["field"], "MGA5LUTH4ETM859L5IM_point");
        assert_eq!(condition["value"], "[40,50,20,30]");
    }

    #[test]
    fn a_region_dragged_backwards_asks_the_same_question() {
        let key = "V".to_string();
        let forward = point_filter(&SelectedRegion {
            key: key.clone(),
            min: Vec2::new(20.0, 30.0),
            max: Vec2::new(40.0, 50.0),
        });
        let backward = point_filter(&SelectedRegion {
            key,
            min: Vec2::new(40.0, 50.0),
            max: Vec2::new(20.0, 30.0),
        });
        assert_eq!(forward, backward);
    }

    #[test]
    fn a_filter_names_values_by_label_and_ticked_nodes_at_their_level() {
        let mut properties = described();
        assert!(
            properties
                .properties
                .iter()
                .all(|p| cell_filter(p).is_none())
        );

        let tree = properties.properties[0].tree_mut().unwrap();
        let neurons = tree.children(None).next().unwrap();
        tree.set(neurons, true);
        assert_eq!(
            cell_filter(&properties.properties[0]),
            Some(json!([
                { "type": "METADATA", "field": "LEVEL_0", "operator": "EQ", "value": "Neurons" }
            ]))
        );

        let cps = properties.properties[2].range_mut().unwrap();
        cps.set_end(RangeEnd::To, 0.5);
        let filter = cell_filter(&properties.properties[2]).unwrap();
        let range = filter[0]["value"].as_str().unwrap();
        assert!(range.starts_with("[0,0.5000"), "{range} keeps its high end");
        assert_eq!(filter[0]["operator"], "BETWEEN");
    }

    #[test]
    fn a_gene_filters_by_its_index() {
        let property = CellProperty {
            id: "ENSG00000128683".into(),
            name: "GAD1".into(),
            shown: true,
            kind: PropertyKind::Numeric(NumericRange {
                from: 1.0,
                ..NumericRange::full(0.0, 4.0, Vec::new())
            }),
            gene: Some(11618),
        };
        let filter = cell_filter(&property).unwrap();
        assert_eq!(filter[0]["type"], "GENE");
        assert_eq!(filter[0]["field"], "11618");
    }
}
