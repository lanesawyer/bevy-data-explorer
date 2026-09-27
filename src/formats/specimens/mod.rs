//! Specimen records from the Brain Knowledge Platform, as a table.
//!
//! Not a file format: a question asked of the platform's GraphQL API. It is
//! here rather than in `catalog` because a catalog only says what there is to
//! open, and this *is* a dataset — one a frame shows exactly the way it shows
//! a CSV, since both end at rows and columns.
//!
//! The address is the API endpoint itself with the project named on it:
//!
//! ```text
//! https://idf-api-prod.aibs-idk-prod.net/?specimens=JGN327NUXRZSHEV88TN
//! ```
//!
//! So the thing being talked to is the thing being named, and a different
//! deployment of the API is reached by writing its host instead.
//!
//! A specimen is not a row. It carries a list of annotations and a list of
//! measurements, each tagged with the feature it belongs to, and which
//! features a specimen has varies within one project. So the columns are the
//! union of the features seen across the page, and a specimen with nothing
//! under a feature leaves that cell empty — which is the honest answer, and
//! what a spreadsheet of the same records would show.
//!
//! A search is the platform's own: every word of every specimen, matched as
//! its search box matches them, rather than the page in hand.

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::graphql::{self, Response};
use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::source::properties::NumericRange;
use crate::source::table::{
    RecordFile, RecordFiles, RecordImage, RecordImages, RecordImagesState, SelectedRecord, SortKey,
    SourceTable, TableColumn, TableFilter, TableFilterKind, TableFilterTerm, TableFilterValue,
    TableFilters, TablePaging, TablePartition, TablePartitions, TableSearch, TableSort,
};
use crate::source::{SourceBusy, SourceStatus};

use super::table::{PAGE_ROWS, Table};

mod filters;
mod layout;
mod pages;
mod picked;
mod plan;
mod query;
mod recount;
mod spans;

use filters::*;
use layout::*;
use pages::*;
use picked::*;
use plan::*;
use query::*;
use recount::*;
use spans::*;

/// Specimens asked for in one request: the page the frame is showing and no
/// more.
///
/// Measured against the SEA-AD donor project, where 84 specimens of 30
/// features came to 199KB in 0.7s, and the Genetic Tools Atlas, where 500 came
/// to 1.5MB in 1.1s. Reading all 10,901 of the latter took 34MB and some
/// twenty seconds before a frame appeared, which is the whole reason a page is
/// what gets fetched.
const PAGE: usize = PAGE_ROWS;

/// The query parameter that names a project's specimens.
const PARAMETER: &str = "specimens";

/// The column holding a specimen's own accession.
const SPECIMEN: &str = "Specimen";

/// The column holding what kind of specimen it is.
const KIND: &str = "Type";

/// What the platform sorts the [`SPECIMEN`] and [`KIND`] columns by.
const SPECIMEN_FIELD: &str = "cRID.symbol";
const KIND_FIELD: &str = "specimenType.name";

/// What the platform narrows specimens to one kind by.
const KIND_ID_FIELD: &str = "specimenType.referenceId";

/// What a project's kinds of specimen are headed, over the choice of them.
const KINDS_LABEL: &str = "Specimen type";

/// Whether `url` asks for a project's specimens, and if so the endpoint to ask
/// and the project to ask about.
///
/// The endpoint is the address with the query dropped: what names the dataset
/// is also what answers for it.
pub fn query_of(url: &str) -> Option<(String, String)> {
    let (endpoint, query) = url.split_once('?')?;
    let project = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == PARAMETER)
        .map(|(_, value)| value)?;
    if project.is_empty() {
        return None;
    }
    Some((endpoint.to_string(), project.to_string()))
}

/// Name a table after the project, falling back to what it was asked for.
fn label_for(project: &str, title: Option<&str>) -> String {
    match title {
        Some(title) if !title.trim().is_empty() => title.trim().to_string(),
        _ => format!("Specimens {project}"),
    }
}

/// Read the first page of a project's specimens, and find out how many there
/// are altogether.
///
/// One request for the project's title, the count and the page, and beside
/// it one for how the platform lays the project out. A project it lays out no
/// differently from the page opens on the first. One that shows its kinds
/// apart is read again for its first kind alone, and one that opens sorted is
/// read again in that order, which costs those a round trip and every other
/// project nothing. A frame opens on what comes back, and [`SpecimenSystems`]
/// fetches any other page, or kind, the frame is turned to.
pub async fn read(endpoint: &str, project: &str) -> Result<Specimens, String> {
    let mut scope = Scope::project(project);
    let (answer, layouts) = futures::join!(
        ask(endpoint, &scope, 0, &[], Value::Null, None),
        ask_layout(endpoint, project)
    );
    let mut answer = answer?;
    // Without its layout a project is still a table, of every feature its
    // first page carries, in an order of the viewer's own.
    let layouts = layouts
        .map(layouts_of)
        .inspect_err(|e| warn!("asking how {project} lays out its specimens: {e}"))
        .unwrap_or_default();

    let layout = match layouts.kinds.first() {
        Some((kind, layout)) => {
            scope.kind = Some(kind.id.clone());
            layout.clone()
        }
        None => layouts.whole.clone().unwrap_or_default(),
    };
    let Layout {
        mut plan,
        sort,
        images,
    } = layout;
    if scope.kind.is_some() || !sort.is_empty() {
        let first = ask(endpoint, &scope, 0, &[], plan.sort(&sort), None).await?;
        answer.aio_specimen = first.aio_specimen;
        answer.total = first.total;
    }
    if answer.aio_specimen.is_empty() {
        return Err(format!(
            "{endpoint} knows no specimens in project {project}"
        ));
    }
    plan.extend(&answer.aio_specimen);

    let title = answer.project.first().and_then(|it| it.title.clone());
    let total = answer.counted().unwrap_or(answer.aio_specimen.len());
    let mut table = Table::paged(
        label_for(project, title.as_deref()),
        detail_of(&plan),
        plan.headers(),
        plan.rows(&answer.aio_specimen),
        None,
        Some(total),
    );
    for (column, hidden) in table.rows.columns.iter_mut().zip(plan.hidden()) {
        column.hidden_by_default = hidden;
    }
    Ok(Specimens {
        endpoint: endpoint.to_string(),
        scope,
        table,
        plan,
        sort,
        kinds: layouts.kinds,
        images,
    })
}

fn detail_of(plan: &Plan) -> String {
    format!(
        "Brain Knowledge Platform specimens, {} columns",
        plan.headers().len()
    )
}

/// A project's specimens: the page that was read, and what it takes to read
/// another.
pub struct Specimens {
    endpoint: String,
    scope: Scope,
    pub table: Table,
    plan: Plan,
    /// The order the page was read in, which the table opens sorted by.
    sort: Vec<SortKey>,
    /// The kinds of specimen the project shows apart, each with its layout;
    /// empty for one that shows them together.
    kinds: Vec<(TablePartition, Layout)>,
    /// The pictures a specimen of the kind on screen may have, by title.
    images: Vec<String>,
}

/// The systems every specimen table shares, registered once however many are
/// open.
pub struct SpecimenSystems;

impl Plugin for SpecimenSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                serve_kinds,
                offer_filters,
                serve_values,
                serve_spans,
                serve_counts,
                serve_pages,
                serve_picked,
            )
                .chain()
                .in_set(Stage::Sources),
        );
    }
}

/// Register a project's specimens as a source that can be paged.
pub fn spawn_source(world: &mut World, specimens: Specimens) -> Entity {
    let Specimens {
        endpoint,
        scope,
        table,
        plan,
        sort,
        kinds,
        images,
    } = specimens;
    let source = super::table::spawn_source(world, table);
    let mut entity = world.entity_mut(source);
    if let Some(chosen) = scope.kind.clone() {
        entity.insert(TablePartitions {
            label: KINDS_LABEL.to_string(),
            partitions: kinds.iter().map(|(kind, _)| kind.clone()).collect(),
            chosen,
        });
    }
    let layouts = kinds
        .into_iter()
        .map(|(kind, layout)| (kind.id, layout))
        .collect();
    // The page in hand was read in this order, so it is not read again.
    entity.insert((
        TableSort(sort.clone()),
        SpecimenPages::new(endpoint, scope, plan, sort, layouts),
        PickedSpecimen::new(images),
        RecordImages::default(),
        RecordFiles::default(),
        TableSearch::new(
            "Whole words in any column, as the platform searches: * ends a word \
             early, quotes keep words together, AND wants both. The filters' \
             counts leave the search out.",
        ),
    ));
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_names_the_endpoint_that_answers_for_it() {
        let (endpoint, project) =
            query_of("https://idf-api-prod.aibs-idk-prod.net/?specimens=JGN327NUXRZSHEV88TN")
                .unwrap();
        assert_eq!(endpoint, "https://idf-api-prod.aibs-idk-prod.net/");
        assert_eq!(project, "JGN327NUXRZSHEV88TN");
        // Whatever else is on the query string, and whatever the host.
        assert_eq!(
            query_of("https://dev.example.net/?v=2&specimens=ABC").unwrap(),
            ("https://dev.example.net/".into(), "ABC".into())
        );
    }

    #[test]
    fn anything_else_is_not_a_specimen_query() {
        assert!(query_of("https://example.com/image.zarr/").is_none());
        assert!(query_of("https://example.com/a.csv?x=1").is_none());
        // Named with nothing to name: not a project.
        assert!(query_of("https://example.com/?specimens=").is_none());
        // A parameter that merely ends the same way.
        assert!(query_of("https://example.com/?nospecimens=ABC").is_none());
    }

    #[test]
    fn a_project_with_no_title_is_named_after_what_was_asked_for() {
        assert_eq!(label_for("ABC", Some("SEA-AD donors")), "SEA-AD donors");
        assert_eq!(label_for("ABC", None), "Specimens ABC");
        assert_eq!(label_for("ABC", Some("  ")), "Specimens ABC");
    }
}
