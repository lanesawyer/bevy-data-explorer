//! Where a record came from and what came of it, as a graph a frame draws.
//!
//! Another question the frame asks and a format answers. A format with
//! records that are made from one another — the BKP Registry's processes,
//! and the specimens and data assets they take in and put out — writes a
//! [`SourceLineage`] and draws nothing itself. The frame lays it out, and
//! writes [`LineageAsked`] when a record's links are wanted or a crowd of
//! them is to be shown whole; the format fetches the links and adds them.

use std::collections::{BTreeSet, HashMap, HashSet};

use bevy::prelude::*;

/// One record in a lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageNode {
    /// What the format knows it by. Nothing else reads it.
    pub id: String,
    pub name: String,
    /// What kind of record it is and how it stands, in a few words.
    pub detail: String,
    /// Whether it is a step that made something rather than something made.
    pub step: bool,
    /// Where it is stored, if it is something stored.
    pub address: Option<String>,
    /// Columns from the one the lineage is drawn from: before it, negative.
    pub depth: i32,
    /// The record it was first reached from, which it is drawn beside and
    /// hidden with. None for the one the lineage is drawn from.
    pub parent: Option<usize>,
    pub links: Links,
}

/// Whether a record's own links have been read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Links {
    /// It has none to read, as a subject in the registry has none.
    None,
    Unread,
    Fetching,
    Read,
    Failed(String),
}

/// A lineage: the records in it, and which was made from which.
///
/// Grown only through [`SourceLineage::link`], which keeps it indexed: one
/// read can add thirteen thousand records, as an export's input did that was
/// put out by every alignment run before it.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SourceLineage {
    nodes: Vec<LineageNode>,
    /// Each by its place in `nodes`, the one taken in first.
    edges: Vec<(usize, usize)>,
    places: HashMap<String, usize>,
    linked: HashSet<(usize, usize)>,
}

impl SourceLineage {
    /// A lineage of one record, the one it is drawn from.
    pub fn new(root: LineageNode) -> Self {
        SourceLineage {
            places: [(root.id.clone(), 0)].into(),
            nodes: vec![root],
            edges: Vec::new(),
            linked: HashSet::new(),
        }
    }

    pub fn nodes(&self) -> &[LineageNode] {
        &self.nodes
    }

    pub fn node_mut(&mut self, at: usize) -> &mut LineageNode {
        &mut self.nodes[at]
    }

    pub fn edges(&self) -> &[(usize, usize)] {
        &self.edges
    }

    /// The place of the record known as `id`.
    pub fn find(&self, id: &str) -> Option<usize> {
        self.places.get(id).copied()
    }

    /// Add a record linked to the one at `from`, before it if `before`, and
    /// say where it is. One already there keeps its place and depth: the
    /// first way it was reached is where it is drawn.
    pub fn link(&mut self, from: usize, mut node: LineageNode, before: bool) -> usize {
        let at = match self.find(&node.id) {
            Some(at) => at,
            None => {
                node.depth = self.nodes[from].depth + if before { -1 } else { 1 };
                node.parent = Some(from);
                let at = self.nodes.len();
                self.places.insert(node.id.clone(), at);
                self.nodes.push(node);
                at
            }
        };
        let edge = if before { (at, from) } else { (from, at) };
        if self.linked.insert(edge) {
            self.edges.push(edge);
        }
        at
    }
}

/// What the frame has asked of a lineage: the records whose links are
/// wanted, and those whose links are shown whole rather than cut short.
/// Both by [`LineageNode::id`].
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageAsked {
    pub expanded: BTreeSet<String>,
    pub unfolded: BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> LineageNode {
        LineageNode {
            id: id.into(),
            name: id.into(),
            detail: String::new(),
            step: false,
            address: None,
            depth: 0,
            parent: None,
            links: Links::Unread,
        }
    }

    #[test]
    fn a_record_reached_twice_is_drawn_once_where_it_was_first_found() {
        let mut lineage = SourceLineage::new(node("p"));
        let input = lineage.link(0, node("a"), true);
        let output = lineage.link(0, node("b"), false);
        assert_eq!(lineage.nodes()[input].depth, -1);
        assert_eq!(lineage.nodes()[output].depth, 1);
        assert_eq!(lineage.nodes()[output].parent, Some(0));
        assert_eq!(lineage.edges(), [(1, 0), (0, 2)]);
        // Read again from the other side.
        let again = lineage.link(output, node("a"), false);
        assert_eq!(again, input);
        assert_eq!(lineage.nodes()[input].depth, -1);
        assert_eq!(lineage.nodes().len(), 3);
        assert_eq!(lineage.edges().len(), 3);
        lineage.link(0, node("a"), true);
        assert_eq!(lineage.edges().len(), 3, "no edge twice");
    }
}
