//! The Allen Institute's Scatterbrain point-cloud format.
//!
//! A Scatterbrain dataset is a Potree-style octree of 2D points. Each node
//! holds its own subsample of the region it covers and its children add
//! further points, so drawing a node *and* its descendants is how detail
//! accumulates — the node counts in the tree sum to the dataset total rather
//! than each level restating the whole cloud.
//!
//! Columns are stored one per directory, split by node:
//! `{metadata}/{column}/{referenceId}/{node}.bin`. Coordinates are raw
//! little-endian `f32` pairs with no header; the categorical columns are raw
//! `u16`. A file's length is therefore exactly the node's point count times the
//! column's stride, which is the cheapest possible integrity check.

use serde::Deserialize;

/// Axis-aligned rectangle in dataset coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Rect {
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    pub fn centre(&self) -> (f32, f32) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        )
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct BoundingBox {
    lx: f32,
    ly: f32,
    ux: f32,
    uy: f32,
}

impl From<BoundingBox> for Rect {
    fn from(b: BoundingBox) -> Self {
        Rect {
            min_x: b.lx,
            min_y: b.ly,
            max_x: b.ux,
            max_y: b.uy,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PointAttribute {
    pub description: String,
    pub name: String,
    #[serde(default)]
    pub elements: usize,
    #[serde(default)]
    pub size: usize,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
struct RawNode {
    file: String,
    #[serde(rename = "numSpecimens")]
    num_specimens: u64,
    #[serde(default)]
    children: Vec<RawNode>,
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    #[serde(rename = "boundingBox")]
    bounding_box: BoundingBox,
    #[serde(rename = "tightBoundingBox")]
    tight_bounding_box: Option<BoundingBox>,
    #[serde(rename = "metadataFileEndpoint")]
    metadata_file_endpoint: String,
    #[serde(rename = "pointAttributes")]
    point_attributes: Vec<PointAttribute>,
    points: u64,
    root: RawNode,
    #[serde(rename = "spatialColumn")]
    spatial_column: String,
    #[serde(rename = "visualizationReferenceId")]
    visualization_reference_id: String,
}

/// One octree node, flattened into an arena.
#[derive(Debug, Clone)]
pub struct Node {
    /// Node name, e.g. `r0200402`. The digits after `r` are the path of child
    /// indices from the root.
    pub name: String,
    pub file: String,
    pub count: u64,
    pub depth: usize,
    /// Region this node covers, subdivided from the root box.
    pub bounds: Rect,
    pub children: Vec<usize>,
}

#[derive(Debug)]
pub struct Scatterbrain {
    pub nodes: Vec<Node>,
    pub bounds: Rect,
    /// Bounds of the points themselves, which may be tighter than the octree
    /// cube; used for framing the initial view.
    pub tight_bounds: Rect,
    pub total_points: u64,
    pub attributes: Vec<PointAttribute>,
    metadata_endpoint: String,
    reference_id: String,
    spatial_column: String,
}

impl Scatterbrain {
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawMetadata = serde_json::from_str(text)
            .map_err(|e| format!("parsing Scatterbrain metadata: {e}"))?;

        let bounds: Rect = raw.bounding_box.into();
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return Err("bounding box is empty".into());
        }

        // The reader assumes the spatial column is a pair of f32s, which is
        // what makes a file length of `count * 8` a valid integrity check.
        // Verify that against the declared attribute rather than trusting it.
        let spatial = raw
            .point_attributes
            .iter()
            .find(|a| a.name == raw.spatial_column)
            .ok_or_else(|| {
                format!(
                    "spatial column `{}` is not in pointAttributes",
                    raw.spatial_column
                )
            })?;
        if spatial.kind != "float" || spatial.elements != 2 || spatial.size != 8 {
            return Err(format!(
                "spatial column `{}` is {} x {} ({} bytes); only 2 x float32 is supported",
                spatial.name, spatial.elements, spatial.kind, spatial.size
            ));
        }

        let mut nodes = Vec::new();
        flatten(&raw.root, bounds, 0, &mut nodes)?;

        Ok(Scatterbrain {
            tight_bounds: raw.tight_bounding_box.map(Rect::from).unwrap_or(bounds),
            bounds,
            nodes,
            total_points: raw.points,
            attributes: raw.point_attributes,
            metadata_endpoint: ensure_slash(raw.metadata_file_endpoint),
            reference_id: raw.visualization_reference_id,
            spatial_column: raw.spatial_column,
        })
    }

    pub fn root(&self) -> &Node {
        &self.nodes[0]
    }

    /// URL of a column's data for one node.
    pub fn column_url(&self, column: &str, node: &Node) -> String {
        format!(
            "{}{}/{}/{}",
            self.metadata_endpoint, column, self.reference_id, node.file
        )
    }

    pub fn positions_url(&self, node: &Node) -> String {
        self.column_url(&self.spatial_column, node)
    }

    /// Categorical columns available for colouring, i.e. everything but the
    /// coordinates.
    pub fn category_columns(&self) -> Vec<&PointAttribute> {
        self.attributes
            .iter()
            .filter(|a| a.name != self.spatial_column && a.kind == "uint16")
            .collect()
    }
}

fn ensure_slash(mut url: String) -> String {
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

/// Walk the nested metadata into an arena, subdividing bounds on the way down.
fn flatten(
    raw: &RawNode,
    bounds: Rect,
    depth: usize,
    out: &mut Vec<Node>,
) -> Result<usize, String> {
    let name = raw
        .file
        .strip_suffix(".bin")
        .unwrap_or(&raw.file)
        .to_string();

    let index = out.len();
    out.push(Node {
        name: name.clone(),
        file: raw.file.clone(),
        count: raw.num_specimens,
        depth,
        bounds,
        children: Vec::new(),
    });

    let mut children = Vec::with_capacity(raw.children.len());
    for child in &raw.children {
        let child_name = child.file.strip_suffix(".bin").unwrap_or(&child.file);
        let digit = child_name
            .chars()
            .last()
            .and_then(|c| c.to_digit(10))
            .ok_or_else(|| format!("node `{child_name}` has no child index"))?;
        children.push(flatten(
            child,
            child_bounds(bounds, digit as u8),
            depth + 1,
            out,
        )?);
    }
    out[index].children = children;
    Ok(index)
}

/// Subdivide a node's box for the given octree child index.
///
/// The index packs one bit per axis, `x` in bit 2 and `y` in bit 1, with `z` in
/// bit 0. This data is planar, so the z bit is always clear and only the even
/// indices 0, 2, 4 and 6 ever appear — which is why the node names look like
/// they skip numbers.
pub fn child_bounds(bounds: Rect, index: u8) -> Rect {
    let (cx, cy) = bounds.centre();
    let (min_x, max_x) = if index & 0b100 != 0 {
        (cx, bounds.max_x)
    } else {
        (bounds.min_x, cx)
    };
    let (min_y, max_y) = if index & 0b010 != 0 {
        (cy, bounds.max_y)
    } else {
        (bounds.min_y, cy)
    };
    Rect {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

/// Decode a coordinates file: little-endian `f32` pairs, no header.
pub fn decode_positions(bytes: &[u8], expected: u64) -> Result<Vec<[f32; 2]>, String> {
    if bytes.len() % 8 != 0 {
        return Err(format!(
            "coordinate file is {} bytes, not a whole number of xy pairs",
            bytes.len()
        ));
    }
    let count = bytes.len() / 8;
    if count as u64 != expected {
        return Err(format!(
            "coordinate file holds {count} points, but the tree declares {expected}"
        ));
    }
    Ok(bytes
        .chunks_exact(8)
        .map(|c| {
            [
                f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            ]
        })
        .collect())
}

/// Decode a categorical column: little-endian `u16`, one per point.
pub fn decode_categories(bytes: &[u8], expected: u64) -> Result<Vec<u16>, String> {
    if bytes.len() % 2 != 0 || (bytes.len() / 2) as u64 != expected {
        return Err(format!(
            "category file holds {} values, but the tree declares {expected}",
            bytes.len() / 2
        ));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> Scatterbrain {
        Scatterbrain::parse(include_str!("../testdata/scatterbrain.json")).unwrap()
    }

    #[test]
    fn flattens_the_whole_tree() {
        let sb = reference();
        assert_eq!(sb.nodes.len(), 135);
        assert_eq!(sb.root().name, "r");
        assert_eq!(sb.root().count, 134065);
        assert_eq!(sb.total_points, 4_042_976);
    }

    #[test]
    fn node_counts_are_additive_across_the_tree() {
        // Each node stores its own subsample rather than restating its
        // children, which is why rendering must draw a node *and* its
        // descendants to gain detail.
        let sb = reference();
        let summed: u64 = sb.nodes.iter().map(|n| n.count).sum();
        assert_eq!(summed, sb.total_points);
    }

    #[test]
    fn the_root_box_is_a_cube() {
        let sb = reference();
        assert!((sb.bounds.width() - sb.bounds.height()).abs() < 1e-4);
    }

    #[test]
    fn child_indices_follow_the_potree_bit_layout() {
        // Verified against the real store: r0 is the lower-left quadrant, r4
        // lower-right, r2 upper-left, r6 upper-right.
        let b = Rect {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 8.0,
            max_y: 8.0,
        };
        assert_eq!(
            child_bounds(b, 0),
            Rect {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 4.0,
                max_y: 4.0
            }
        );
        assert_eq!(
            child_bounds(b, 2),
            Rect {
                min_x: 0.0,
                min_y: 4.0,
                max_x: 4.0,
                max_y: 8.0
            }
        );
        assert_eq!(
            child_bounds(b, 4),
            Rect {
                min_x: 4.0,
                min_y: 0.0,
                max_x: 8.0,
                max_y: 4.0
            }
        );
        assert_eq!(
            child_bounds(b, 6),
            Rect {
                min_x: 4.0,
                min_y: 4.0,
                max_x: 8.0,
                max_y: 8.0
            }
        );
    }

    #[test]
    fn nested_node_bounds_match_their_name_path() {
        let sb = reference();
        // r0200402 is eight levels down; walking its digits must land inside
        // the root box and stay inside each ancestor.
        let deep = sb.nodes.iter().find(|n| n.name == "r0200402").unwrap();
        assert_eq!(deep.depth, 7);
        assert!(deep.bounds.intersects(&sb.bounds));
        assert!(deep.bounds.width() < sb.bounds.width() / 64.0);

        let parent = sb.nodes.iter().find(|n| n.name == "r020040").unwrap();
        assert!(deep.bounds.min_x >= parent.bounds.min_x - 1e-4);
        assert!(deep.bounds.max_x <= parent.bounds.max_x + 1e-4);
    }

    #[test]
    fn builds_column_urls_in_the_layout_the_store_uses() {
        let sb = reference();
        assert_eq!(
            sb.positions_url(sb.root()),
            "https://d2o7sc91n904vd.cloudfront.net/wmb_tenx_01172024_stage-20240128193624/G4I4GFJXJB9ATZ3PTX1/metadata/G4I4GFJXJB9ATZ3PTX1Coordinates/G4I4GFJXJB9ATZ3PTX1/r.bin"
        );
    }

    #[test]
    fn categorical_columns_exclude_the_coordinates() {
        let sb = reference();
        let columns = sb.category_columns();
        assert_eq!(columns.len(), 10);
        assert_eq!(columns[0].description, "Class");
        assert!(columns.iter().all(|c| c.kind == "uint16"));
    }

    #[test]
    fn rejects_a_spatial_column_that_is_not_an_f32_pair() {
        let text = include_str!("../testdata/scatterbrain.json").replace(
            r#""elements":2,"name":"G4I4GFJXJB9ATZ3PTX1Coordinates","size":8"#,
            r#""elements":3,"name":"G4I4GFJXJB9ATZ3PTX1Coordinates","size":12"#,
        );
        let error = Scatterbrain::parse(&text).unwrap_err();
        assert!(error.contains("only 2 x float32"), "got: {error}");
    }

    #[test]
    fn rejects_metadata_whose_spatial_column_is_missing() {
        let text = include_str!("../testdata/scatterbrain.json").replace(
            r#""spatialColumn":"G4I4GFJXJB9ATZ3PTX1Coordinates""#,
            r#""spatialColumn":"NoSuchColumn""#,
        );
        assert!(Scatterbrain::parse(&text).is_err());
    }

    #[test]
    fn decodes_positions_and_rejects_a_truncated_file() {
        let bytes = 1.0f32
            .to_le_bytes()
            .into_iter()
            .chain(2.0f32.to_le_bytes())
            .collect::<Vec<u8>>();
        assert_eq!(decode_positions(&bytes, 1).unwrap(), vec![[1.0, 2.0]]);
        // A count that disagrees with the tree means we would render garbage.
        assert!(decode_positions(&bytes, 2).is_err());
        assert!(decode_positions(&bytes[..7], 1).is_err());
    }

    #[test]
    fn decodes_categories() {
        let bytes = [1u8, 0, 33, 0];
        assert_eq!(decode_categories(&bytes, 2).unwrap(), vec![1, 33]);
        assert!(decode_categories(&bytes, 3).is_err());
    }
}
