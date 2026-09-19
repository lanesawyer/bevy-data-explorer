//! A property whose values nest: a taxonomy of cell types, or an atlas of
//! regions within regions.
//!
//! Each level is a column of its own — a cell has a class, a subclass, a
//! supertype — and each value but the coarsest has one parent a level up. So
//! one filter serves the whole tree: ticking a node admits every cell under it,
//! whichever level it is at, and that comes down to a set of codes in the
//! finest column. The streamers see an ordinary categorical restriction.
//!
//! Ticks are kept minimal: a node is ticked, or implied by a ticked ancestor,
//! and never both. Ticking every child of a node ticks the node instead, and
//! unticking one child of a ticked node splits the tick among its siblings, so
//! the set of cells admitted is always exactly what the boxes on screen show.

use std::collections::HashSet;

use super::properties::PropertyValue;

/// One level of a tree, and the column its codes are stored in.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeLevel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    /// Index into [`Tree::levels`], coarsest first.
    pub level: usize,
    /// Index into [`Tree::nodes`] of the node a level up, if there is one.
    pub parent: Option<usize>,
    /// Its code in its level's column, label, color, count, and whether this
    /// node itself is ticked.
    pub value: PropertyValue,
}

#[derive(Debug, Clone)]
pub struct Tree {
    pub levels: Vec<TreeLevel>,
    /// Siblings in the order they are listed.
    pub nodes: Vec<TreeNode>,
    /// The level points are colored by when this tree colors them.
    pub color_level: usize,
}

impl Tree {
    /// The nodes directly under `parent`, or the roots for `None`, in order.
    pub fn children(&self, parent: Option<usize>) -> impl Iterator<Item = usize> + '_ {
        self.nodes
            .iter()
            .enumerate()
            .filter(move |(_, node)| node.parent == parent)
            .map(|(index, _)| index)
    }

    pub fn has_children(&self, node: usize) -> bool {
        self.nodes.iter().any(|other| other.parent == Some(node))
    }

    pub fn level_values(&self, level: usize) -> impl Iterator<Item = &PropertyValue> {
        self.nodes
            .iter()
            .filter(move |node| node.level == level)
            .map(|node| &node.value)
    }

    pub fn level_values_mut(&mut self, level: usize) -> impl Iterator<Item = &mut PropertyValue> {
        self.nodes
            .iter_mut()
            .filter(move |node| node.level == level)
            .map(|node| &mut node.value)
    }

    /// `node` and every node above it, nearest first.
    fn lineage(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(node), |&at| self.nodes[at].parent)
    }

    /// Whether cells under this node are admitted: it is ticked, or something
    /// above it is.
    pub fn checked(&self, node: usize) -> bool {
        self.lineage(node).any(|at| self.nodes[at].value.selected)
    }

    /// Whether some but not all of the cells under this node are admitted.
    pub fn partly_checked(&self, node: usize) -> bool {
        !self.checked(node)
            && self
                .nodes
                .iter()
                .enumerate()
                .any(|(other, found)| found.value.selected && self.is_below(other, node))
    }

    /// Whether `node` sits somewhere under `ancestor`.
    fn is_below(&self, node: usize, ancestor: usize) -> bool {
        self.lineage(node).skip(1).any(|at| at == ancestor)
    }

    /// Tick or untick one node, keeping the ticks minimal.
    pub fn set(&mut self, node: usize, on: bool) {
        if node >= self.nodes.len() || self.checked(node) == on {
            return;
        }
        if on {
            self.nodes[node].value.selected = true;
            self.clear_below(node);
            // Every sibling ticked is the parent ticked.
            let mut at = node;
            while let Some(parent) = self.nodes[at].parent {
                if !self
                    .children(Some(parent))
                    .all(|child| self.nodes[child].value.selected)
                {
                    break;
                }
                self.nodes[parent].value.selected = true;
                self.clear_below(parent);
                at = parent;
            }
        } else if self.nodes[node].value.selected {
            self.nodes[node].value.selected = false;
        } else {
            // Implied by an ancestor: untick that, and tick everything it
            // covered except the path down to this node.
            let path: Vec<usize> = self.lineage(node).collect();
            let Some(ticked) = path.iter().position(|&at| self.nodes[at].value.selected) else {
                return;
            };
            self.nodes[path[ticked]].value.selected = false;
            for step in (1..=ticked).rev() {
                let (above, below) = (path[step], path[step - 1]);
                let siblings: Vec<usize> = self.children(Some(above)).collect();
                for sibling in siblings {
                    if sibling != below {
                        self.nodes[sibling].value.selected = true;
                    }
                }
            }
        }
    }

    fn clear_below(&mut self, node: usize) {
        for other in 0..self.nodes.len() {
            if self.is_below(other, node) {
                self.nodes[other].value.selected = false;
            }
        }
    }

    pub fn clear(&mut self) {
        for node in &mut self.nodes {
            node.value.selected = false;
        }
    }

    /// How many nodes are ticked.
    pub fn applied(&self) -> usize {
        self.nodes.iter().filter(|node| node.value.selected).count()
    }

    /// The column filtering happens in: the finest level.
    pub fn filter_column(&self) -> &str {
        self.levels.last().map_or("", |level| level.id.as_str())
    }

    /// Codes in the finest column whose cells are admitted.
    pub fn admitted(&self) -> HashSet<u16> {
        let finest = self.levels.len().saturating_sub(1);
        self.nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| node.level == finest && self.checked(*index))
            .map(|(_, node)| node.value.code)
            .collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn node(level: usize, parent: Option<usize>, code: u16) -> TreeNode {
        TreeNode {
            level,
            parent,
            value: PropertyValue {
                code,
                label: format!("{level}.{code}"),
                color: None,
                count: None,
                selected: false,
            },
        }
    }

    /// Two classes; the first holds subclasses 10 and 11, the second 12.
    pub fn tree() -> Tree {
        Tree {
            levels: vec![
                TreeLevel {
                    id: "class".into(),
                    name: "Class".into(),
                },
                TreeLevel {
                    id: "subclass".into(),
                    name: "Subclass".into(),
                },
            ],
            nodes: vec![
                node(0, None, 0),
                node(0, None, 1),
                node(1, Some(0), 10),
                node(1, Some(0), 11),
                node(1, Some(1), 12),
            ],
            color_level: 0,
        }
    }

    #[test]
    fn ticking_a_node_admits_everything_under_it() {
        let mut tree = tree();
        tree.set(0, true);
        assert_eq!(tree.admitted(), HashSet::from([10, 11]));
        assert!(tree.checked(2) && tree.checked(3) && !tree.checked(4));
        assert_eq!(tree.filter_column(), "subclass");
    }

    #[test]
    fn ticking_every_child_ticks_the_parent_instead() {
        let mut tree = tree();
        tree.set(2, true);
        assert!(tree.partly_checked(0));
        tree.set(3, true);
        assert!(tree.nodes[0].value.selected);
        assert_eq!(tree.applied(), 1, "one tick, not three");
        assert!(!tree.partly_checked(0));
    }

    #[test]
    fn unticking_under_a_ticked_parent_splits_the_tick() {
        let mut tree = tree();
        tree.set(0, true);
        tree.set(3, false);
        assert!(!tree.nodes[0].value.selected);
        assert!(tree.nodes[2].value.selected);
        assert_eq!(tree.admitted(), HashSet::from([10]));
    }

    #[test]
    fn ticking_a_parent_absorbs_ticks_below_it() {
        let mut tree = tree();
        tree.set(2, true);
        tree.set(0, true);
        assert_eq!(tree.applied(), 1);
        tree.set(0, false);
        assert!(tree.admitted().is_empty());
    }
}
