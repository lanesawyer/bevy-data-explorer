//! Records from the BKP Registry — its specimens, processes and data assets —
//! each kind a table, as the Pre-Public Data Catalog lists them.
//!
//! Not a file format, any more than [`super::specimens`] is: a question asked
//! of the registry's GraphQL API, which sends nothing without a token. The
//! address is the endpoint with the kind on it, named as the registry's own
//! query for it is:
//!
//! ```text
//! https://stage-bkpr.brain.devlims.org/graphql/?records=dataAssets
//! ```
//!
//! There are far too many to read whole — 30 thousand specimens, 2 million
//! processes and 17.8 million data assets on 2026-09-27 — so a page is read
//! at a time, ordered and narrowed at the registry. A picked record's links
//! are read when it is picked: the processes a specimen or data asset went
//! into and came out of, and what a process took in and put out. A search
//! matches names, at the registry, so it finds a record on any page.

use std::collections::HashMap;

use bevy::prelude::*;
use serde_json::Value;

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::source::SourceBusy;
use crate::source::table::{
    RelatedRecords, SelectedRecord, SortKey, SourceTable, TableFilter, TableFilterKind,
    TableFilterValue, TableFilters, TablePaging, TableSearch, TableSort,
};

use super::table::{MAX_CHARS, PAGE_ROWS, Table};

mod filters;
mod kinds;
mod pages;
mod query;
mod related;

pub use kinds::Kind;

use filters::serve_values;
use kinds::{Asked, CREATED, ID};
use pages::serve_pages;
use query::{Landed, ask_page};
use related::serve_related;

/// The pre-production registry, which the live tests read. The endpoint
/// otherwise comes off the address, as whoever listed it wrote it.
#[cfg(test)]
const STAGE: &str = "https://stage-bkpr.brain.devlims.org/graphql/";

/// The query parameter naming the kind of record.
const PARAMETER: &str = "records";

/// The address a kind of record is read from.
pub fn address(endpoint: &str, kind: Kind) -> String {
    format!("{endpoint}?{PARAMETER}={}", kind.root())
}

/// Whether `url` asks for a kind of the registry's records, and if so the
/// endpoint to ask and the kind.
pub fn query_of(url: &str) -> Option<(String, Kind)> {
    let (endpoint, query) = url.split_once('?')?;
    let kind = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == PARAMETER)
        .and_then(|(_, value)| Kind::from_root(value))?;
    Some((endpoint.to_string(), kind))
}

/// How a table opens: newest first.
fn opening_sort() -> Vec<SortKey> {
    vec![SortKey {
        column: CREATED.to_string(),
        descending: true,
    }]
}

/// Read the first page of a kind of record, and how many there are.
pub async fn read(endpoint: &str, kind: Kind) -> Result<Records, String> {
    let sort = opening_sort();
    let landed = ask_page(endpoint, kind, 0, PAGE_ROWS, &Asked::default(), &sort).await?;
    let mut table = Table::paged(
        format!("{} (BKP Registry)", kind.title()),
        format!("BKP Registry {}", kind.title().to_lowercase()),
        kind.headers(),
        kind.rows(&landed.nodes),
        None,
        Some(landed.total),
    );
    for (column, hidden) in table.rows.columns.iter_mut().zip(kind.hidden()) {
        column.hidden_by_default = hidden;
    }
    Ok(Records {
        endpoint: endpoint.to_string(),
        kind,
        table,
        sort,
    })
}

/// A kind of record: the page that was read, and what it takes to read
/// another.
pub struct Records {
    endpoint: String,
    kind: Kind,
    pub table: Table,
    /// The order the page was read in, which the table opens sorted by.
    sort: Vec<SortKey>,
}

/// Where a table of records came from, and what is being asked of it.
///
/// A component of the source entity, so two frames on two kinds each turn
/// their own pages.
#[derive(Component)]
pub struct RecordPages {
    endpoint: String,
    kind: Kind,
    /// The page being fetched, and the fetch, while one is in flight.
    fetching: Option<(usize, Fetching<Result<Landed, String>>)>,
    /// The values and search in force on the rows now on screen, so a tick
    /// that changes nothing does not refetch and one that does is noticed.
    applied: Asked,
    /// The order the rows now on screen were asked for in, likewise.
    sorted: Vec<SortKey>,
    /// Filters being read, what they are read under, and the read.
    reading: Option<ValuesRead>,
    /// What each filter's counts were counted under, by path.
    counted: HashMap<String, Asked>,
    /// The record whose links are shown or being fetched, and the fetch.
    picked: Option<String>,
    linking: Option<Fetching<Result<Value, String>>>,
}

type ValuesRead = (
    Vec<String>,
    Asked,
    Fetching<Result<Vec<(String, Vec<(String, u64)>)>, String>>,
);

/// The systems every table of records shares, registered once however many
/// are open.
pub struct RecordSystems;

impl Plugin for RecordSystems {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (serve_values, serve_pages, serve_related)
                .chain()
                .in_set(Stage::Sources),
        );
    }
}

/// The filters a kind is narrowed by, none of them read: what each offers
/// is asked when it is opened.
fn filters_of(kind: Kind) -> Vec<TableFilter> {
    kind.facets()
        .iter()
        .map(|facet| TableFilter::unread_values(facet.path, facet.name))
        .collect()
}

/// Register a kind of record as a source that can be paged, sorted and
/// narrowed.
pub fn spawn_source(world: &mut World, records: Records) -> Entity {
    let Records {
        endpoint,
        kind,
        table,
        sort,
    } = records;
    let source = super::table::spawn_source(world, table);
    world.entity_mut(source).insert((
        TableSort(sort.clone()),
        TableFilters::ready(filters_of(kind)),
        TableSearch::new("Names holding the text, in any case."),
        RelatedRecords::default(),
        RecordPages {
            endpoint,
            kind,
            fetching: None,
            applied: Asked::default(),
            sorted: sort,
            reading: None,
            counted: HashMap::new(),
            picked: None,
            linking: None,
        },
    ));
    source
}

/// What is asked of a table now: the values ticked and the text searched
/// for.
fn asked_of(filters: &TableFilters, search: &TableSearch) -> Asked {
    Asked {
        terms: filters.chosen(),
        search: search.wanted().map(str::to_string),
    }
}

/// The id of the record picked out of a table, once it has been read.
fn id_of(record: &SelectedRecord) -> Option<String> {
    record
        .0
        .as_ref()?
        .fields
        .as_ref()?
        .iter()
        .find(|(name, _)| name == ID)
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_names_the_endpoint_and_the_kind() {
        for kind in Kind::ALL {
            assert_eq!(
                query_of(&address(STAGE, kind)),
                Some((STAGE.to_string(), kind))
            );
        }
        assert_eq!(
            address(STAGE, Kind::DataAssets),
            "https://stage-bkpr.brain.devlims.org/graphql/?records=dataAssets"
        );
    }

    #[test]
    fn anything_else_is_not_a_kind_of_record() {
        assert!(query_of("https://example.com/a.csv").is_none());
        assert!(query_of("https://example.com/?records=cells").is_none());
        assert!(query_of("https://example.com/?specimens=ABC").is_none());
    }

    #[test]
    fn every_filter_is_offered_unread() {
        for kind in Kind::ALL {
            let filters = filters_of(kind);
            assert!(!filters.is_empty());
            assert!(filters.iter().all(|it| !it.read() && !it.wanted));
        }
    }
}
