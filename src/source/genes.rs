//! Genes a dataset's cells can be colored and filtered by.
//!
//! A dataset measures thousands of genes, far too many to offer as properties
//! up front, so they are searched for by name and added one at a time. Each
//! one added becomes a numeric [`CellProperty`](super::properties::CellProperty)
//! whose `gene` says where its values are read, and from then on is colored
//! and filtered like any other.
//!
//! Two things have to be true to offer genes: the format can read expression
//! values, which it says with [`ReadsGenes`], and a service knows the
//! dataset's genes, which is where [`GeneSearch`] comes from. The panel edits
//! the search; whatever knows the genes answers it.

use bevy::prelude::*;

/// A gene a dataset measures.
#[derive(Debug, Clone, PartialEq)]
pub struct Gene {
    /// The identifier the service knows it by, such as an Ensembl id.
    pub id: String,
    pub symbol: String,
    /// Where its values sit in the dataset's expression files.
    pub index: u32,
    /// The highest expression any cell shows. The lowest is always zero: most
    /// cells express any one gene not at all.
    pub high: f32,
}

/// Marks a source whose files hold expression values that can be read by a
/// gene's index.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ReadsGenes;

/// A source's search for genes, and the genes asked to be added.
///
/// Carried only by sources that can offer genes, so its presence is what shows
/// the panel.
#[derive(Component, Debug, Clone, Default)]
pub struct GeneSearch {
    /// What has been typed. Written by the panel.
    pub query: String,
    /// What the query found. Written by the service.
    pub results: Vec<Gene>,
    pub state: SearchState,
    /// Genes asked for and not yet added, waiting on their histograms.
    pub adding: Vec<Gene>,
}

impl GeneSearch {
    /// Ask for a gene to be added, once however often it is asked.
    pub fn add(&mut self, gene: Gene) {
        if !self.adding.iter().any(|pending| pending.id == gene.id) {
            self.adding.push(gene);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SearchState {
    #[default]
    Idle,
    Searching,
    Failed(String),
}
