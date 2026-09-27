//! A project that shows its kinds of specimen apart: each kind a table of its
//! own, with the columns the platform shows it with, and the one on screen
//! swapped for another when the frame asks.

use super::*;

/// Each kind a project shows apart, in the platform's order, with how many
/// specimens it holds and the columns it is shown with.
///
/// A display feature that is neither an annotation nor a measurement is left
/// out, since a row is read from those two alone.
pub(super) fn kinds_of(data: KindsData) -> Vec<(TablePartition, Plan)> {
    let counts: HashMap<String, u64> = labeled(&data.counts).into_iter().collect();
    let mut kinds = data.kinds;
    kinds.sort_by_key(|kind| kind.priority_order.unwrap_or(i64::MAX));
    kinds
        .into_iter()
        .map(|kind| {
            let mut features = kind.display_features;
            features.sort_by_key(|feature| feature.priority_order.unwrap_or(i64::MAX));
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
                        })
                    })
                    .collect(),
            );
            let partition = TablePartition {
                name: kind
                    .title
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| kind.reference_id.clone()),
                count: counts.get(&kind.reference_id).copied(),
                id: kind.reference_id,
            };
            (partition, plan)
        })
        .collect()
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
/// filters and another sort, and its own count. Everything the last kind had
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
        let Some(plan) = pages.layout(&partitions.chosen) else {
            continue;
        };
        let kind = partitions.chosen();
        info!(
            "showing {} specimens",
            kind.map_or(partitions.chosen.as_str(), |kind| kind.name.as_str())
        );
        pages.show_kind(partitions.chosen.clone(), plan);

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
            sort.set_if_neq(TableSort::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn layout() -> KindsData {
        let text = include_str!("../../../testdata/specimen_kinds.json");
        let value: Value = serde_json::from_str(text).unwrap();
        serde_json::from_value(value["data"].clone()).unwrap()
    }

    #[test]
    fn each_kind_is_offered_in_the_platforms_order_with_its_count() {
        let kinds = kinds_of(layout());
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
        let kinds = kinds_of(layout());
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
        assert!(
            kinds[0]
                .1
                .columns()
                .iter()
                .any(|(_, title, measured)| { title == "Number of Expected Cells" && *measured })
        );
    }

    #[test]
    fn a_kind_opens_on_the_columns_the_platform_shows_it_with() {
        let kinds = kinds_of(layout());
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
        let donor = &kinds_of(layout())[1].1;
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
        let mut plan = kinds_of(layout()).swap_remove(1).1;
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
        let kinds = kinds_of(layout());
        let (aliquot, donor) = (kinds[0].0.clone(), kinds[1].0.clone());
        let scope = Scope {
            project: "P".into(),
            kind: Some(aliquot.id.clone()),
        };
        let layouts = kinds
            .iter()
            .map(|(kind, plan)| (kind.id.clone(), plan.clone()))
            .collect();
        let mut pages = SpecimenPages::new("E".into(), scope, kinds[0].1.clone(), layouts);
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
    fn a_project_that_shows_no_kinds_apart_has_none() {
        let data: KindsData =
            serde_json::from_value(json!({ "kinds": [], "counts": null })).unwrap();
        assert!(kinds_of(data).is_empty());
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
            let (donor, plan) = &specimens.kinds[1];
            let scope = Scope {
                project: PROJECT.into(),
                kind: Some(donor.id.clone()),
            };
            let page = ask(endpoint, &scope, 0, &[], Value::Null).await.unwrap();
            assert_eq!(page.counted(), donor.count.map(|it| it as usize));
            let rows = plan.rows(&page.aio_specimen);
            let headers = plan.headers();
            let filled = |name: &str| {
                let at = headers.iter().position(|it| it == name).unwrap();
                rows.iter().filter(|row| !row[at].is_empty()).count()
            };
            assert!(filled("Species") > 0);
            assert!(filled("Age of Death Value") > 0);

            // Its filters are counted among donors alone.
            let filters = ask_values(endpoint, &scope, &plan.columns()).await.unwrap();
            let species = filters.iter().find(|it| it.name == "Species").unwrap();
            let counted: u64 = species.listed().iter().map(|it| it.count).sum();
            assert!(counted <= donor.count.unwrap());
            assert!(counted > 0);
        });
    }
}
