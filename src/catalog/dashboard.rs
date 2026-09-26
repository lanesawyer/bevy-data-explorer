//! A data source's own front page: what it holds, counted, and a few of its
//! datasets worth opening first.
//!
//! What is worth saying differs from one source to the next — a registry of
//! millions of assets counts them by modality and species, a portal of a few
//! dozen might only name its newest — so a catalog does not fill in a fixed
//! form. It composes a [`Dashboard`] from a handful of [`Block`]s, in whatever
//! order suits it, and the home page draws whatever it was given. A block
//! says what a thing is, never how it is drawn, so a source can be given a
//! dashboard without a line changing in `ui`.
//!
//! The datasets a dashboard offers are [`Entry`]s like any a picker lists,
//! opened through [`crate::formats::discover`] the same way, and named after
//! the entry once open.

use super::Entry;

/// Everything a source's front page shows, in order.
#[derive(Clone, Debug, Default)]
pub struct Dashboard {
    pub blocks: Vec<Block>,
}

/// One part of a dashboard.
#[derive(Clone, Debug)]
pub enum Block {
    /// Headline numbers, side by side.
    Figures(Vec<Figure>),
    /// How one count divides, largest first. Every bar is of the same thing,
    /// so they can be drawn against one scale.
    Breakdown { title: String, bars: Vec<Bar> },
    /// Datasets to open, each a button.
    Datasets {
        title: String,
        /// A line under the title, on what they have in common.
        note: Option<String>,
        entries: Vec<Entry>,
    },
}

/// A headline number, with what it counts under it.
#[derive(Clone, Debug)]
pub struct Figure {
    pub label: String,
    pub value: u64,
    /// A line on what the number is made of, such as how many are published.
    pub note: Option<String>,
}

impl Figure {
    pub fn new(label: impl Into<String>, value: u64) -> Self {
        Figure {
            label: label.into(),
            value,
            note: None,
        }
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// One part of a breakdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub label: String,
    pub value: u64,
}

impl Block {
    /// A breakdown of `bars`, largest first and without the empty ones, which
    /// only say what a source could hold and does not.
    pub fn breakdown(title: impl Into<String>, bars: impl IntoIterator<Item = Bar>) -> Self {
        let mut bars: Vec<Bar> = bars.into_iter().filter(|bar| bar.value > 0).collect();
        bars.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));
        Block::Breakdown {
            title: title.into(),
            bars,
        }
    }

    /// Whether there is nothing in it to show.
    pub fn is_empty(&self) -> bool {
        match self {
            Block::Figures(figures) => figures.is_empty(),
            Block::Breakdown { bars, .. } => bars.is_empty(),
            Block::Datasets { entries, .. } => entries.is_empty(),
        }
    }
}

impl Dashboard {
    /// Every dataset it offers, for naming one once it is open.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.blocks.iter().flat_map(|block| match block {
            Block::Datasets { entries, .. } => entries.as_slice(),
            _ => &[],
        })
    }
}

/// How a source's dashboard is coming along.
#[derive(Debug, Default)]
pub enum DashboardState {
    /// Not asked yet, or asked to be asked again.
    #[default]
    Waiting,
    Loading,
    Ready(Dashboard),
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(label: &str, value: u64) -> Bar {
        Bar {
            label: label.into(),
            value,
        }
    }

    #[test]
    fn a_breakdown_leads_with_its_largest_and_drops_what_is_empty() {
        let Block::Breakdown { bars, .. } = Block::breakdown(
            "By species",
            [
                bar("marmoset", 5),
                bar("none", 0),
                bar("mouse", 5204),
                bar("human", 297),
            ],
        ) else {
            unreachable!()
        };
        let labels: Vec<&str> = bars.iter().map(|bar| bar.label.as_str()).collect();
        assert_eq!(labels, ["mouse", "human", "marmoset"]);
    }
}
