//! Lists of datasets the app can offer without being told their address.
//!
//! A catalog answers one question — what is there to open — and every dataset
//! it names is opened the way a typed URL is, through
//! [`crate::formats::discover`]. So a catalog knows nothing about formats, and
//! a format nothing about catalogs: adding either touches only its own module
//! and the line in `main` that registers it.
//!
//! Catalogs are listed asynchronously. The built-in examples answer at once;
//! one read over the network lands whenever it does. Each keeps its own slot,
//! in registration order, so a slow catalog never reorders one listed before
//! it, and a failed one costs only its own entries.

pub mod bkp;
pub mod examples;

use bevy::prelude::*;
use futures::future::BoxFuture;

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;

/// One dataset a catalog offers.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    /// What kind of dataset it is, in the words shown beside it.
    pub kind: String,
    pub url: String,
    /// Further words to find it by, such as the full title a short name
    /// abbreviates.
    pub keywords: String,
}

/// Somewhere datasets are listed.
pub trait Catalog: Send + Sync + 'static {
    /// What to call it, in search and in the log.
    fn name(&self) -> &str;

    /// Everything it offers. Run once, off the main thread.
    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>>;
}

/// Where an entry sits: its catalog's place, and its place in that catalog.
///
/// Both positions stay put as other catalogs land, which is what lets a menu
/// item hold one across frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EntryId {
    catalog: usize,
    entry: usize,
}

struct Slot {
    name: String,
    entries: Vec<Entry>,
    listing: Option<Fetching<Result<Vec<Entry>, String>>>,
}

/// Every registered catalog, and what each has listed so far.
#[derive(Resource, Default)]
pub struct Catalogs {
    slots: Vec<Slot>,
}

impl Catalogs {
    /// Register `catalog` and start listing it.
    pub fn add(&mut self, catalog: impl Catalog) {
        self.slots.push(Slot {
            name: catalog.name().to_string(),
            entries: Vec::new(),
            listing: Some(fetching(catalog.list())),
        });
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.slots.get(id.catalog)?.entries.get(id.entry)
    }

    /// How many entries have landed, across every catalog. Only ever grows, so
    /// a list rebuilt from these can tell from this alone that it is stale.
    pub fn len(&self) -> usize {
        self.slots.iter().map(|slot| slot.entries.len()).sum()
    }

    /// Every entry not open yet, with the name of the catalog it came from.
    ///
    /// `opened` is every address a source was read from. A dataset already
    /// open is offered as the source it is instead, so listing it here too
    /// would offer it twice.
    pub fn unopened<'a>(
        &'a self,
        opened: &'a [&'a str],
    ) -> impl Iterator<Item = (EntryId, &'a str, &'a Entry)> + 'a {
        self.slots
            .iter()
            .enumerate()
            .flat_map(|(catalog, slot)| {
                slot.entries.iter().enumerate().map(move |(entry, found)| {
                    (EntryId { catalog, entry }, slot.name.as_str(), found)
                })
            })
            .filter(|(_, _, entry)| !opened.contains(&entry.url.as_str()))
    }

    fn take_listings(&mut self) {
        for slot in &mut self.slots {
            let Some(result) = slot.listing.as_mut().and_then(Fetching::take) else {
                continue;
            };
            slot.listing = None;
            match result {
                Ok(entries) => {
                    info!("{}: {} datasets", slot.name, entries.len());
                    slot.entries = entries;
                }
                Err(e) => warn!("{}: could not list datasets: {e}", slot.name),
            }
        }
    }
}

/// Registering a catalog on the app, for `main` and any plugin that brings one.
pub trait AppCatalogs {
    fn add_catalog(&mut self, catalog: impl Catalog) -> &mut Self;
}

impl AppCatalogs for App {
    fn add_catalog(&mut self, catalog: impl Catalog) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<Catalogs>()
            .add(catalog);
        self
    }
}

/// Takes in catalogs as they finish listing.
pub struct CatalogPlugin;

impl Plugin for CatalogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Catalogs>()
            .add_systems(Update, take_listings.in_set(Stage::Catalogs));
    }
}

fn take_listings(mut catalogs: ResMut<Catalogs>) {
    if catalogs.slots.iter().any(|slot| slot.listing.is_some()) {
        catalogs.take_listings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    struct Fixed(&'static str, Vec<&'static str>);

    impl Catalog for Fixed {
        fn name(&self) -> &str {
            self.0
        }

        fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
            let entries = self
                .1
                .iter()
                .map(|url| Entry {
                    name: url.to_string(),
                    kind: String::new(),
                    url: url.to_string(),
                    keywords: String::new(),
                })
                .collect();
            Box::pin(async move { Ok(entries) })
        }
    }

    fn listed(catalogs: &mut Catalogs) {
        let started = Instant::now();
        while catalogs.slots.iter().any(|slot| slot.listing.is_some()) {
            assert!(started.elapsed() < Duration::from_secs(2));
            catalogs.take_listings();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn entries_keep_their_catalog_order_and_skip_what_is_open() {
        let mut catalogs = Catalogs::default();
        catalogs.add(Fixed("first", vec!["a", "b"]));
        catalogs.add(Fixed("second", vec!["c"]));
        listed(&mut catalogs);

        let urls: Vec<&str> = catalogs
            .unopened(&["b"])
            .map(|(_, _, entry)| entry.url.as_str())
            .collect();
        assert_eq!(urls, ["a", "c"]);
        assert_eq!(catalogs.len(), 3);

        let (id, name, _) = catalogs.unopened(&[]).last().unwrap();
        assert_eq!(name, "second");
        assert_eq!(catalogs.get(id).unwrap().url, "c");
    }
}
