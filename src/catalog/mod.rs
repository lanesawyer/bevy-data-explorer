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
//!
//! A catalog too large to list, such as the BKP Registry, is searched instead:
//! it lists nothing, and is asked again whenever what is typed into a picker
//! settles. What it answers replaces what it answered before.
//!
//! A catalog may belong to a [`Provider`], which settings can turn off. One
//! turned off offers nothing to a picker and is never searched, but is still
//! listed: a bookmark or a pasted address may name a dataset it knows, and
//! that dataset should still be named, and its cells described, as the
//! catalog has them. A catalog with no provider, like the examples, is always
//! on.

pub mod bkp;
pub mod cells;
pub mod examples;
pub mod genes;
pub mod registry;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use bevy::prelude::*;
use futures::future::BoxFuture;

use crate::app::net::{Fetching, fetching};
use crate::app::prefs::Preferences;
use crate::app::schedule::Stage;
use crate::source::genes::Gene;
use crate::source::properties::{CellColumns, CellProperties, CellProperty};
use crate::source::region::SelectedRegion;
use crate::source::{Category, DataSource, SourceUrl};
use examples::Example;

/// One dataset a catalog offers.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    /// What kind of dataset it is, in the words shown beside it.
    pub kind: String,
    /// What it is drawn as, for filtering by.
    pub category: Category,
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
    /// How each column's values break down by the column being colored by:
    /// the column, then one triple per group of (its code, the color's code,
    /// how many cells hold both).
    pub mixes: Vec<(String, Vec<(u16, u16, u64)>)>,
    /// The column the mixes were crossed against. Nothing when the service
    /// did not cross them — a gradient has no codes to cross, and a coloring
    /// with more values than a bar can show is not worth the bytes.
    pub mix_column: Option<String>,
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

/// Whoever runs a catalog, as settings offers it to be turned off. Several
/// catalogs may share one, and are turned off together.
#[derive(Clone, Copy)]
pub struct Provider {
    /// What the preferences remember it by. Never change one: a user who
    /// turned it off would find it back on.
    pub key: &'static str,
    pub name: &'static str,
    /// A line on what it offers, under its switch.
    pub about: &'static str,
    /// What an empty window offers from it, under its name, while it is on.
    pub examples: &'static [Example],
}

/// Somewhere datasets are listed.
pub trait Catalog: Send + Sync + 'static {
    /// What to call it, in search and in the log.
    fn name(&self) -> &str;

    /// Who runs it, if it can be turned off.
    fn provider(&self) -> Option<Provider> {
        None
    }

    /// Everything it offers. Run once, off the main thread.
    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>>;

    /// Whether it is too large to list, and is searched instead. One that is
    /// never has [`Self::list`] called.
    fn searched(&self) -> bool {
        false
    }

    /// What matches `text`, which is at least [`SEARCH_MIN_CHARS`] long, of
    /// the category `only` when one is named.
    fn search(
        &self,
        _text: String,
        _only: Option<Category>,
    ) -> BoxFuture<'static, Result<Found, String>> {
        Box::pin(async { Ok(Found::default()) })
    }
}

/// What a searched catalog answered: the entries it sent, and how many
/// matched in all.
#[derive(Default)]
pub struct Found {
    pub entries: Vec<Entry>,
    pub total: usize,
}

/// The shortest text a searched catalog is asked about. Shorter matches too
/// much to be worth a request.
pub const SEARCH_MIN_CHARS: usize = 3;

/// How long typing must pause before a searched catalog is asked, so a word
/// is one request rather than one a letter.
const SEARCH_AFTER_SECS: f32 = 0.35;

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
    provider: Option<Provider>,
    catalog: Arc<dyn Catalog>,
    on: bool,
    entries: Vec<Entry>,
    listing: Option<Fetching<Result<Vec<Entry>, String>>>,
    search: Option<Search>,
}

/// A searched catalog's side of its slot.
struct Search {
    /// The text last asked about, whether or not it has answered, and the
    /// category it was narrowed to.
    asked: String,
    asked_only: Option<Category>,
    asking: Option<Fetching<Result<Found, String>>>,
    /// How many matched the last answer, of which `entries` are the first.
    total: usize,
    /// Why the last search failed.
    problem: Option<String>,
    /// Every entry any search has answered, so that one opened from an
    /// earlier search is still named after it once the search has moved on.
    seen: HashMap<String, Entry>,
}

impl Search {
    /// Whether what was last asked about holds everything `text` could
    /// match in `only`, so its answer is worth showing for it.
    fn answers(&self, text: &str, only: Option<Category>) -> bool {
        self.asked.chars().count() >= SEARCH_MIN_CHARS
            && (self.asked_only.is_none() || self.asked_only == only)
            && text
                .trim()
                .to_lowercase()
                .contains(&self.asked.to_lowercase())
    }

    fn asked_for(&self, text: &str, only: Option<Category>) -> bool {
        self.asked == text && self.asked_only == only
    }
}

/// What a picker shows under a searched catalog's heading, in place of or
/// after its entries.
pub enum SearchNote {
    /// Nothing has been typed, or not enough.
    TypeToSearch,
    Searching,
    /// More matched than were sent.
    Showing {
        shown: usize,
        total: usize,
    },
    Failed(String),
}

impl SearchNote {
    pub fn text(&self) -> String {
        match self {
            SearchNote::TypeToSearch => {
                format!("Type {SEARCH_MIN_CHARS} or more characters to search")
            }
            SearchNote::Searching => "Searching\u{2026}".into(),
            SearchNote::Showing { shown, total } => {
                format!("The first {shown} of {total} matches. Type more to narrow them.")
            }
            SearchNote::Failed(problem) => problem.clone(),
        }
    }
}

/// Every registered catalog, and what each has listed so far.
#[derive(Resource, Default)]
pub struct Catalogs {
    slots: Vec<Slot>,
    /// Bumped whenever any catalog's entries or notes change, so a list built
    /// from them can tell it is stale.
    generation: usize,
    /// The search typed, the category it is narrowed to, and when either
    /// last changed.
    wanted: String,
    wanted_only: Option<Category>,
    wanted_at: f32,
    /// The keys of the providers turned off.
    off: BTreeSet<String>,
}

impl Catalogs {
    /// Register `catalog` and start listing it, unless it is searched.
    pub fn add(&mut self, catalog: impl Catalog) {
        let provider = catalog.provider();
        let search = catalog.searched().then(|| Search {
            asked: String::new(),
            asked_only: None,
            asking: None,
            total: 0,
            problem: None,
            seen: HashMap::new(),
        });
        self.slots.push(Slot {
            name: catalog.name().to_string(),
            on: provider.is_none_or(|provider| !self.off.contains(provider.key)),
            provider,
            listing: search.is_none().then(|| fetching(catalog.list())),
            catalog: Arc::new(catalog),
            entries: Vec::new(),
            search,
        });
    }

    /// Turn off the catalogs of the providers keyed in `off`, and on the
    /// rest.
    pub fn turn_off(&mut self, off: &BTreeSet<String>) {
        if self.off == *off {
            return;
        }
        self.off.clone_from(off);
        for slot in &mut self.slots {
            slot.on = slot
                .provider
                .is_none_or(|provider| !off.contains(provider.key));
            if !slot.on
                && let Some(search) = slot.search.as_mut()
            {
                // Forgotten, so it is asked afresh when turned back on.
                search.asking = None;
                search.asked.clear();
                search.asked_only = None;
                search.total = 0;
                slot.entries.clear();
            }
        }
        self.generation += 1;
    }

    /// Every provider some catalog names, once each, in the order registered.
    pub fn providers(&self) -> Vec<Provider> {
        let mut providers: Vec<Provider> = Vec::new();
        for provider in self.slots.iter().filter_map(|slot| slot.provider) {
            if !providers.iter().any(|known| known.key == provider.key) {
                providers.push(provider);
            }
        }
        providers
    }

    /// Note what a picker's search now says. The searched catalogs are asked
    /// once it has sat still for a moment.
    pub fn want(&mut self, text: &str, only: Option<Category>, now: f32) {
        let text = text.trim();
        if self.wanted != text || self.wanted_only != only {
            self.wanted = text.to_string();
            self.wanted_only = only;
            self.wanted_at = now;
            self.generation += 1;
        }
    }

    /// The searched catalogs by name, and what to say under each for `text`.
    pub fn search_notes(
        &self,
        text: &str,
        only: Option<Category>,
    ) -> Vec<(&str, Option<SearchNote>)> {
        let text = text.trim();
        self.slots
            .iter()
            .filter(|slot| slot.on)
            .filter_map(|slot| {
                let search = slot.search.as_ref()?;
                let note = if text.chars().count() < SEARCH_MIN_CHARS {
                    Some(SearchNote::TypeToSearch)
                } else if self.wanted == text
                    && self.wanted_only == only
                    && (search.asking.is_some() || !search.asked_for(text, only))
                {
                    Some(SearchNote::Searching)
                } else if !search.answers(text, only) {
                    // Another picker's search, not this one's.
                    None
                } else if let Some(problem) = &search.problem {
                    Some(SearchNote::Failed(problem.clone()))
                } else if search.total > slot.entries.len() {
                    Some(SearchNote::Showing {
                        shown: slot.entries.len(),
                        total: search.total,
                    })
                } else {
                    None
                };
                Some((slot.name.as_str(), note))
            })
            .collect()
    }

    /// Ask each searched catalog about the text wanted, once typing has
    /// paused. Asking again drops the request before, which stops it.
    fn search(&mut self, now: f32) {
        if now - self.wanted_at < SEARCH_AFTER_SECS {
            return;
        }
        let text = self.wanted.clone();
        let only = self.wanted_only;
        let long_enough = text.chars().count() >= SEARCH_MIN_CHARS;
        for slot in self.slots.iter_mut().filter(|slot| slot.on) {
            let Some(search) = slot.search.as_mut() else {
                continue;
            };
            if search.asked_for(&text, only) {
                continue;
            }
            search.asked = text.clone();
            search.asked_only = only;
            search.problem = None;
            search.total = 0;
            slot.entries.clear();
            search.asking = long_enough.then(|| fetching(slot.catalog.search(text.clone(), only)));
            self.generation += 1;
        }
    }

    fn take_searches(&mut self) {
        for slot in &mut self.slots {
            let Some(search) = slot.search.as_mut() else {
                continue;
            };
            let Some(result) = search.asking.as_mut().and_then(Fetching::take) else {
                continue;
            };
            search.asking = None;
            match result {
                Ok(found) => {
                    info!(
                        "{}: {} match \"{}\", {} sent",
                        slot.name,
                        found.total,
                        search.asked,
                        found.entries.len()
                    );
                    for entry in &found.entries {
                        search.seen.insert(entry.url.clone(), entry.clone());
                    }
                    search.total = found.total;
                    slot.entries = found.entries;
                }
                Err(e) => {
                    warn!(
                        "{}: could not search for \"{}\": {e}",
                        slot.name, search.asked
                    );
                    search.problem = Some(e);
                }
            }
            self.generation += 1;
        }
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.slots.get(id.catalog)?.entries.get(id.entry)
    }

    /// How many entries have landed, across every catalog.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.slots.iter().map(|slot| slot.entries.len()).sum()
    }

    /// Changes whenever what a picker would show from the catalogs does.
    pub fn generation(&self) -> usize {
        self.generation
    }

    /// The entry listed at `url`, with the name of the catalog listing it.
    /// A searched catalog also answers for what earlier searches found.
    pub fn find(&self, url: &str) -> Option<(&str, &Entry)> {
        self.slots.iter().find_map(|slot| {
            slot.entries
                .iter()
                .find(|entry| entry.url == url)
                .or_else(|| slot.search.as_ref()?.seen.get(url))
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
    ///
    /// A searched catalog's entries are offered only to a picker whose
    /// search they answer: several pickers may be open, and only one of
    /// them was asked about.
    pub fn unopened<'a>(
        &'a self,
        opened: &'a [&'a str],
        query: &'a str,
        only: Option<Category>,
    ) -> impl Iterator<Item = (EntryId, &'a str, &'a Entry)> + 'a {
        self.slots
            .iter()
            .enumerate()
            .filter(move |(_, slot)| {
                slot.on
                    && slot
                        .search
                        .as_ref()
                        .is_none_or(|search| search.answers(query, only))
            })
            .flat_map(|(catalog, slot)| {
                slot.entries.iter().enumerate().map(move |(entry, found)| {
                    (EntryId { catalog, entry }, slot.name.as_str(), found)
                })
            })
            .filter(move |(_, _, entry)| {
                !opened.contains(&entry.url.as_str())
                    && only.is_none_or(|only| entry.category == only)
            })
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
                    self.generation += 1;
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
        let off = self
            .world()
            .get_resource::<Preferences>()
            .map(|prefs| prefs.sources_off.clone())
            .unwrap_or_default();
        let mut catalogs = self.world_mut().get_resource_or_init::<Catalogs>();
        catalogs.turn_off(&off);
        catalogs.add(catalog);
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
                turn_off_sources,
                take_listings,
                search_catalogs,
                registry::sync_token,
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

fn turn_off_sources(prefs: Res<Preferences>, mut catalogs: ResMut<Catalogs>) {
    if prefs.is_changed() && catalogs.off != prefs.sources_off {
        catalogs.turn_off(&prefs.sources_off);
    }
}

fn take_listings(mut catalogs: ResMut<Catalogs>) {
    if catalogs.listing() {
        catalogs.take_listings();
    }
}

fn search_catalogs(mut catalogs: ResMut<Catalogs>, time: Res<Time>) {
    // Checked before writing, so a frame with nothing to do leaves the
    // resource unchanged.
    let now = time.elapsed_secs();
    let due = catalogs.slots.iter().filter(|slot| slot.on).any(|slot| {
        slot.search.as_ref().is_some_and(|search| {
            search.asking.is_some() || !search.asked_for(&catalogs.wanted, catalogs.wanted_only)
        })
    });
    if due {
        catalogs.search(now);
        catalogs.take_searches();
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
                    category: Category::Image,
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

    struct Searched;

    impl Catalog for Searched {
        fn name(&self) -> &str {
            "searched"
        }

        fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
            unreachable!("a searched catalog is never listed")
        }

        fn searched(&self) -> bool {
            true
        }

        fn search(
            &self,
            text: String,
            only: Option<Category>,
        ) -> BoxFuture<'static, Result<Found, String>> {
            let entry = Entry {
                name: text.clone(),
                kind: String::new(),
                category: only.unwrap_or(Category::Image),
                url: format!("s3://bucket/{text}"),
                keywords: String::new(),
                cells: None,
            };
            Box::pin(async move {
                Ok(Found {
                    entries: vec![entry],
                    total: 70,
                })
            })
        }
    }

    fn searched_for(catalogs: &mut Catalogs, text: &str, now: &mut f32) {
        catalogs.want(text, None, *now);
        *now += 1.0;
        catalogs.search(*now);
        let started = Instant::now();
        while catalogs.slots[0].search.as_ref().unwrap().asking.is_some() {
            assert!(started.elapsed() < Duration::from_secs(2));
            catalogs.take_searches();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_searched_catalog_answers_what_is_typed_and_remembers_what_it_found() {
        let mut catalogs = Catalogs::default();
        catalogs.add(Searched);
        assert!(!catalogs.listing());
        let mut now = 0.0;

        searched_for(&mut catalogs, "ab", &mut now);
        assert_eq!(catalogs.len(), 0, "too short to ask about");
        assert!(matches!(
            catalogs.search_notes("ab", None)[0].1,
            Some(SearchNote::TypeToSearch)
        ));

        searched_for(&mut catalogs, "brain", &mut now);
        assert_eq!(catalogs.len(), 1);
        assert!(matches!(
            catalogs.search_notes("brain", None)[0].1,
            Some(SearchNote::Showing {
                shown: 1,
                total: 70
            })
        ));
        assert_eq!(catalogs.unopened(&[], "brain scan", None).count(), 1);
        assert_eq!(
            catalogs.unopened(&[], "", None).count(),
            0,
            "another picker's empty search is not offered this answer"
        );
        catalogs.want("brains", None, now);
        assert!(matches!(
            catalogs.search_notes("brains", None)[0].1,
            Some(SearchNote::Searching)
        ));
        assert!(
            catalogs.search_notes("other", None)[0].1.is_none(),
            "a picker nobody is typing in is not left saying it is searching"
        );

        searched_for(&mut catalogs, "", &mut now);
        assert_eq!(catalogs.len(), 0);
        assert!(
            catalogs.find("s3://bucket/brain").is_some(),
            "what was opened from a search is still named after it"
        );
    }

    #[test]
    fn typing_is_asked_about_only_once_it_pauses() {
        let mut catalogs = Catalogs::default();
        catalogs.add(Searched);
        catalogs.want("brain", None, 10.0);
        catalogs.search(10.1);
        assert!(catalogs.slots[0].search.as_ref().unwrap().asking.is_none());
        catalogs.search(10.0 + SEARCH_AFTER_SECS);
        assert!(catalogs.slots[0].search.as_ref().unwrap().asking.is_some());
    }

    #[test]
    fn entries_keep_their_catalog_order_and_skip_what_is_open() {
        let mut catalogs = Catalogs::default();
        catalogs.add(Fixed("first", vec!["a", "b"]));
        catalogs.add(Fixed("second", vec!["c"]));
        listed(&mut catalogs);

        let urls: Vec<&str> = catalogs
            .unopened(&["b"], "", None)
            .map(|(_, _, entry)| entry.url.as_str())
            .collect();
        assert_eq!(urls, ["a", "c"]);
        assert_eq!(catalogs.len(), 3);

        let (id, name, _) = catalogs.unopened(&[], "", None).last().unwrap();
        assert_eq!(name, "second");
        assert_eq!(catalogs.get(id).unwrap().url, "c");
    }

    struct Provided(Fixed);

    const PROVIDER: Provider = Provider {
        key: "provided",
        name: "Provided",
        about: "",
        examples: &[],
    };

    impl Catalog for Provided {
        fn name(&self) -> &str {
            self.0.name()
        }

        fn provider(&self) -> Option<Provider> {
            Some(PROVIDER)
        }

        fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
            self.0.list()
        }
    }

    #[test]
    fn a_provider_turned_off_offers_nothing_but_still_names_what_it_knows() {
        let off = BTreeSet::from([PROVIDER.key.to_string()]);
        let mut catalogs = Catalogs::default();
        catalogs.turn_off(&off);
        catalogs.add(Provided(Fixed("provided", vec!["a"])));
        catalogs.add(Fixed("always", vec!["b"]));
        let keys: Vec<&str> = catalogs.providers().iter().map(|p| p.key).collect();
        assert_eq!(keys, [PROVIDER.key]);
        listed(&mut catalogs);
        assert_eq!(catalogs.unopened(&[], "", None).count(), 1);
        assert!(
            catalogs.find("a").is_some(),
            "still listed, so what it knows is named after it"
        );

        catalogs.turn_off(&BTreeSet::new());
        assert_eq!(catalogs.unopened(&[], "", None).count(), 2);

        catalogs.turn_off(&off);
        let urls: Vec<&str> = catalogs
            .unopened(&[], "", None)
            .map(|(_, _, entry)| entry.url.as_str())
            .collect();
        assert_eq!(urls, ["b"]);
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
