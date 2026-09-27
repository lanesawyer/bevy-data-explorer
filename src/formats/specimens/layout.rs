//! How the platform lays a project's specimens out: which features are
//! columns, in what order, which are shown when the table opens, and what the
//! rows are sorted by — for the whole project, or for each kind of specimen a
//! project shows apart, with the one on screen swapped for another when the
//! frame asks.

use super::*;

/// One table as the platform lays it out: its columns, and the order its rows
/// open in.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Layout {
    pub(super) plan: Plan,
    pub(super) sort: Vec<SortKey>,
    /// The pictures a specimen may have, by title, in the order to show them.
    /// Empty for a table whose specimens have none.
    pub(super) images: Vec<String>,
}

/// How a project is laid out, whichever way it is.
#[derive(Default)]
pub(super) struct Layouts {
    /// Each kind the project shows apart, in the platform's order, with how
    /// many specimens it holds. Empty for a project shown as one table.
    pub(super) kinds: Vec<(TablePartition, Layout)>,
    /// The one table of a project that shows no kinds apart, if the platform
    /// lays it out at all.
    pub(super) whole: Option<Layout>,
}

pub(super) fn layouts_of(data: LayoutData) -> Layouts {
    let counts: HashMap<String, u64> = labeled(&data.counts).into_iter().collect();
    let mut kinds = data.kinds;
    kinds.sort_by_key(|kind| kind.priority_order.unwrap_or(i64::MAX));
    let kinds = kinds
        .into_iter()
        .map(|kind| {
            let layout = layout_of(kind.display_features, kind.default_sort.as_deref(), true);
            let partition = TablePartition {
                name: kind
                    .title
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| kind.reference_id.clone()),
                count: counts.get(&kind.reference_id).copied(),
                id: kind.reference_id,
            };
            (partition, layout)
        })
        .collect();
    let whole = data
        .whole
        .filter(|whole| !whole.display_features.is_empty())
        .map(|whole| layout_of(whole.display_features, whole.default_sort.as_deref(), false));
    Layouts { kinds, whole }
}

/// A table laid out from the platform's display features and default sort.
///
/// A display feature that is neither an annotation nor a measurement is left
/// out of the columns, since a row is read from those two alone. An image is
/// kept apart, as a picture the specimen may have.
fn layout_of(mut features: Vec<DisplayFeature>, sort: Option<&str>, one_kind: bool) -> Layout {
    features.sort_by_key(|feature| feature.priority_order.unwrap_or(i64::MAX));
    let images = features
        .iter()
        .filter(|feature| feature.kind.as_deref() == Some("IMAGE"))
        .filter_map(|feature| feature.feature_type.title.clone())
        .collect();
    let plan = Plan::laid_out(
        features
            .into_iter()
            .filter_map(|feature| {
                let measured = match feature.kind.as_deref()? {
                    "ANNOTATION" => false,
                    "MEASUREMENT" => true,
                    _ => return None,
                };
                Some(Feature {
                    id: feature.feature_type.reference_id.unwrap_or_default(),
                    title: feature.feature_type.title?,
                    unit: feature.unit.filter(|unit| !unit.is_empty()),
                    measured,
                    hidden: feature.is_default == Some(false),
                    spanned: feature
                        .filter_operator
                        .as_deref()
                        .map(|operator| operator == "BETWEEN"),
                    extent: feature
                        .measurement_stats
                        .and_then(|stats| stats.min.zip(stats.max))
                        .filter(|(min, max)| min <= max),
                })
            })
            .collect(),
    );
    let plan = if one_kind { plan.of_one_kind() } else { plan };
    let sort = plan.keys_of(&sort_fields(sort));
    Layout { plan, sort, images }
}

/// The fields a default sort names, and whether each is descending.
///
/// The platform writes it as a string holding a JSON list. One it cannot be
/// read from sorts by nothing, which is how the table would open anyway.
fn sort_fields(sort: Option<&str>) -> Vec<(String, bool)> {
    #[derive(Deserialize)]
    struct Field {
        field: String,
        #[serde(default)]
        order: Option<String>,
    }
    let Some(sort) = sort.filter(|it| !it.trim().is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<Field>>(sort) {
        Ok(fields) => fields
            .into_iter()
            .map(|it| {
                let descending = it
                    .order
                    .is_some_and(|order| order.eq_ignore_ascii_case("desc"));
                (it.field, descending)
            })
            .collect(),
        Err(e) => {
            warn!("reading the platform's default sort {sort:?}: {e}");
            Vec::new()
        }
    }
}

/// The columns a plan heads a table with, before any row has been measured.
pub(super) fn columns_of(plan: &Plan) -> Vec<TableColumn> {
    plan.headers()
        .into_iter()
        .zip(plan.hidden())
        .map(|(name, hidden_by_default)| TableColumn {
            chars: name.chars().count(),
            name,
            numeric: false,
            hidden_by_default,
        })
        .collect()
}

/// Put the kind a frame has asked for on screen.
///
/// A kind is a different table, not a narrower one: other columns, so other
/// filters and the kind's own default sort, and its own count. Everything the last kind had
/// is dropped and asked for again, and the rows are emptied until the first
/// page of the new kind lands, rather than drawing one kind's rows under
/// another's headings.
pub(super) fn serve_kinds(
    mut sources: Query<(
        &mut SpecimenPages,
        &TablePartitions,
        &mut SourceTable,
        &mut TablePaging,
        &mut SourceStatus,
        Option<&mut TableFilters>,
        Option<&mut TableSort>,
    )>,
) {
    for (mut pages, partitions, mut rows, mut paging, mut status, filters, sort) in &mut sources {
        if pages.scope.kind.as_deref() == Some(partitions.chosen.as_str()) {
            continue;
        }
        let Some(layout) = pages.layout(&partitions.chosen) else {
            continue;
        };
        let kind = partitions.chosen();
        info!(
            "showing {} specimens",
            kind.map_or(partitions.chosen.as_str(), |kind| kind.name.as_str())
        );
        pages.show_kind(partitions.chosen.clone(), layout.plan);

        *rows = SourceTable {
            columns: columns_of(&pages.plan),
            rows: Vec::new(),
            first: 0,
        };
        let total = kind.and_then(|kind| kind.count).map(|count| count as usize);
        *paging = TablePaging::new(paging.size, total);
        status.0 =
            super::super::table::status_of(total.unwrap_or_default(), rows.columns.len(), None);
        if let Some(mut filters) = filters {
            *filters = TableFilters::pending();
        }
        if let Some(mut sort) = sort {
            sort.set_if_neq(TableSort(layout.sort));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn parse(text: &str) -> Layouts {
        let value: Value = serde_json::from_str(text).unwrap();
        layouts_of(serde_json::from_value(value["data"].clone()).unwrap())
    }

    /// The BICAN Rapid Release Inventory: library aliquots and donors apart.
    fn layouts() -> Layouts {
        parse(include_str!("../../../testdata/specimen_layout_bican.json"))
    }

    fn kinds() -> Vec<(TablePartition, Plan)> {
        layouts()
            .kinds
            .into_iter()
            .map(|(kind, layout)| (kind, layout.plan))
            .collect()
    }

    /// Neurons in mouse primary visual cortex: one table, sorted, most of it
    /// hidden.
    fn whole() -> Layout {
        parse(include_str!("../../../testdata/specimen_layout_v1.json"))
            .whole
            .expect("the project is laid out whole")
    }

    fn hidden(plan: &Plan) -> Vec<String> {
        plan.headers()
            .into_iter()
            .zip(plan.hidden())
            .filter(|(_, hidden)| *hidden)
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn a_project_shown_whole_opens_as_the_platform_lays_it_out() {
        let layout = whole();
        assert!(
            layouts().whole.is_none(),
            "a project shown apart has no whole"
        );
        assert!(
            parse(include_str!("../../../testdata/specimen_layout_v1.json"))
                .kinds
                .is_empty()
        );
        // Its images are not columns; its annotations and measurements are,
        // in the platform's order.
        let headers = layout.plan.headers();
        assert_eq!(headers[..2], [SPECIMEN, KIND]);
        // Of every kind, so the kind is worth a column.
        let hidden = hidden(&layout.plan);
        assert!(!hidden.iter().any(|name| name == KIND));
        assert!(hidden.len() >= 70, "most of its 98 features start hidden");
        // Sorted as it opens on the portal, both keys descending, by heading.
        assert_eq!(layout.sort.len(), 2);
        assert!(layout.sort.iter().all(|key| key.descending));
        assert!(layout.sort.iter().all(|key| headers.contains(&key.column)));
        // And asked of the platform by the fields it wrote.
        assert_eq!(
            layout.plan.sort(&layout.sort),
            json!([
                { "field": "4D4RGMXTPBL0YAY5UGJ", "order": "DESC" },
                { "field": "DW0F0S320VR4NX0DBPT", "order": "DESC" },
            ])
        );
    }

    #[test]
    fn a_default_sort_that_cannot_be_read_sorts_by_nothing() {
        assert!(sort_fields(Some("not json")).is_empty());
        assert!(sort_fields(Some("[]")).is_empty());
        assert!(sort_fields(None).is_empty());
        assert_eq!(
            sort_fields(Some(
                r#"[{"field": "A", "order": "ASC"}, {"field": "B", "order": "desc"}]"#
            )),
            [("A".to_string(), false), ("B".to_string(), true)]
        );
    }

    #[test]
    fn each_kind_is_offered_in_the_platforms_order_with_its_count() {
        let kinds = kinds();
        let named: Vec<(&str, Option<u64>)> = kinds
            .iter()
            .map(|(kind, _)| (kind.name.as_str(), kind.count))
            .collect();
        assert_eq!(
            named,
            [("Library Aliquot", Some(3204)), ("Donor", Some(457))]
        );
    }

    #[test]
    fn a_kind_is_shown_with_its_own_columns_in_the_platforms_order() {
        let kinds = kinds();
        let donor = &kinds[1].1;
        assert_eq!(
            donor.headers()[..5],
            [
                SPECIMEN,
                KIND,
                "Subject NHash ID",
                "Species",
                "Species NCBI Taxon"
            ]
        );
        // A measurement sits where the platform put it among the annotations,
        // not after all of them.
        let aliquot = kinds[0].1.headers();
        let cells = aliquot
            .iter()
            .position(|it| it == "Number of Expected Cells")
            .unwrap();
        assert!(cells < aliquot.len() - 1);
        // And is narrowed by as a span, as the portal narrows by it, across
        // the extent the platform gives rather than one guessed from a page.
        let cells = kinds[0]
            .1
            .columns()
            .into_iter()
            .find(|it| it.title == "Number of Expected Cells")
            .unwrap();
        assert!(cells.measured && cells.spanned == Some(true));
        assert_eq!(cells.extent, Some((384.0, 40000.0)));
        assert_eq!(kinds[0].1.extent(&cells.id), Some((384.0, 40000.0)));
    }

    #[test]
    fn a_kind_opens_on_the_columns_the_platform_shows_it_with() {
        let kinds = kinds();
        let aliquot = &kinds[0].1;
        let hidden: Vec<String> = aliquot
            .headers()
            .into_iter()
            .zip(aliquot.hidden())
            .filter(|(_, hidden)| *hidden)
            .map(|(name, _)| name)
            .collect();
        // Every row of one kind says the same kind.
        assert!(hidden.iter().any(|name| name == KIND));
        assert!(
            hidden
                .iter()
                .any(|name| name == "Barcoded Cell Sample NHash ID")
        );
        assert!(!hidden.iter().any(|name| name == "Library Technique"));
        assert!(columns_of(aliquot).iter().any(|it| it.hidden_by_default));
        // 11 of the aliquots' 21 features are left out, and the kind.
        assert_eq!(hidden.len(), 12);
    }

    #[test]
    fn two_features_sharing_a_title_are_two_columns() {
        // A BICAN donor's age at death is recorded as a phrase and as a
        // number, and the platform calls both "Age of Death" on the record.
        let donor = &kinds()[1].1;
        let page = r#"{"data":{"aio_specimen":[{
            "cRID":{"symbol":"DO-1"},"specimenType":{"name":"Donor"},
            "annotations":[
              {"featureType":{"referenceId":"P6DZGN8YWMEZGS7CB61","title":"Age of Death"},
               "taxons":[{"symbol":"68 years"}]},
              {"featureType":{"referenceId":"1G6JRA4GFDAOF1U9H03","title":"Age of Death"},
               "taxons":[{"symbol":"68"}]}],
            "measurements":[]}]}}"#;
        let value: Value = serde_json::from_str(page).unwrap();
        let data: Data = serde_json::from_value(value["data"].clone()).unwrap();
        let headers = donor.headers();
        let row = &donor.rows(&data.aio_specimen)[0];
        let at = |name: &str| headers.iter().position(|it| it == name).unwrap();
        assert_eq!(row[at("Age of Death")], "68 years");
        assert_eq!(row[at("Age of Death Value")], "68");
    }

    #[test]
    fn a_feature_the_layout_left_out_goes_on_the_end() {
        let mut plan = kinds().swap_remove(1).1;
        let before = plan.headers();
        let page = r#"{"data":{"aio_specimen":[{
            "cRID":{"symbol":"DO-1"},"specimenType":{"name":"Donor"},
            "annotations":[{"featureType":{"referenceId":"NEW","title":"A new feature"},
               "taxons":[{"symbol":"x"}]}],
            "measurements":[]}]}}"#;
        let value: Value = serde_json::from_str(page).unwrap();
        let data: Data = serde_json::from_value(value["data"].clone()).unwrap();
        assert!(plan.extend(&data.aio_specimen));
        let after = plan.headers();
        assert_eq!(after[..before.len()], before[..]);
        assert_eq!(after.last().unwrap(), "A new feature");
    }

    #[test]
    fn choosing_another_kind_puts_its_table_in_place_of_the_last() {
        let kinds = kinds();
        let (aliquot, donor) = (kinds[0].0.clone(), kinds[1].0.clone());
        let scope = Scope {
            project: "P".into(),
            kind: Some(aliquot.id.clone()),
        };
        let layouts = kinds
            .iter()
            .map(|(kind, plan)| {
                let layout = Layout {
                    plan: plan.clone(),
                    ..default()
                };
                (kind.id.clone(), layout)
            })
            .collect();
        let mut pages =
            SpecimenPages::new("E".into(), scope, kinds[0].1.clone(), Vec::new(), layouts);
        pages.offered = true;

        let mut world = World::new();
        let mut sort = TableSort::default();
        sort.press("Library Technique", false);
        let source = world
            .spawn((
                pages,
                TablePartitions {
                    label: KINDS_LABEL.into(),
                    partitions: vec![aliquot.clone(), donor.clone()],
                    chosen: aliquot.id.clone(),
                },
                SourceTable {
                    columns: columns_of(&kinds[0].1),
                    rows: vec![vec!["LA-1".into()]],
                    first: 300,
                },
                TablePaging {
                    page: 3,
                    ..TablePaging::new(100, aliquot.count.map(|it| it as usize))
                },
                SourceStatus(String::new()),
                TableFilters::ready(vec![TableFilter::range("X", "Number of Expected Cells")]),
                sort,
            ))
            .id();

        // Nothing changes while the kind on screen is the one chosen.
        world.run_system_once(serve_kinds).unwrap();
        assert_eq!(world.get::<TablePaging>(source).unwrap().page, 3);

        world
            .get_mut::<TablePartitions>(source)
            .unwrap()
            .choose(&donor.id);
        world.run_system_once(serve_kinds).unwrap();

        let pages = world.get::<SpecimenPages>(source).unwrap();
        assert_eq!(pages.scope.kind.as_deref(), Some(donor.id.as_str()));
        assert!(pages.reread && !pages.offered);
        let table = world.get::<SourceTable>(source).unwrap();
        assert_eq!(table.columns.len(), kinds[1].1.headers().len());
        assert!(table.rows.is_empty());
        let paging = world.get::<TablePaging>(source).unwrap();
        assert_eq!(paging.page, 0);
        assert_eq!(paging.total, donor.count.map(|it| it as usize));
        assert!(world.get::<TableFilters>(source).unwrap().pending);
        assert!(world.get::<TableSort>(source).unwrap().0.is_empty());
        assert!(
            world
                .get::<SourceStatus>(source)
                .unwrap()
                .0
                .starts_with("457 rows")
        );
    }

    #[test]
    fn a_project_the_platform_does_not_lay_out_has_no_layout() {
        let data: LayoutData =
            serde_json::from_value(json!({ "kinds": [], "whole": null, "counts": null })).unwrap();
        let layouts = layouts_of(data);
        assert!(layouts.kinds.is_empty() && layouts.whole.is_none());
    }

    #[test]
    fn a_kind_narrows_every_ask_to_its_own_specimens() {
        let scope = Scope {
            project: "P".into(),
            kind: Some("K".into()),
        };
        let filters = specimen_filters(&scope, &[]);
        assert_eq!(
            filters,
            json!([
                { "field": "projectReferenceIds", "operator": "EQ", "value": "P" },
                { "field": KIND_ID_FIELD, "operator": "EQ", "value": "K" },
            ])
        );
    }

    #[test]
    #[ignore = "reads the live BICAN rapid release inventory"]
    fn a_project_shown_by_kind_reads_filters_and_counts_each_kind_alone() {
        const PROJECT: &str = "BUQ7G50XHDCFCJCQ03A";
        let endpoint = "https://idf-api-prod.aibs-idk-prod.net/";
        crate::app::net::block_on(async {
            let specimens = read(endpoint, PROJECT).await.unwrap();
            assert_eq!(specimens.kinds.len(), 2);
            let (first, _) = &specimens.kinds[0];
            assert_eq!(specimens.scope.kind.as_deref(), Some(first.id.as_str()));
            assert_eq!(specimens.table.total, first.count.map(|it| it as usize));
            // Every row is of the kind on screen.
            let kind = specimens
                .table
                .rows
                .columns
                .iter()
                .position(|it| it.name == KIND);
            assert!(
                specimens
                    .table
                    .rows
                    .rows
                    .iter()
                    .all(|row| { row[kind.unwrap()] == first.name })
            );

            // The other kind, as a switch would ask for it.
            let (donor, layout) = &specimens.kinds[1];
            let plan = &layout.plan;
            let scope = Scope {
                project: PROJECT.into(),
                kind: Some(donor.id.clone()),
            };
            let page = ask(endpoint, &scope, 0, &[], Value::Null, None)
                .await
                .unwrap();
            assert_eq!(page.counted(), donor.count.map(|it| it as usize));
            let rows = plan.rows(&page.aio_specimen);
            let headers = plan.headers();
            let filled = |name: &str| {
                let at = headers.iter().position(|it| it == name).unwrap();
                rows.iter().filter(|row| !row[at].is_empty()).count()
            };
            assert!(filled("Species") > 0);
            assert!(filled("Age of Death Value") > 0);

            // A filter opened on it is counted among donors alone.
            let species = plan
                .columns()
                .into_iter()
                .find(|it| it.title == "Species")
                .unwrap();
            let read = ask_values(endpoint, &scope, &[species.id], &[])
                .await
                .unwrap();
            let values = read[0].as_ref().expect("species can be grouped");
            let counted: u64 = values.iter().map(|(_, count)| count).sum();
            assert!(counted <= donor.count.unwrap());
            assert!(counted > 0);
        });
    }

    #[test]
    #[ignore = "reads the live Genetic Tools Atlas specimens"]
    fn a_project_shown_whole_opens_sorted_on_the_columns_the_platform_shows() {
        const PROJECT: &str = "7CVKSF7QGAKIQ8LM5LC";
        let endpoint = "https://idf-api-prod.aibs-idk-prod.net/";
        crate::app::net::block_on(async {
            let specimens = read(endpoint, PROJECT).await.unwrap();
            assert!(specimens.kinds.is_empty());
            assert_eq!(specimens.sort.len(), 1, "the portal opens it sorted");
            let columns = &specimens.table.rows.columns;
            let hidden = columns.iter().filter(|it| it.hidden_by_default).count();
            assert!(hidden >= 10, "{hidden} of {} hidden", columns.len());
            // The page came back in that order.
            let at = columns
                .iter()
                .position(|it| it.name == specimens.sort[0].column)
                .unwrap();
            let values: Vec<&str> = specimens
                .table
                .rows
                .rows
                .iter()
                .map(|row| row[at].as_str())
                .filter(|it| !it.is_empty())
                .collect();
            let mut sorted = values.clone();
            sorted.sort_by_key(|it| it.to_lowercase());
            assert_eq!(values, sorted);
        });
    }
}
