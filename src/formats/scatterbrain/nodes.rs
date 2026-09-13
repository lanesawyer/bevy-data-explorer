//! Fetching, decoding and meshing one octree node.
//!
//! Shared by both Scatterbrain plugins: a single cloud and a sectioned dataset
//! differ in how they choose and lay out nodes, not in what a node is or how
//! its bytes become vertices. This lived in `pointcloud` and was imported from
//! `slices`, which made one of the two consumers look like the owner.

use bevy::prelude::*;

use crate::render::points::build_point_mesh;
use crate::source::properties::CellSelection;

use super::{Node, Rect, Scatterbrain, decode_categories, decode_floats, decode_positions};

/// How close the pointer has to come to a point to pick it, in logical pixels.
///
/// Points draw at about a pixel and a half, so picking has to reach further
/// than a point is wide or nothing would ever be hit.
pub const PICK_PX: f32 = 7.0;

/// A resident node's points, kept on the CPU after its mesh is built.
///
/// The pointer has to be resolved against actual coordinates and a mesh cannot
/// be read back, so this is what makes hovering possible at all. Ten bytes a
/// point against the eighty each already costs on the GPU.
pub struct NodePoints {
    pub positions: Vec<[f32; 2]>,
    pub categories: Vec<u16>,
}

impl NodePoints {
    /// The point nearest `target` within `limit` squared units, as an offset
    /// into this node and its value in the coloured-by column.
    ///
    /// Shared with the sectioned streamer, which differs only in having to move
    /// the target into each slide's own coordinates first.
    pub fn nearest(&self, target: Vec2, limit: f32) -> Option<(f32, usize, Vec2, Option<u16>)> {
        let mut best: Option<(f32, usize, Vec2, Option<u16>)> = None;
        for (offset, point) in self.positions.iter().enumerate() {
            let point = Vec2::from(*point);
            let distance = (point - target).length_squared();
            if distance > limit {
                continue;
            }
            if best.is_none_or(|(nearest, ..)| distance < nearest) {
                best = Some((
                    distance,
                    offset,
                    point,
                    self.categories.get(offset).copied(),
                ));
            }
        }
        best
    }
}

/// The rectangle a probe can reach, in dataset coordinates.
pub fn pick_reach(target: Vec2, radius: f32) -> Rect {
    Rect {
        min_x: target.x - radius,
        min_y: target.y - radius,
        max_x: target.x + radius,
        max_y: target.y + radius,
    }
}

pub enum NodeOutcome {
    Ready(Vec<[f32; 2]>, Vec<u16>),
    Failed(String),
}

/// Fetch a node's points, the column they are coloured by, and any columns the
/// filters restrict.
///
/// Filtered-out points are dropped here rather than hidden later, so they cost
/// no vertices and no budget.
pub fn load_node(
    cloud: &Scatterbrain,
    node: &Node,
    selection: &CellSelection,
) -> Result<(Vec<[f32; 2]>, Vec<u16>), String> {
    let mut positions = decode_positions(&fetch(&cloud.positions_url(node))?, node.count)?;

    let mut categories = match &selection.colour_by {
        Some(column) => {
            let bytes = fetch(&cloud.column_url(column, node))?;
            decode_categories(&bytes, node.count)?
        }
        None => Vec::new(),
    };

    if selection.filters.is_empty() {
        return Ok((positions, categories));
    }

    // A column is decoded to whatever its restriction compares against: codes
    // for a categorical filter, floats for a numeric one.
    let mut columns: Vec<Vec<f32>> = Vec::with_capacity(selection.filters.len());
    for (column, restriction) in &selection.filters {
        let bytes = fetch(&cloud.column_url(column, node))?;
        let values = if restriction.is_numeric() {
            decode_floats(&bytes, node.count)?
        } else {
            decode_categories(&bytes, node.count)?
                .into_iter()
                .map(f32::from)
                .collect()
        };
        columns.push(values);
    }

    let mut values = vec![0.0f32; columns.len()];
    let mut keep = Vec::with_capacity(positions.len());
    for index in 0..positions.len() {
        for (slot, column) in values.iter_mut().zip(&columns) {
            *slot = column.get(index).copied().unwrap_or_default();
        }
        keep.push(selection.admits(&values));
    }

    let mut index = 0;
    positions.retain(|_| {
        index += 1;
        keep[index - 1]
    });
    if !categories.is_empty() {
        let mut index = 0;
        categories.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }
    Ok((positions, categories))
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = reqwest::blocking::get(url)
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("fetching {url}: {e}"))?;
    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| format!("reading {url}: {e}"))
}

/// Build a point-list mesh, colouring each point by its category.
///
/// One vertex per point keeps a multi-million point cloud affordable; the
/// trade-off is that the hardware draws each as a single pixel, so there is no
/// point-size control without a custom shader.
pub fn build_mesh(positions: &[[f32; 2]], categories: &[u16]) -> Mesh {
    let points: Vec<Vec2> = positions.iter().map(|p| Vec2::new(p[0], p[1])).collect();

    let coloured = categories.len() == positions.len();
    let colours: Vec<[f32; 4]> = if coloured {
        categories.iter().map(|c| category_colour(*c)).collect()
    } else {
        vec![[0.8, 0.85, 0.9, 1.0]; positions.len()]
    };

    // Each vertex carries its point's category so that hovering one can enlarge
    // the rest sharing it. Without a colour-by column there are no groups to
    // pick out, and the mesh says so by carrying none.
    build_point_mesh(&points, &colours, if coloured { categories } else { &[] })
}

/// A repeating categorical palette. Categories here are label indices with no
/// inherent order, so hues are spread by a golden-ratio step to keep
/// neighbouring indices visually distinct.
pub fn category_colour(category: u16) -> [f32; 4] {
    let hue = (category as f32 * 137.507_76) % 360.0;
    let colour = Color::hsl(hue, 0.72, 0.62).to_linear();
    [colour.red, colour.green, colour.blue, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_get_distinguishable_colours() {
        // Adjacent label indices are unrelated, so they must not look alike.
        let a = category_colour(0);
        let b = category_colour(1);
        let distance: f32 = (0..3).map(|i| (a[i] - b[i]).abs()).sum();
        assert!(distance > 0.2, "neighbouring categories look too similar");
        assert_eq!(a[3], 1.0);
    }
}
