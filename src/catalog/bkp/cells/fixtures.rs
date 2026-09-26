//! Answers as the platform gives them, shared by the tests of more than one
//! part.

use super::*;
use crate::source::properties::CellColumn;

/// How many cells a round of counts came to, however they were grouped.
#[cfg(test)]
pub(super) fn counted(counts: &CellCounts) -> u64 {
    counts
        .values
        .iter()
        .flat_map(|(_, values)| values.iter().map(|(_, count)| *count))
        .sum()
}

pub(super) fn column(id: &str, numeric: bool) -> CellColumn {
    CellColumn {
        id: id.into(),
        name: format!("file {id}"),
        numeric,
    }
}

/// Trimmed from the live answers for SEA-AD CaH.
pub(super) const DISPLAY_TEXT: &str = r#"{
    "defaultColorBy": {"referenceId": "LEVEL_1"},
    "displayFeatures": [
      {"isDefault": true, "priorityOrder": 7,
       "featureType": {"referenceId": "BRAAK", "title": "Braak"}},
      {"isDefault": true, "priorityOrder": 9,
       "featureType": {"referenceId": "CPS", "title": "CPS"}},
      {"isDefault": true, "priorityOrder": 0,
       "featureType": {"referenceId": "TAX", "title": "SEA-AD CaH Taxonomy"},
       "featureSet": [
         {"priorityOrder": 2, "featureType": {"referenceId": "LEVEL_1", "title": "Subclass"}},
         {"priorityOrder": 1, "featureType": {"referenceId": "LEVEL_0", "title": "Neighborhood"}}
       ]},
      {"isDefault": true, "priorityOrder": 3,
       "featureType": {"referenceId": "GENES", "title": "Genes"}}
    ]
}"#;

pub(super) const VALUES_TEXT: &str = r##"[
    {"color": "#d52221", "featureType": {"referenceId": "BRAAK"},
     "featureTypeValueIndex": {"value": "Braak IV", "index": 3, "priorityOrder": 5}},
    {"color": "#fedbcb", "featureType": {"referenceId": "BRAAK"},
     "featureTypeValueIndex": {"value": "Braak 0", "index": 5, "priorityOrder": 1}},
    {"color": "#1655F2", "featureType": {"referenceId": "LEVEL_1"},
     "featureTypeValueIndex": {"value": "STR D1 MSN", "index": 15, "priorityOrder": 1,
                               "referenceId": "SCLA_01", "parentReferenceId": "NEIG_01"}},
    {"color": "#1655F2", "featureType": {"referenceId": "LEVEL_1"},
     "featureTypeValueIndex": {"value": "STR D1 MSN", "index": 15, "priorityOrder": 1,
                               "referenceId": "SCLA_01", "parentReferenceId": "NEIG_01"}},
    {"color": "#8D6C62", "featureType": {"referenceId": "LEVEL_1"},
     "featureTypeValueIndex": {"value": "Endothelial", "index": 1, "priorityOrder": 14,
                               "referenceId": "SCLA_14", "parentReferenceId": "NEIG_07"}},
    {"color": null, "featureType": {"referenceId": "LEVEL_0"},
     "featureTypeValueIndex": {"value": "Neurons", "index": 0, "priorityOrder": 1,
                               "referenceId": "NEIG_01"}},
    {"color": null, "featureType": {"referenceId": "LEVEL_0"},
     "featureTypeValueIndex": {"value": "Vascular", "index": 3, "priorityOrder": 7,
                               "referenceId": "NEIG_07"}},
    {"color": "#000000", "featureType": {"referenceId": "DONOR"},
     "featureTypeValueIndex": {"value": "H20.33.046", "index": 37, "priorityOrder": 24}}
]"##;

pub(super) fn described() -> CellProperties {
    let columns = CellColumns(vec![
        column("DONOR", false),
        column("BRAAK", false),
        column("LEVEL_0", false),
        column("LEVEL_1", false),
        column("CPS", true),
        column("UNKNOWN", false),
    ]);
    build(
        &columns,
        serde_json::from_str(DISPLAY_TEXT).unwrap(),
        serde_json::from_str(VALUES_TEXT).unwrap(),
        &HashMap::from([("CPS".to_string(), (0.0, 1.0))]),
        &HashMap::from([("CPS".to_string(), vec![1, 2, 3])]),
    )
}

pub(super) fn values_counted(counts: &CellCounts, column: &str, code: u16) -> u64 {
    counts
        .values
        .iter()
        .find(|(id, _)| id == column)
        .and_then(|(_, counts)| counts.iter().find(|(found, _)| *found == code))
        .map_or(0, |(_, count)| *count)
}
