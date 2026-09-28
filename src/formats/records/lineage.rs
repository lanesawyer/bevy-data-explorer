//! Where one of the registry's records came from and what came of it: the
//! processes it went into and came out of, what those took in and put out,
//! and so on as far as it is followed.
//!
//! Addressed as the endpoint with the record on the query string, named by
//! its kind and id:
//!
//! ```text
//! https://stage-bkpr.brain.devlims.org/graphql/?lineage=processes:8c3f…
//! ```
//!
//! Opened with the record's own links read; any other record's are read
//! when the frame asks for them, one record to a request, as a picked row's
//! are. Most of the registry is one step deep — two million of its processes
//! archive a specimen into one directory — but its cell omics runs chain four
//! processes long, and one run can put out twenty data assets.

use serde_json::json;

use super::query::ask_related;
use super::related::{asset_of, process_of};
use super::*;
use crate::source::lineage::{LineageAsked, LineageNode, Links, SourceLineage};
use crate::source::{self, SourceExtent, SourceStatus, SourceUrl};

/// The query parameter naming the record a lineage is drawn from.
const PARAMETER: &str = "lineage";

/// The address of the lineage of the record of `kind` known as `id`.
pub fn address(endpoint: &str, kind: Kind, id: &str) -> String {
    format!("{endpoint}?{PARAMETER}={}", related::link_to(kind, id))
}

/// Whether `url` asks for a record's lineage, and if so the endpoint to ask
/// and the record.
pub fn lineage_of(url: &str) -> Option<(String, Kind, String)> {
    let (endpoint, query) = url.split_once('?')?;
    let link = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == PARAMETER)
        .map(|(_, value)| value)?;
    let (kind, id) = related::kind_and_id(link)?;
    Some((endpoint.to_string(), kind, id))
}

/// A record's lineage as far as its own links, read.
pub struct Lineage {
    endpoint: String,
    pub name: String,
    lineage: SourceLineage,
}

/// Read the record at `id` and what it links to.
pub async fn read(endpoint: &str, kind: Kind, id: &str) -> Result<Lineage, String> {
    let node = ask_related(endpoint, kind, id).await?;
    let root = root_of(kind, id, &node);
    let name = root.name.clone();
    let mut lineage = SourceLineage::new(root);
    add_links(&mut lineage, 0, kind, &node);
    Ok(Lineage {
        endpoint: endpoint.to_string(),
        name,
        lineage,
    })
}

/// The record a lineage is drawn from, out of its own fields.
fn root_of(kind: Kind, id: &str, node: &Value) -> LineageNode {
    let linked = match kind {
        Kind::Processes => Some(process_of(node)),
        Kind::Specimens => asset_of(&json!({ "specimen": node })),
        Kind::DataAssets => asset_of(&json!({ "dataAsset": node })),
    };
    let linked = linked.unwrap_or_else(|| RelatedRecord {
        name: id.to_string(),
        detail: String::new(),
        address: None,
        link: None,
    });
    LineageNode {
        id: related::link_to(kind, id),
        links: Links::Read,
        ..node_of(linked, kind == Kind::Processes)
    }
}

/// A linked record as a node, not yet placed.
fn node_of(record: RelatedRecord, step: bool) -> LineageNode {
    let (id, links) = match record.link {
        Some(link) => (link, Links::Unread),
        // A subject: nothing to read further, and known only by its name.
        None => (format!("subject:{}", record.name), Links::None),
    };
    LineageNode {
        id,
        name: record.name,
        detail: record.detail,
        step,
        address: record.address,
        depth: 0,
        parent: None,
        links,
    }
}

/// Add what the record at `at`, of `kind`, links to according to `node`.
fn add_links(lineage: &mut SourceLineage, at: usize, kind: Kind, node: &Value) {
    let list = |field: &str| {
        node.get(field)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let (before, after, step) = match kind {
        Kind::Processes => ("inputs", "outputs", false),
        Kind::Specimens | Kind::DataAssets => ("outputOf", "inputTo", true),
    };
    for (field, earlier) in [(before, true), (after, false)] {
        for linked in list(field) {
            let record = if step {
                Some(process_of(&linked))
            } else {
                asset_of(&linked)
            };
            if let Some(record) = record {
                lineage.link(at, node_of(record, step), earlier);
            }
        }
    }
    lineage.node_mut(at).links = Links::Read;
}

/// A lineage's records whose links are being read, and the reads.
#[derive(Component)]
pub struct LineageReads {
    endpoint: String,
    reading: Vec<(String, Kind, Fetching<Result<Value, String>>)>,
}

pub fn spawn_source(world: &mut World, lineage: Lineage) -> Entity {
    let Lineage {
        endpoint,
        name,
        lineage,
    } = lineage;
    let source = source::register_in(
        world,
        source::SourceInfo {
            name: format!("Lineage of {name}"),
            // Records rather than a place, as a table's rows are.
            unit: String::new(),
            detail: "BKP Registry lineage".into(),
            stat: String::new(),
            category: source::Category::Table,
        },
        SourceExtent {
            center: Vec2::ZERO,
            size: Vec2::ONE,
            finest: 1.0,
        },
    );
    let root = lineage.nodes()[0].id.clone();
    world.entity_mut(source).insert((
        SourceStatus(status_of(&lineage, 0)),
        LineageAsked {
            expanded: [root].into(),
            unfolded: Default::default(),
        },
        lineage,
        LineageReads {
            endpoint,
            reading: Vec::new(),
        },
    ));
    source
}

fn status_of(lineage: &SourceLineage, reading: usize) -> String {
    let steps = lineage.nodes().iter().filter(|node| node.step).count();
    let mut status = format!(
        "{} records, {steps} of them processes",
        lineage.nodes().len()
    );
    if reading > 0 {
        status.push_str(&format!("\nReading the links of {reading}\u{2026}"));
    }
    status
}

/// Read the links of every record the frame asked for, and add them as they
/// land.
pub(super) fn serve_lineage(
    mut sources: Query<(
        &mut SourceLineage,
        &LineageAsked,
        &mut LineageReads,
        &mut SourceStatus,
        &mut SourceBusy,
        Option<&SourceUrl>,
    )>,
) {
    for (mut lineage, asked, mut reads, mut status, mut busy, url) in &mut sources {
        let wanted: Vec<usize> = lineage
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| node.links == Links::Unread && asked.expanded.contains(&node.id))
            .map(|(at, _)| at)
            .collect();
        for at in wanted {
            let id = lineage.nodes()[at].id.clone();
            let Some((kind, record)) = related::kind_and_id(&id) else {
                lineage.node_mut(at).links = Links::None;
                continue;
            };
            lineage.node_mut(at).links = Links::Fetching;
            let endpoint = reads.endpoint.clone();
            let read = fetching(async move { ask_related(&endpoint, kind, &record).await });
            reads.reading.push((id, kind, read));
        }
        let mut landed = Vec::new();
        reads
            .reading
            .retain_mut(|(id, kind, read)| match read.take() {
                Some(answer) => {
                    landed.push((id.clone(), *kind, answer));
                    false
                }
                None => true,
            });
        for (id, kind, answer) in landed {
            let Some(at) = lineage.find(&id) else {
                continue;
            };
            match answer {
                Ok(node) => add_links(&mut lineage, at, kind, &node),
                Err(e) => {
                    warn!(
                        "reading the lineage at {}: {e}",
                        url.map_or("the BKP Registry", |url| &url.0)
                    );
                    lineage.node_mut(at).links = Links::Failed(e);
                }
            }
        }
        let text = status_of(&lineage, reads.reading.len());
        if status.0 != text {
            status.0 = text;
        }
        busy.set_if_neq(SourceBusy(!reads.reading.is_empty()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::table::TableFilterTerm;

    #[test]
    fn a_lineage_is_addressed_by_its_record() {
        let url = address(STAGE, Kind::Processes, "8c3f");
        assert_eq!(
            url,
            "https://stage-bkpr.brain.devlims.org/graphql/?lineage=processes:8c3f"
        );
        assert_eq!(
            lineage_of(&url),
            Some((STAGE.to_string(), Kind::Processes, "8c3f".to_string()))
        );
        assert!(lineage_of(&super::super::address(STAGE, Kind::Processes)).is_none());
        assert!(lineage_of("https://example.com/?lineage=cells:1").is_none());
    }

    #[test]
    fn a_process_is_drawn_between_what_it_took_in_and_put_out() {
        // As the stage registry answered for an alignment, on 2026-09-27.
        let node: Value = serde_json::from_str(
            r#"{"id": "a487", "name": "align 12", "state": "SUCCESS",
                "createdAt": "2026-09-01T07:00:00Z", "schemaType": {"name": "prod-ocs-align"},
                "inputs": [{"specimen": null, "subject": null, "dataAsset": {"id": "d1",
                    "name": "ingested", "type": "Directory", "status": "RELEASED", "instances": []}},
                  {"specimen": null, "dataAsset": null, "subject": {"name": "729537"}}],
                "outputs": [{"specimen": null, "subject": null, "dataAsset": {"id": "d2",
                    "name": "aligned", "type": "Directory", "status": "RELEASED",
                    "instances": [{"downloadUrl": "s3://bucket/aligned/"}]}}]}"#,
        )
        .unwrap();
        let root = root_of(Kind::Processes, "a487", &node);
        assert!(root.step);
        assert_eq!(root.name, "align 12");
        let mut lineage = SourceLineage::new(root);
        add_links(&mut lineage, 0, Kind::Processes, &node);
        let names: Vec<(&str, i32, bool)> = lineage
            .nodes()
            .iter()
            .map(|node| (node.name.as_str(), node.depth, node.links == Links::Unread))
            .collect();
        assert_eq!(
            names,
            [
                ("align 12", 0, false),
                ("ingested", -1, true),
                ("729537", -1, false),
                ("aligned", 1, true),
            ]
        );
        assert_eq!(lineage.edges(), [(1, 0), (2, 0), (0, 3)]);
        assert_eq!(
            lineage.nodes()[3].address.as_deref(),
            Some("s3://bucket/aligned/")
        );
    }

    #[test]
    fn a_data_asset_is_drawn_between_the_processes_it_came_out_of_and_went_into() {
        let node: Value = serde_json::from_str(
            r#"{"id": "d2", "name": "aligned", "type": "Directory", "status": "RELEASED",
                "instances": [],
                "outputOf": [{"id": "a487", "name": "align 12", "state": "SUCCESS",
                    "createdAt": "2026-09-01T07:00:00Z", "schemaType": {"name": "prod-ocs-align"}}],
                "inputTo": [{"id": "e1", "name": "export 3", "state": "SUCCESS",
                    "createdAt": "2026-09-02T07:00:00Z", "schemaType": {"name": "cell-omics-data-export"}}]}"#,
        )
        .unwrap();
        let mut lineage = SourceLineage::new(root_of(Kind::DataAssets, "d2", &node));
        assert!(!lineage.nodes()[0].step);
        add_links(&mut lineage, 0, Kind::DataAssets, &node);
        assert_eq!(lineage.nodes()[1].id, "processes:a487");
        assert_eq!(lineage.nodes()[1].depth, -1);
        assert!(lineage.nodes()[1].step);
        assert_eq!(lineage.nodes()[2].depth, 1);
        assert_eq!(lineage.edges(), [(1, 0), (0, 2)]);
    }

    #[test]
    #[ignore = "reads the live BKP Registry, signed in"]
    fn a_live_export_reaches_back_to_its_ingest() {
        use super::super::query::ask_page;
        crate::app::net::block_on(async {
            let asked = Asked {
                terms: vec![TableFilterTerm::Is {
                    field: "schemaType.name".into(),
                    value: "cell-omics-data-export".into(),
                }],
                search: None,
            };
            let landed = ask_page(STAGE, Kind::Processes, 0, 1, &asked, &[])
                .await
                .unwrap();
            let id = landed.nodes[0]["id"].as_str().unwrap();
            let mut lineage = read(STAGE, Kind::Processes, id).await.unwrap().lineage;
            // Back through the inputs only, one layer at a time: forward
            // again from each would read every sibling run.
            let upstream = |lineage: &SourceLineage, at: usize| {
                let node = &lineage.nodes()[at];
                node.parent
                    .is_none_or(|parent| node.depth < lineage.nodes()[parent].depth)
            };
            let mut reads = 0;
            for _ in 0..6 {
                let unread: Vec<usize> = (0..lineage.nodes().len())
                    .filter(|&at| {
                        lineage.nodes()[at].links == Links::Unread && upstream(&lineage, at)
                    })
                    .take(40 - reads)
                    .collect();
                for at in unread {
                    let (kind, record) = related::kind_and_id(&lineage.nodes()[at].id).unwrap();
                    let node = ask_related(STAGE, kind, &record).await.unwrap();
                    add_links(&mut lineage, at, kind, &node);
                    reads += 1;
                }
            }
            let deepest = lineage.nodes().iter().map(|node| node.depth).min().unwrap();
            println!(
                "{} records, back to depth {deepest}: {:?}",
                lineage.nodes().len(),
                lineage
                    .nodes()
                    .iter()
                    .filter(|node| node.step)
                    .map(|node| (&node.detail, node.depth))
                    .collect::<Vec<_>>()
            );
            assert!(
                deepest <= -4,
                "an export is at least two processes past its ingest"
            );
        });
    }
}
