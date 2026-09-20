//! Lists of datasets the app can offer without being told their address.
//!
//! A catalog answers one question — what is there to open — and every dataset
//! it names is opened the way a typed URL is, through
//! [`crate::formats::discover`]. So a catalog knows nothing about formats, and
//! a format nothing about catalogs: adding either touches only its own module
//! and the line in `main` that registers it.
//!
//! An entry may also name a service that knows the dataset's cells better than
//! its files do — their labels, colors and counts. Whichever way the dataset
//! was then opened, [`cells`] finds its entry by address and asks. Datasets no
//! catalog vouches for, the built-in examples among them, keep what their files
//! say.
//!
//! Catalogs are listed asynchronously. The built-in examples answer at once;
//! one read over the network lands whenever it does. Each keeps its own slot,
//! in registration order, so a slow catalog never reorders one listed before
//! it, and a failed one costs only its own entries.

pub mod bkp;
pub mod cells;
pub mod examples;
pub mod genes;

use std::sync::Arc;

use bevy::prelude::*;
use futures::future::BoxFuture;

use crate::app::net::{Fetching, fetching};
use crate::app::schedule::Stage;
use crate::source::genes::Gene;
use crate::source::properties::{CellColumns, CellProperties, CellProperty};
use crate::source::region::SelectedRegion;
use crate::source::{DataSource, SourceUrl};

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
    /// Who to ask about its cells, if anyone knows more than the files.
    pub cells: Option<CellService>,
}

/// How the cells of a dataset are distributed across its properties.
#[derive(Debug, Default)]
pub struct CellCounts {
    /// How many cells hold each value, by column id and then by code.
    pub values: Vec<(String, Vec<(u16, u64)>)>,
    /// Each numeric property's histogram, by property id, in the buckets it
    /// already has.
    pub histograms: Vec<(String, Vec<u32>)>,
}

/// What a service found inside a region of a dataset: how its cells are
/// distributed, and — once a category is drilled into — the cells themselves.
///
/// One per source rather than per frame: a source can only carry one
/// [`SelectedRegion`] at a time, so two frames selecting on one dataset share
/// the last rectangle drawn, which is also the one both of them show.
#[derive(Component, Debug, Default)]
pub struct RegionSummary {
    /// The region these counts are of, so a stale answer landing after the
    /// rectangle moved can be told apart from a current one.
    pub counted: Option<SelectedRegion>,
    pub state: SummaryState,
    /// How many cells in the region hold each value, by column id and code.
    pub values: Vec<(String, Vec<(u16, u64)>)>,
    /// The individual cells of the one category drilled into, and how that
    /// fetch is going. Fetched separately because it is asked only when a
    /// category is picked, and picking one should not re-ask the counts.
    pub cells: Vec<CellRecord>,
    pub focus_state: SummaryState,
}

/// One cell, as a service lists the cells inside a region.
///
/// Values arrive named and already as labels rather than as the codes the
/// files store, so a record can be shown without going back to the properties
/// for a translation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRecord {
    pub id: String,
    pub index: u64,
    /// Column id and what this cell holds in it.
    pub values: Vec<(String, String)>,
}

impl CellRecord {
    /// What this cell holds in `column`, if the service said.
    pub fn value(&self, column: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(id, _)| id == column)
            .map(|(_, value)| value.as_str())
    }
}

/// One value of one column, naming a category to narrow a region's counts to.
///
/// Named by label rather than by code because that is what the counting
/// queries take, and what survives being compared against an answer.
#[derive(Component, Clone, Debug, PartialEq, Eq, Default)]
pub struct RegionFocus {
    pub column: String,
    pub label: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum SummaryState {
    /// Nothing selected, or nothing that can count one.
    #[default]
    Idle,
    Counting,
    Ready,
    Failed(String),
}

impl RegionSummary {
    /// The cells in the region holding each value of `column`, largest first.
    pub fn buckets(&self, column: &str) -> Vec<(u16, u64)> {
        ranked(&self.values, column)
    }

    /// How many cells the region holds of `label` in `column`.
    ///
    /// Read from the counts rather than from the records, which are only ever
    /// the first page of them.
    pub fn counted_in(&self, column: &str, code: u16) -> Option<u64> {
        self.buckets(column)
            .into_iter()
            .find(|(found, _)| *found == code)
            .map(|(_, count)| count)
    }
}

/// The values of `column` in `counted`, largest first and none of them empty.
fn ranked(counted: &[(String, Vec<(u16, u64)>)], column: &str) -> Vec<(u16, u64)> {
    let mut found: Vec<(u16, u64)> = counted
        .iter()
        .find(|(id, _)| id == column)
        .map(|(_, counted)| counted.clone())
        .unwrap_or_default();
    // Largest first: a selection is read for what is mostly in it, and a
    // taxonomy's own order buries that under hundreds of empty values.
    found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    found.retain(|(_, count)| *count > 0);
    found
}

/// A service that knows a dataset's cells better than its files do: what its
/// codes are called, which color each is drawn in, which properties are worth
/// showing and in what order, and how many cells hold each value.
pub trait DescribeCells: Send + Sync + 'static {
    /// Properties for the columns the files hold. A column the service does
    /// not know is left out or hidden; one the files lack is never offered,
    /// since there would be nothing to read.
    fn describe(&self, columns: CellColumns) -> BoxFuture<'static, Result<CellProperties, String>>;

    /// How many cells hold each value of each categorical property, and fall
    /// in each bucket of each numeric one, among the cells the other
    /// properties' filters admit. Asked separately because it is far slower,
    /// and the labels are worth showing before it answers; and asked again
    /// whenever the filters change.
    fn count(&self, properties: &CellProperties) -> BoxFuture<'static, Result<CellCounts, String>>;

    /// How many cells inside `region` hold each value of each categorical
    /// property, among the cells the other properties' filters admit.
    ///
    /// Separate from [`Self::count`] because a region is dragged out and
    /// redrawn far more often than a filter changes, and because a service may
    /// know a dataset's cells without indexing where they are.
    fn count_region(
        &self,
        _properties: &CellProperties,
        _region: &SelectedRegion,
    ) -> BoxFuture<'static, Result<CellCounts, String>> {
        Box::pin(async { Err("this service cannot count a region".into()) })
    }

    /// The individual cells inside `region`, and of the category `within` when
    /// one has been drilled into, at most `limit` of them.
    ///
    /// The counts say what a rectangle is made of; this says what is actually
    /// in it. A rectangle can hold hundreds of thousands of cells, so only the
    /// first page is ever asked for and the counts are what say how many there
    /// really are.
    fn cells_in(
        &self,
        _properties: &CellProperties,
        _region: &SelectedRegion,
        _within: Option<&RegionFocus>,
        _limit: usize,
    ) -> BoxFuture<'static, Result<Vec<CellRecord>, String>> {
        Box::pin(async { Err("this service cannot list the cells of a region".into()) })
    }

    /// Whether this service can count a region at all. The selection summary
    /// is offered only when it can.
    fn counts_regions(&self) -> bool {
        false
    }

    /// Whether this service knows the genes the dataset measured. The genes
    /// panel is offered only when it does.
    fn has_genes(&self) -> bool {
        false
    }

    /// Genes whose symbol starts with `text`, in no particular case.
    fn search_genes(&self, _text: String) -> BoxFuture<'static, Result<Vec<Gene>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    /// A gene as a numeric property, with the histogram of its expression.
    fn describe_gene(&self, _gene: Gene) -> BoxFuture<'static, Result<CellProperty, String>> {
        Box::pin(async { Err("this service knows no genes".into()) })
    }
}

/// A shared [`DescribeCells`], cheap to clone onto every entry it serves.
#[derive(Clone)]
pub struct CellService(pub Arc<dyn DescribeCells>);

impl std::fmt::Debug for CellService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CellService")
    }
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

    /// The entry listed at `url`, with the name of the catalog listing it.
    pub fn find(&self, url: &str) -> Option<(&str, &Entry)> {
        self.slots.iter().find_map(|slot| {
            slot.entries
                .iter()
                .find(|entry| entry.url == url)
                .map(|entry| (slot.name.as_str(), entry))
        })
    }

    /// Whether any catalog has yet to answer, so an address found in none of
    /// them may still turn up.
    pub fn listing(&self) -> bool {
        self.slots.iter().any(|slot| slot.listing.is_some())
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
        app.init_resource::<Catalogs>().add_systems(
            Update,
            (
                take_listings,
                name_sources,
                cells::ask,
                cells::take_answers,
                cells::recount,
                cells::recount_region,
                genes::offer,
                genes::search,
                genes::add,
            )
                .chain()
                .in_set(Stage::Catalogs),
        );
    }
}

/// Marks a source whose name no catalog has left to settle.
#[derive(Component)]
pub struct Named;

/// Name each source a catalog lists the way the catalog does.
///
/// A format can only name a dataset from its address, which for a Scatterbrain
/// file is a reference id; the catalog it came from has the title people know
/// it by.
fn name_sources(
    mut commands: Commands,
    catalogs: Res<Catalogs>,
    mut sources: Query<(Entity, &SourceUrl, &mut DataSource), Without<Named>>,
) {
    for (entity, url, mut source) in &mut sources {
        match catalogs.find(&url.0) {
            Some((_, entry)) => {
                if source.name != entry.name {
                    source.name = entry.name.clone();
                }
            }
            None if catalogs.listing() => continue,
            None => {}
        }
        commands.entity(entity).insert(Named);
    }
}

fn take_listings(mut catalogs: ResMut<Catalogs>) {
    if catalogs.listing() {
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
                    cells: None,
                })
                .collect();
            Box::pin(async move { Ok(entries) })
        }
    }

    fn listed(catalogs: &mut Catalogs) {
        let started = Instant::now();
        while catalogs.listing() {
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

    #[test]
    fn an_address_is_found_in_whichever_catalog_lists_it() {
        let mut catalogs = Catalogs::default();
        catalogs.add(Fixed("first", vec!["a"]));
        catalogs.add(Fixed("second", vec!["c"]));
        assert!(catalogs.listing());
        listed(&mut catalogs);

        assert!(!catalogs.listing());
        assert_eq!(catalogs.find("c").map(|(name, _)| name), Some("second"));
        assert!(catalogs.find("z").is_none());
    }
}
