//! Answering a source's search for genes, and adding the ones picked.
//!
//! The panel only edits a source's [`GeneSearch`]: what is typed, and which
//! genes are wanted. Here the catalog's service is asked, and what it answers
//! is written back — a search's results into the search, and a gene's
//! property into the source's [`CellProperties`].
//!
//! Genes are offered once a source's labels are in, since a description
//! replaces the properties wholesale and would take any gene added before it
//! away with the placeholders. Counting, which follows, only writes counts in,
//! so they need not wait for that.

use bevy::prelude::*;

use super::cells::Described;
use super::{Catalogs, CellService, Entry};
use crate::app::net::{Fetching, fetching};
use crate::source::SourceUrl;
use crate::source::genes::{Gene, GeneSearch, ReadsGenes, SearchState};
use crate::source::properties::{CellProperties, CellProperty};

/// The service a source's genes come from, if it has one. Absent until the
/// source has been looked at, and `None` once it has been and none knows its
/// genes, so it is not looked at again.
#[derive(Component)]
pub struct GeneService(Option<CellService>);

impl GeneService {
    /// Whether anything knows this source's genes.
    pub fn knows_genes(&self) -> bool {
        self.0.is_some()
    }
}

/// What has been asked of the service for one source.
#[derive(Component, Default)]
pub struct GeneReads {
    /// The query last sent, so a search is asked once per change of text.
    asked: String,
    searching: Option<Fetching<Result<Vec<Gene>, String>>>,
    adding: Vec<(String, Fetching<Result<CellProperty, String>>)>,
}

/// Offer genes on each source that can read them and whose catalog knows them.
pub fn offer(
    mut commands: Commands,
    catalogs: Res<Catalogs>,
    sources: Query<(Entity, &SourceUrl), (With<ReadsGenes>, With<Described>, Without<GeneService>)>,
) {
    for (entity, url) in &sources {
        let service = match catalogs.find(&url.0) {
            Some((
                _,
                Entry {
                    cells: Some(service),
                    ..
                },
            )) if service.0.has_genes() => Some(service.clone()),
            _ => None,
        };
        let mut source = commands.entity(entity);
        if service.is_some() {
            source.insert((GeneSearch::default(), GeneReads::default()));
        }
        source.insert(GeneService(service));
    }
}

/// Ask the service whenever the query changes, and take in what it finds.
///
/// A new query drops the read of the old one, which cancels it: only the
/// latest text's answer is worth waiting for.
pub fn search(mut sources: Query<(&GeneService, &mut GeneSearch, &mut GeneReads)>) {
    for (service, mut search, mut reads) in &mut sources {
        let Some(service) = &service.0 else { continue };
        let query = search.query.trim().to_string();
        if query != reads.asked {
            reads.asked = query.clone();
            if query.is_empty() {
                reads.searching = None;
                search.results.clear();
                search.state = SearchState::Idle;
            } else {
                reads.searching = Some(fetching(service.0.search_genes(query)));
                search.state = SearchState::Searching;
            }
        }

        let Some(result) = reads.searching.as_mut().and_then(Fetching::take) else {
            continue;
        };
        reads.searching = None;
        match result {
            Ok(found) => {
                search.results = found;
                search.state = SearchState::Idle;
            }
            Err(error) => {
                warn!("gene search for {:?}: {error}", reads.asked);
                search.results.clear();
                search.state = SearchState::Failed(error);
            }
        }
    }
}

/// Describe each gene asked for, and add it to the source's properties once
/// its histogram is in.
pub fn add(
    mut sources: Query<(
        &GeneService,
        &mut GeneSearch,
        &mut GeneReads,
        &mut CellProperties,
    )>,
) {
    for (service, mut search, mut reads, mut properties) in &mut sources {
        let Some(service) = &service.0 else { continue };
        for gene in &search.adding {
            if !reads.adding.iter().any(|(id, _)| *id == gene.id) {
                info!("adding gene {}", gene.symbol);
                let reading = fetching(service.0.describe_gene(gene.clone()));
                reads.adding.push((gene.id.clone(), reading));
            }
        }

        let mut landed = Vec::new();
        for (id, reading) in &mut reads.adding {
            if let Some(result) = reading.take() {
                landed.push((id.clone(), result));
            }
        }
        for (id, result) in landed {
            reads.adding.retain(|(adding, _)| *adding != id);
            search.adding.retain(|gene| gene.id != id);
            match result {
                Ok(gene) => properties.add_gene(gene),
                Err(error) => {
                    warn!("could not add gene {id}: {error}");
                    search.state = SearchState::Failed(error);
                }
            }
        }
    }
}
