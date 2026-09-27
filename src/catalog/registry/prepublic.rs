//! The Pre-Public Data Catalog: the registry's specimens, processes and data
//! assets, each a table.
//!
//! A catalog of three entries that never change, so it lists them at once
//! and asks nothing; opening one is what asks, and it is
//! [`crate::formats::records`] that reads it, as it would the same address
//! pasted in. Beside the searched [`super::Registry`] rather than in it,
//! since that one offers single assets to open and this whole tables of
//! records, and under the same provider, so turning the registry off turns
//! both off.

use futures::future::BoxFuture;

use super::PROVIDER;
use crate::catalog::{Catalog, Entry, Provider};
use crate::formats::records::{Kind, address};
use crate::source::Category;

pub struct PrePublic {
    endpoint: String,
}

impl PrePublic {
    pub fn stage() -> Self {
        PrePublic {
            endpoint: super::STAGE.to_string(),
        }
    }
}

impl Catalog for PrePublic {
    fn name(&self) -> &str {
        "Pre-Public Data Catalog"
    }

    fn provider(&self) -> Option<Provider> {
        Some(PROVIDER)
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        let entries = entries(&self.endpoint);
        Box::pin(async move { Ok(entries) })
    }
}

/// The three tables, in the order the catalog's own pages list them.
fn entries(endpoint: &str) -> Vec<Entry> {
    Kind::ALL
        .into_iter()
        .map(|kind| entry(endpoint, kind))
        .collect()
}

/// One kind of record as a table to open.
pub(super) fn entry(endpoint: &str, kind: Kind) -> Entry {
    Entry {
        name: kind.title().to_string(),
        kind: "Pre-Public catalog".into(),
        category: Category::Table,
        url: address(endpoint, kind),
        keywords: format!("BKP Registry Pre-Public Data Catalog {}", kind.root()),
        cells: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_is_offered_as_a_table_the_format_reads() {
        let entries = entries(super::super::STAGE);
        let names: Vec<&str> = entries.iter().map(|it| it.name.as_str()).collect();
        assert_eq!(names, ["Specimens", "Processes", "Data assets"]);
        for entry in &entries {
            assert_eq!(entry.category, Category::Table);
            assert!(crate::formats::discover::names_a_table(&entry.url));
        }
    }
}
