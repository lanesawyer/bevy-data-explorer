//! Which columns a specimen table has and in what order, and how a specimen
//! becomes a row under them.

use super::*;

/// One feature a specimen can carry, and so one column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Feature {
    /// What the platform calls it, which is what a filter narrows by. Empty
    /// when the platform gave none, and then it cannot be asked for.
    pub(super) id: String,
    /// What the column is headed.
    pub(super) title: String,
    /// The unit it is recorded in, which names the column rather than every
    /// cell in it.
    pub(super) unit: Option<String>,
    /// A measurement rather than an annotation: a number read off its value
    /// rather than taxa listed, and worth a span when the platform cannot
    /// list it.
    pub(super) measured: bool,
    /// Left out when the table opens, because the platform's own page leaves
    /// it out. Still a column, offered in the frame's menu.
    pub(super) hidden: bool,
    /// Whether the platform narrows by it as a span of numbers rather than by
    /// values, if it says.
    pub(super) spanned: Option<bool>,
}

impl Feature {
    /// What a feature is told apart by: its id, or its title when it has
    /// none. Not the title alone, since two features can share one — a
    /// BICAN donor carries two called "Age of Death".
    fn key(&self) -> &str {
        if self.id.is_empty() {
            &self.title
        } else {
            &self.id
        }
    }

    /// Whether this is the feature `feature_type` names.
    fn names(&self, feature_type: &Titled) -> bool {
        match feature_type
            .reference_id
            .as_deref()
            .filter(|id| !id.is_empty())
        {
            Some(id) => id == self.id,
            None => self.id.is_empty() && feature_type.title.as_deref() == Some(&self.title),
        }
    }
}

/// Which columns a specimen table has, and in what order.
///
/// Settled before the rows are drawn and kept, so turning the page does not
/// relay out the table under the pointer. A later page carrying a feature no
/// earlier one had grows the plan rather than losing the value — columns only
/// ever gain, which the frame notices and rebuilds for.
///
/// When the platform says which features a kind of specimen is shown with,
/// the plan is laid out from that, in its order. Otherwise it is settled from
/// the first page and ordered so a table reads the same way twice: what
/// identifies a specimen first, then annotations by name, then measurements
/// by name. The platform's own order of a specimen's features is not stable
/// between requests.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub(super) features: Vec<Feature>,
    /// Laid out by the platform, so what a page adds goes on the end rather
    /// than being sorted in among what it chose.
    laid_out: bool,
    /// Of one kind of specimen, so the column saying which kind holds the
    /// same word all the way down.
    one_kind: bool,
}

impl Plan {
    #[cfg(test)]
    pub(super) fn of(specimens: &[Specimen]) -> Plan {
        let mut plan = Plan::default();
        plan.extend(specimens);
        plan
    }

    /// A plan in the order the platform gives it.
    pub(super) fn laid_out(features: Vec<Feature>) -> Plan {
        let mut plan = Plan {
            features: Vec::new(),
            laid_out: true,
            one_kind: false,
        };
        for feature in features {
            if !plan
                .features
                .iter()
                .any(|known| known.key() == feature.key())
            {
                plan.features.push(feature);
            }
        }
        plan
    }

    /// The same plan, for a table of one kind of specimen.
    pub(super) fn of_one_kind(self) -> Plan {
        Plan {
            one_kind: true,
            ..self
        }
    }

    /// A sort the platform wrote as fields, as the table's headings. A field
    /// the plan has no column for is left out.
    pub(super) fn keys_of(&self, fields: &[(String, bool)]) -> Vec<SortKey> {
        let columns: Vec<(String, String)> =
            self.fields().into_iter().zip(self.headers()).collect();
        fields
            .iter()
            .filter_map(|(field, descending)| {
                let (_, heading) = columns
                    .iter()
                    .find(|(known, _)| !known.is_empty() && known == field)?;
                Some(SortKey {
                    column: heading.clone(),
                    descending: *descending,
                })
            })
            .collect()
    }

    /// Add any feature these specimens carry that the plan has not got.
    ///
    /// Returns whether anything was added, since that is what makes the
    /// columns on screen wrong until they are rebuilt.
    pub(super) fn extend(&mut self, specimens: &[Specimen]) -> bool {
        let mut found: Vec<Feature> = Vec::new();
        for specimen in specimens {
            let annotations = specimen
                .annotations
                .iter()
                .map(|it| (&it.feature_type, None, false));
            // The unit belongs to the feature rather than to the reading, so
            // the first one seen names the column.
            let measurements = specimen.measurements.iter().map(|it| {
                (
                    &it.feature_type,
                    it.unit.as_deref().filter(|unit| !unit.is_empty()),
                    true,
                )
            });
            for (feature_type, unit, measured) in annotations.chain(measurements) {
                let Some(title) = feature_type.title.as_deref() else {
                    continue;
                };
                let known = |feature: &Feature| feature.names(feature_type);
                if self.features.iter().any(known) || found.iter().any(known) {
                    continue;
                }
                found.push(Feature {
                    id: feature_type.reference_id.clone().unwrap_or_default(),
                    title: title.to_string(),
                    unit: unit.map(str::to_string),
                    measured,
                    hidden: false,
                    spanned: None,
                });
            }
        }
        if found.is_empty() {
            return false;
        }
        let by_name =
            |a: &Feature, b: &Feature| (a.measured, &a.title).cmp(&(b.measured, &b.title));
        if self.laid_out {
            found.sort_by(by_name);
            self.features.extend(found);
        } else {
            self.features.extend(found);
            self.features.sort_by(by_name);
        }
        true
    }

    /// Every feature that could be narrowed by: those the platform named.
    pub(super) fn columns(&self) -> Vec<Feature> {
        self.features
            .iter()
            .filter(|feature| !feature.id.is_empty())
            .cloned()
            .collect()
    }

    /// A sort on the table's headings, as the platform's fields.
    ///
    /// A heading the plan has no field for is left out rather than refusing
    /// the sort, and a sort left with nothing is no sort at all.
    pub(super) fn sort(&self, keys: &[SortKey]) -> Value {
        let fields: Vec<(String, String)> = self.headers().into_iter().zip(self.fields()).collect();
        let sort: Vec<Value> = keys
            .iter()
            .filter_map(|key| {
                let (_, field) = fields.iter().find(|(heading, _)| *heading == key.column)?;
                (!field.is_empty()).then(|| {
                    json!({
                        "field": field,
                        "order": if key.descending { "DESC" } else { "ASC" },
                    })
                })
            })
            .collect();
        if sort.is_empty() {
            Value::Null
        } else {
            Value::Array(sort)
        }
    }

    /// What the platform calls each column, in the order of [`Plan::headers`].
    pub(super) fn fields(&self) -> Vec<String> {
        let mut fields = vec![SPECIMEN_FIELD.to_string(), KIND_FIELD.to_string()];
        fields.extend(self.features.iter().map(|feature| feature.id.clone()));
        fields
    }

    /// Whether each column, in the order of [`Plan::headers`], is left out
    /// when the table opens.
    ///
    pub(super) fn hidden(&self) -> Vec<bool> {
        let mut hidden = vec![false, self.one_kind];
        hidden.extend(self.features.iter().map(|feature| feature.hidden));
        hidden
    }

    pub(super) fn headers(&self) -> Vec<String> {
        let mut headers = vec![SPECIMEN.to_string(), KIND.to_string()];
        headers.extend(self.features.iter().map(|feature| match &feature.unit {
            Some(unit) => format!("{} ({unit})", feature.title),
            None => feature.title.clone(),
        }));
        headers
    }

    pub(super) fn rows(&self, specimens: &[Specimen]) -> Vec<Vec<String>> {
        specimens
            .iter()
            .map(|specimen| {
                let mut row = vec![
                    specimen
                        .crid
                        .as_ref()
                        .and_then(|crid| crid.symbol.clone())
                        .unwrap_or_default(),
                    specimen
                        .specimen_type
                        .as_ref()
                        .and_then(|kind| kind.name.clone())
                        .unwrap_or_default(),
                ];
                row.extend(self.features.iter().map(|feature| {
                    if feature.measured {
                        measured(specimen, feature)
                    } else {
                        annotated(specimen, feature)
                    }
                }));
                row
            })
            .collect()
    }
}

/// What a specimen is annotated with under `feature`.
///
/// An annotation can name more than one taxon — a donor with two clinical
/// diagnoses has both — so they are listed rather than one being picked.
pub(super) fn annotated(specimen: &Specimen, feature: &Feature) -> String {
    let mut values: Vec<&str> = specimen
        .annotations
        .iter()
        .filter(|annotation| feature.names(&annotation.feature_type))
        .flat_map(|annotation| annotation.taxons.iter())
        .filter_map(|taxon| taxon.symbol.as_deref())
        .filter(|symbol| !symbol.is_empty())
        .collect();
    values.dedup();
    values.join(", ")
}

/// What a specimen measured for `feature`.
///
/// The first reading, because the platform repeats some of them: the SEA-AD
/// donors each carry sex and age at death twice, with the same value both
/// times. Listing a value beside itself would say something the data does not.
pub(super) fn measured(specimen: &Specimen, feature: &Feature) -> String {
    specimen
        .measurements
        .iter()
        .find(|measurement| feature.names(&measurement.feature_type))
        .and_then(|measurement| measurement.value.clone())
        .unwrap_or_default()
}

/// Read a saved answer, for tests and for anything that has the JSON already.
#[cfg(test)]
pub(super) fn from_answer(name: &str, text: &str) -> Result<Table, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let data: Data = serde_json::from_value(value["data"].clone()).map_err(|e| e.to_string())?;
    let plan = Plan::of(&data.aio_specimen);
    Ok(Table::paged(
        name,
        "test",
        plan.headers(),
        plan.rows(&data.aio_specimen),
        None,
        data.counted(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Table {
        from_answer("SEA-AD", include_str!("../../../testdata/specimens.json")).unwrap()
    }

    fn names(table: &Table) -> Vec<&str> {
        table
            .rows
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect()
    }

    fn column(table: &Table, name: &str) -> usize {
        names(table).iter().position(|it| *it == name).unwrap()
    }

    #[test]
    fn a_specimen_becomes_a_row_led_by_its_accession() {
        let table = fixture();
        assert_eq!(names(&table)[0], SPECIMEN);
        assert_eq!(table.rows.rows.len(), 3);
        assert_eq!(table.rows.cell(0, 0), "H21.33.012");
        assert_eq!(table.rows.cell(0, column(&table, KIND)), "sample");
    }

    #[test]
    fn every_feature_any_specimen_carries_gets_a_column() {
        // MoCA is recorded for one of the three, and is a column all the same;
        // the two without it are empty there rather than absent.
        let table = fixture();
        let moca = column(&table, "MoCA score");
        let recorded: Vec<&str> = (0..3).map(|row| table.rows.cell(row, moca)).collect();
        assert_eq!(recorded.iter().filter(|it| !it.is_empty()).count(), 1);
    }

    #[test]
    fn a_measurement_names_its_unit_in_the_column_rather_than_every_cell() {
        let table = fixture();
        let age = column(&table, "Age at death (years)");
        assert_eq!(table.rows.cell(0, age), "93");
        // And one with no unit is named plainly.
        assert_eq!(table.rows.cell(0, column(&table, "Sex")), "Female");
    }

    #[test]
    fn a_measurement_the_platform_repeats_is_reported_once() {
        // The SEA-AD donors carry sex and age at death twice over, with the
        // same value both times.
        let table = fixture();
        assert_eq!(table.rows.cell(0, column(&table, "Sex")), "Female");
    }

    #[test]
    fn an_annotation_with_more_than_one_taxon_lists_them() {
        let table = fixture();
        let diagnosis = column(&table, "Consensus clinical diagnosis");
        assert_eq!(
            table.rows.cell(0, diagnosis),
            "Multiple System Atrophy, Other"
        );
    }

    #[test]
    fn the_columns_come_out_in_the_same_order_twice() {
        // The platform's own order changes between requests, so the table
        // fixes one: what identifies a specimen, then its annotations by
        // name, then its measurements by name.
        assert_eq!(
            names(&fixture()),
            [
                SPECIMEN,
                KIND,
                "Cognitive status",
                "Consensus clinical diagnosis",
                "Donor ID",
                "Race/ ethnicity",
                "Age at death (years)",
                "Brain pH",
                "MoCA score",
                "Sex",
            ]
        );
    }

    #[test]
    fn a_sort_on_headings_is_asked_for_by_the_platforms_fields() {
        let feature = |id: &str, title: &str, unit: Option<&str>| Feature {
            id: id.into(),
            title: title.into(),
            unit: unit.map(Into::into),
            measured: unit.is_some(),
            hidden: false,
            spanned: None,
        };
        let plan = Plan::laid_out(vec![
            feature("MM1MMES48T9H7ZX6E3Y", "Cognitive status", None),
            // A feature the platform gave no id cannot be asked for.
            feature("", "Donor ID", None),
            feature("HPEYHZG6D7XY8CBK448", "Age at death", Some("years")),
        ]);
        let key = |column: &str, descending: bool| SortKey {
            column: column.into(),
            descending,
        };
        let sort = plan.sort(&[
            key("Cognitive status", false),
            key("Age at death (years)", true),
            key(SPECIMEN, false),
            key("Donor ID", false),
            key("Not a column", false),
        ]);
        // The unit is part of the heading, not of the field.
        assert_eq!(
            sort,
            json!([
                { "field": "MM1MMES48T9H7ZX6E3Y", "order": "ASC" },
                { "field": "HPEYHZG6D7XY8CBK448", "order": "DESC" },
                { "field": SPECIMEN_FIELD, "order": "ASC" },
            ])
        );
        assert_eq!(plan.sort(&[]), Value::Null);
    }

    #[test]
    fn a_numeric_column_is_told_from_a_worded_one() {
        let table = fixture();
        assert!(table.rows.columns[column(&table, "Age at death (years)")].numeric);
        assert!(!table.rows.columns[column(&table, "Sex")].numeric);
    }

    fn specimens(text: &str) -> Vec<Specimen> {
        let value: Value = serde_json::from_str(text).unwrap();
        let data: Data = serde_json::from_value(value["data"].clone()).unwrap();
        data.aio_specimen
    }

    #[test]
    fn a_page_read_later_fills_the_columns_the_first_one_settled() {
        // What stops the table relaying itself out every time a page turns.
        let all = specimens(include_str!("../../../testdata/specimens.json"));
        let mut plan = Plan::of(&all[..1]);
        let settled = plan.headers();
        assert!(!plan.extend(&all[..1]), "the same page adds nothing");
        assert_eq!(plan.headers(), settled);

        // And every row of a later page comes out the same width.
        for row in plan.rows(&all[1..]) {
            assert_eq!(row.len(), settled.len());
        }
    }

    #[test]
    fn a_feature_first_seen_on_a_later_page_gains_a_column() {
        // Losing the value would be the alternative, and columns only ever
        // gain, so the frame notices and lays itself out again.
        let all = specimens(include_str!("../../../testdata/specimens.json"));
        let with_moca = all
            .iter()
            .position(|it| {
                it.measurements
                    .iter()
                    .any(|m| m.feature_type.title.as_deref() == Some("MoCA score"))
            })
            .expect("one of the fixture donors has a MoCA score");
        let without: Vec<&Specimen> = all
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != with_moca)
            .map(|(_, it)| it)
            .collect();

        // A plan built without that donor has no column for it.
        let mut plan = Plan::default();
        for specimen in &without {
            plan.extend(std::slice::from_ref(*specimen));
        }
        assert!(!plan.headers().iter().any(|name| name == "MoCA score"));

        // Reading the page that has one adds it.
        assert!(plan.extend(&all[with_moca..=with_moca]));
        assert!(plan.headers().iter().any(|name| name == "MoCA score"));
    }
}
