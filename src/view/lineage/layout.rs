//! Where each record of a lineage is drawn: a column for each step before or
//! after the record it is drawn from, and each record beside the one it was
//! reached from.

use std::collections::HashMap;

use bevy::math::{Rect, Vec2};

use crate::source::lineage::{LineageAsked, SourceLineage};
use crate::widgets::space;

pub(super) const CARD_W: f32 = 232.0;
pub(super) const CARD_H: f32 = 76.0;
const COLUMN_GAP: f32 = space::GRAPH_COLUMNS;
const ROW_GAP: f32 = space::GRAPH_CARDS;
const MARGIN: f32 = space::PANEL_INSET;
const LINE_PX: f32 = 1.5;

/// Records one shows on one side before the rest are folded into a card
/// saying how many: an ingest puts out twenty data assets, and a specimen can
/// go into a hundred archiving runs.
const FOLD_ABOVE: usize = 8;
/// How many are still shown when they are folded.
const FOLD_KEEP: usize = 5;
/// The most shown when they are asked for whole. An alignment's output went
/// into thirteen thousand runs, and a card each would be more than a frame
/// can build, let alone read.
const UNFOLD_MAX: usize = 60;

/// What a card stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shows {
    /// A record, by its place in the lineage.
    Node(usize),
    /// The records reached from `parent` on one side that are not shown,
    /// and whether they can be asked for.
    Fold {
        parent: usize,
        hidden: usize,
        more: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Card {
    pub shows: Shows,
    pub rect: Rect,
}

/// A lineage laid out: its cards, the lines between them, and how much room
/// the lot takes.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Laid {
    pub cards: Vec<Card>,
    pub lines: Vec<Rect>,
    pub size: Vec2,
}

/// A card before it has a place: what it shows, its column, and the card it
/// hangs off.
struct Placing {
    shows: Shows,
    depth: i32,
    parent: Option<usize>,
    level: usize,
}

pub(super) fn lay_out(lineage: &SourceLineage, asked: &LineageAsked) -> Laid {
    if lineage.nodes().is_empty() {
        return Laid::default();
    }
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); lineage.nodes().len()];
    for (at, node) in lineage.nodes().iter().enumerate() {
        if let Some(parent) = node.parent {
            children[parent].push(at);
        }
    }

    // Which cards there are, each after the one it hangs off.
    let mut placing = vec![Placing {
        shows: Shows::Node(0),
        depth: lineage.nodes()[0].depth,
        parent: None,
        level: 0,
    }];
    let mut next = 0;
    while next < placing.len() {
        let (card, level) = (next, placing[next].level);
        next += 1;
        let Shows::Node(at) = placing[card].shows else {
            continue;
        };
        let node = &lineage.nodes()[at];
        if !asked.expanded.contains(&node.id) {
            continue;
        }
        let unfolded = asked.unfolded.contains(&node.id);
        for before in [true, false] {
            let side: Vec<usize> = children[at]
                .iter()
                .copied()
                .filter(|&child| (lineage.nodes()[child].depth < node.depth) == before)
                .collect();
            let shown = match side.len() {
                all if unfolded => all.min(UNFOLD_MAX),
                all if all > FOLD_ABOVE => FOLD_KEEP,
                all => all,
            };
            let folds = shown < side.len();
            for &child in &side[..shown] {
                placing.push(Placing {
                    shows: Shows::Node(child),
                    depth: lineage.nodes()[child].depth,
                    parent: Some(card),
                    level: level + 1,
                });
            }
            if folds {
                placing.push(Placing {
                    shows: Shows::Fold {
                        parent: at,
                        hidden: side.len() - shown,
                        more: !unfolded,
                    },
                    depth: node.depth + if before { -1 } else { 1 },
                    parent: Some(card),
                    level: level + 1,
                });
            }
        }
    }

    // Down the page: each card's siblings centered on it, level by level
    // out from the record the lineage is drawn from, never over a card
    // already in that column.
    let pitch = CARD_H + ROW_GAP;
    let mut y = vec![0.0f32; placing.len()];
    let mut floor: HashMap<i32, f32> = HashMap::new();
    floor.insert(placing[0].depth, pitch);
    let deepest = placing.iter().map(|it| it.level).max().unwrap_or_default();
    for level in 1..=deepest {
        let mut groups: Vec<(usize, i32, Vec<usize>)> = Vec::new();
        for (card, it) in placing.iter().enumerate() {
            if it.level != level {
                continue;
            }
            let parent = it.parent.unwrap_or_default();
            match groups
                .iter_mut()
                .find(|(of, depth, _)| *of == parent && *depth == it.depth)
            {
                Some((_, _, cards)) => cards.push(card),
                None => groups.push((parent, it.depth, vec![card])),
            }
        }
        groups.sort_by(|a, b| y[a.0].total_cmp(&y[b.0]));
        for (parent, depth, cards) in groups {
            let wanted = y[parent] - (cards.len() - 1) as f32 * pitch / 2.0;
            let column = floor.entry(depth).or_insert(f32::MIN);
            let mut at = wanted.max(*column);
            for card in cards {
                y[card] = at;
                at += pitch;
            }
            *column = at;
        }
    }

    let top = y.iter().copied().fold(f32::MAX, f32::min);
    let first = placing.iter().map(|it| it.depth).min().unwrap_or_default();
    let cards: Vec<Card> = placing
        .iter()
        .zip(&y)
        .map(|(it, y)| {
            let min = Vec2::new(
                MARGIN + (it.depth - first) as f32 * (CARD_W + COLUMN_GAP),
                MARGIN + y - top,
            );
            Card {
                shows: it.shows,
                rect: Rect::from_corners(min, min + Vec2::new(CARD_W, CARD_H)),
            }
        })
        .collect();

    let mut lines = Vec::new();
    let card_of = |at: usize| cards.iter().position(|card| card.shows == Shows::Node(at));
    for &(from, to) in lineage.edges() {
        if let (Some(from), Some(to)) = (card_of(from), card_of(to)) {
            elbow(&mut lines, cards[from].rect, cards[to].rect);
        }
    }
    for (card, it) in placing.iter().enumerate() {
        if let (Shows::Fold { .. }, Some(parent)) = (it.shows, it.parent) {
            let (from, to) = if it.depth < placing[parent].depth {
                (card, parent)
            } else {
                (parent, card)
            };
            elbow(&mut lines, cards[from].rect, cards[to].rect);
        }
    }

    let size = cards
        .iter()
        .fold(Vec2::ZERO, |size, card| size.max(card.rect.max))
        + Vec2::splat(MARGIN);
    Laid { cards, lines, size }
}

/// A line out of the right of `from` and into the left of `to`, turning
/// halfway across the gap before `to`. Nothing is drawn back the other way:
/// a record is never made from something drawn after it.
fn elbow(lines: &mut Vec<Rect>, from: Rect, to: Rect) {
    let start = Vec2::new(from.max.x, from.center().y);
    let end = Vec2::new(to.min.x, to.center().y);
    if start.x >= end.x {
        return;
    }
    let turn = end.x - COLUMN_GAP / 2.0;
    let half = LINE_PX / 2.0;
    lines.push(Rect::new(
        start.x,
        start.y - half,
        turn + half,
        start.y + half,
    ));
    lines.push(Rect::new(
        turn - half,
        start.y.min(end.y) - half,
        turn + half,
        start.y.max(end.y) + half,
    ));
    lines.push(Rect::new(turn - half, end.y - half, end.x, end.y + half));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::lineage::{LineageNode, Links};

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

    fn asked(expanded: &[&str]) -> LineageAsked {
        LineageAsked {
            expanded: expanded.iter().map(ToString::to_string).collect(),
            unfolded: Default::default(),
        }
    }

    /// A process with `inputs` taken in and `outputs` put out.
    fn process(inputs: usize, outputs: usize) -> SourceLineage {
        let mut lineage = SourceLineage::new(node("p"));
        for n in 0..inputs {
            lineage.link(0, node(&format!("in{n}")), true);
        }
        for n in 0..outputs {
            lineage.link(0, node(&format!("out{n}")), false);
        }
        lineage
    }

    fn column_of(laid: &Laid, at: usize) -> f32 {
        laid.cards
            .iter()
            .find(|card| card.shows == Shows::Node(at))
            .unwrap()
            .rect
            .min
            .x
    }

    #[test]
    fn inputs_are_drawn_before_a_process_and_outputs_after() {
        let lineage = process(1, 2);
        let laid = lay_out(&lineage, &asked(&["p"]));
        assert_eq!(laid.cards.len(), 4);
        assert!(column_of(&laid, 1) < column_of(&laid, 0));
        assert!(column_of(&laid, 0) < column_of(&laid, 2));
        assert_eq!(column_of(&laid, 2), column_of(&laid, 3));
        // Three turns a link.
        assert_eq!(laid.lines.len(), 9);
    }

    #[test]
    fn a_record_whose_links_are_not_wanted_shows_none_of_them() {
        let lineage = process(1, 2);
        let laid = lay_out(&lineage, &asked(&[]));
        assert_eq!(laid.cards.len(), 1);
        assert!(laid.lines.is_empty());
    }

    #[test]
    fn a_crowd_is_folded_until_it_is_asked_for_whole() {
        let lineage = process(0, 20);
        let laid = lay_out(&lineage, &asked(&["p"]));
        assert_eq!(laid.cards.len(), 1 + FOLD_KEEP + 1);
        assert_eq!(
            laid.cards.last().unwrap().shows,
            Shows::Fold {
                parent: 0,
                hidden: 20 - FOLD_KEEP,
                more: true,
            }
        );
        let mut whole = asked(&["p"]);
        whole.unfolded.insert("p".into());
        assert_eq!(lay_out(&lineage, &whole).cards.len(), 21);
        // Past what a frame can build, the rest are only counted.
        let crowd = process(0, 1000);
        let laid = lay_out(&crowd, &whole);
        assert_eq!(laid.cards.len(), 1 + UNFOLD_MAX + 1);
        assert_eq!(
            laid.cards.last().unwrap().shows,
            Shows::Fold {
                parent: 0,
                hidden: 1000 - UNFOLD_MAX,
                more: false,
            }
        );
    }

    #[test]
    fn no_two_cards_overlap() {
        let mut lineage = process(3, 3);
        // Each output made into more.
        for out in 4..7 {
            for n in 0..3 {
                lineage.link(out, node(&format!("{out}-{n}")), false);
            }
        }
        let expanded: Vec<String> = lineage.nodes().iter().map(|it| it.id.clone()).collect();
        let expanded: Vec<&str> = expanded.iter().map(String::as_str).collect();
        let laid = lay_out(&lineage, &asked(&expanded));
        assert_eq!(laid.cards.len(), lineage.nodes().len());
        for (i, a) in laid.cards.iter().enumerate() {
            assert!(a.rect.min.cmpge(Vec2::ZERO).all());
            assert!(a.rect.max.cmple(laid.size).all());
            for b in &laid.cards[i + 1..] {
                assert!(a.rect.intersect(b.rect).is_empty(), "{a:?} over {b:?}");
            }
        }
    }
}
