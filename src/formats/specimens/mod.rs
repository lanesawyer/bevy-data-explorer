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

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::graphql::{self, Response};
use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::source::SourceBusy;
use crate::source::properties::NumericRange;
use crate::source::table::{
    SortKey, SourceTable, TableColumn, TableFilter, TableFilterKind, TableFilterTerm,
    TableFilterValue, TableFilters, TablePaging, TableSort,
};

use super::table::{PAGE_ROWS, Table};

mod filters;
mod pages;
mod plan;
mod query;
mod recount;
mod spans;

use filters::*;
use pages::*;
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
/// One request: the project's title, the count, and the page. A frame opens on
/// what comes back, and [`SpecimenSystems`] fetches any other page the frame
/// is turned to.
pub async fn read(endpoint: &str, project: &str) -> Result<Specimens, String> {
    let answer = ask(endpoint, project, 0, &[], Value::Null).await?;
    if answer.aio_specimen.is_empty() {
        return Err(format!(
            "{endpoint} knows no specimens in project {project}"
        ));
    }

    let title = answer.project.first().and_then(|it| it.title.clone());
    let total = answer.counted().unwrap_or(answer.aio_specimen.len());
    let plan = Plan::of(&answer.aio_specimen);
    let table = Table::paged(
        label_for(project, title.as_deref()),
        format!(
            "Brain Knowledge Platform specimens, {} columns",
            plan.headers().len()
        ),
        plan.headers(),
        plan.rows(&answer.aio_specimen),
        None,
        Some(total),
    );
    Ok(Specimens {
        endpoint: endpoint.to_string(),
        project: project.to_string(),
        table,
        plan,
    })
}

/// A project's specimens: the page that was read, and what it takes to read
/// another.
pub struct Specimens {
    endpoint: String,
    project: String,
    pub table: Table,
    plan: Plan,
}

/// The systems every specimen table shares, registered once however many are
/// open.
pub struct SpecimenSystems;

impl Plugin for SpecimenSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (offer_filters, serve_spans, serve_counts, serve_pages)
                .chain()
                .in_set(Stage::Sources),
        );
    }
}

/// Register a project's specimens as a source that can be paged.
pub fn spawn_source(world: &mut World, specimens: Specimens) -> Entity {
    let Specimens {
        endpoint,
        project,
        table,
        plan,
    } = specimens;
    let source = super::table::spawn_source(world, table);
    world
        .entity_mut(source)
        .insert(SpecimenPages::new(endpoint, project, plan));
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
