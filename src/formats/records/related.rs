//! What a picked record links to: the processes a specimen or data asset
//! went into and came out of, and the specimens, data assets and subjects a
//! process took in and put out.
//!
//! Asked for one record at a time, when it is picked. Asked with the page it
//! would come for every row: one specimen alone was input to over a hundred
//! processes. A record followed from another along a [`RecordTrail`] is
//! asked the same way, its own fields with it.

use super::query::ask_related;
use super::*;
use crate::source::table::{
    FollowedRecord, RecordLineage, RecordTrail, RelatedGroup, RelatedRecord,
};

use super::kinds::{Shape, read, time_of};

/// Fetch what the record shown links to — the one at the end of the trail,
/// or the one picked out if nothing has been followed — and hand it over
/// when it lands.
pub(super) fn serve_related(
    mut sources: Query<(
        &mut RecordPages,
        &SelectedRecord,
        &RecordTrail,
        &mut RelatedRecords,
        &mut FollowedRecord,
        &mut RecordLineage,
    )>,
) {
    for (mut pages, record, trail, mut related, mut followed, mut lineage) in &mut sources {
        let wanted = match trail.0.last() {
            Some(step) => kind_and_id(&step.link),
            None => id_of(record).map(|id| (pages.kind, id)),
        };
        let following = !trail.0.is_empty();
        if wanted != pages.picked {
            related.set_if_neq(if wanted.is_some() {
                RelatedRecords::Fetching
            } else {
                RelatedRecords::None
            });
            followed.set_if_neq(match (following, &wanted) {
                (false, _) => FollowedRecord::None,
                (true, Some(_)) => FollowedRecord::Fetching,
                (true, None) => FollowedRecord::Failed("Not a record this table can read.".into()),
            });
            lineage.set_if_neq(RecordLineage(None));
            pages.linking = wanted.clone().map(|(kind, id)| {
                let endpoint = pages.endpoint.clone();
                fetching(async move { ask_related(&endpoint, kind, &id).await })
            });
            pages.picked = wanted;
        }
        let Some(answer) = pages.linking.as_mut().and_then(Fetching::take) else {
            continue;
        };
        pages.linking = None;
        let Some((kind, id)) = pages.picked.clone() else {
            continue;
        };
        match answer {
            Ok(node) => {
                let groups = groups_of(kind, &node);
                // A record linked to nothing has no lineage worth a frame.
                if !groups.is_empty() {
                    lineage.set_if_neq(RecordLineage(Some(lineage::address(
                        &pages.endpoint,
                        kind,
                        &id,
                    ))));
                }
                *related = RelatedRecords::Ready(groups);
                if following {
                    *followed = FollowedRecord::Ready(fields_of(kind, &node));
                }
            }
            Err(e) => {
                warn!("reading what a BKP Registry record links to: {e}");
                *related = RelatedRecords::Failed(e.clone());
                if following {
                    *followed = FollowedRecord::Failed(e);
                }
            }
        }
    }
}

/// What a linked record is followed by: the kind of record it is and its id.
pub(super) fn link_to(kind: Kind, id: &str) -> String {
    format!("{}:{id}", kind.root())
}

/// The kind and id a [`link_to`] names.
pub(super) fn kind_and_id(link: &str) -> Option<(Kind, String)> {
    let (root, id) = link.split_once(':')?;
    Some((Kind::from_root(root)?, id.to_string()))
}

/// A followed record's fields, headed as its table's columns are.
fn fields_of(kind: Kind, node: &Value) -> Vec<(String, String)> {
    let values = kind
        .rows(std::slice::from_ref(node))
        .pop()
        .unwrap_or_default();
    kind.headers().into_iter().zip(values).collect()
}

/// What a record links to, under what links it. A link it has none of is
/// left out.
fn groups_of(kind: Kind, node: &Value) -> Vec<RelatedGroup> {
    let links: [(&str, &str); 2] = match kind {
        Kind::Specimens | Kind::DataAssets => [
            ("inputTo", "Input to processes"),
            ("outputOf", "Output of processes"),
        ],
        Kind::Processes => [("inputs", "Inputs"), ("outputs", "Outputs")],
    };
    links
        .into_iter()
        .filter_map(|(field, title)| {
            let listed = node.get(field)?.as_array()?;
            let records: Vec<RelatedRecord> = match kind {
                Kind::Processes => listed.iter().filter_map(asset_of).collect(),
                _ => listed.iter().map(process_of).collect(),
            };
            (!records.is_empty()).then(|| RelatedGroup {
                title: title.to_string(),
                records,
            })
        })
        .collect()
}

/// Several things about a record, the ones it has, in a line.
fn line(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|it| !it.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

fn text(node: &Value, path: &str) -> String {
    read(node, path, Shape::Text)
}

pub(super) fn process_of(node: &Value) -> RelatedRecord {
    RelatedRecord {
        name: text(node, "name"),
        detail: line(&[
            text(node, "schemaType.name"),
            text(node, "state"),
            node.get("createdAt")
                .and_then(Value::as_str)
                .map(time_of)
                .unwrap_or_default(),
        ]),
        address: None,
        link: node
            .get("id")
            .and_then(Value::as_str)
            .map(|id| link_to(Kind::Processes, id)),
    }
}

/// Whatever a process took in or put out: a data asset, a specimen or a
/// subject. Anything else it can hold — a cell, a collection — is not read.
pub(super) fn asset_of(node: &Value) -> Option<RelatedRecord> {
    if let Some(asset) = node.get("dataAsset").filter(|it| !it.is_null()) {
        // Opened from its first copy with an address, since one on a file
        // share is nothing to fetch.
        let addresses = asset
            .get("instances")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|it| it.get("downloadUrl")?.as_str());
        let address = addresses
            .clone()
            .find(|it| it.contains("://"))
            .or_else(|| addresses.clone().next());
        return Some(RelatedRecord {
            name: text(asset, "name"),
            detail: line(&[
                "Data asset".into(),
                text(asset, "type"),
                text(asset, "status"),
            ]),
            address: address.map(str::to_string),
            link: asset
                .get("id")
                .and_then(Value::as_str)
                .map(|id| link_to(Kind::DataAssets, id)),
        });
    }
    if let Some(specimen) = node.get("specimen").filter(|it| !it.is_null()) {
        return Some(RelatedRecord {
            name: text(specimen, "name"),
            detail: line(&["Specimen".into(), text(specimen, "specimenType.label")]),
            address: None,
            link: specimen
                .get("id")
                .and_then(Value::as_str)
                .map(|id| link_to(Kind::Specimens, id)),
        });
    }
    let subject = node.get("subject").filter(|it| !it.is_null())?;
    Some(RelatedRecord {
        name: text(subject, "name"),
        detail: line(&["Subject".into(), text(subject, "species.name")]),
        address: None,
        link: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_lists_what_it_took_in_and_put_out() {
        // As the stage registry answered for a scan's archiving, on
        // 2026-09-27.
        let node: Value = serde_json::from_str(
            r#"{"inputs": [{"specimen": {"name": "C57BL6J-729537",
                "specimenType": {"label": "brain specimen"}}, "dataAsset": null, "subject": null}],
              "outputs": [{"specimen": null, "subject": null, "dataAsset": {
                "name": "0500389528-0011", "type": "Directory", "status": "ARCHIVED",
                "instances": [{"downloadUrl": "//10.128.133.8/share/0011"},
                              {"downloadUrl": "s3://aibs-archive-gda-historical/0378/0500389528-0011/"}]}}]}"#,
        )
        .unwrap();
        let groups = groups_of(Kind::Processes, &node);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].title, "Inputs");
        assert_eq!(
            groups[0].records[0].detail,
            "Specimen \u{b7} brain specimen"
        );
        let output = &groups[1].records[0];
        assert_eq!(output.detail, "Data asset \u{b7} Directory \u{b7} ARCHIVED");
        assert_eq!(
            output.address.as_deref(),
            Some("s3://aibs-archive-gda-historical/0378/0500389528-0011/")
        );
        assert!(groups[0].records[0].link.is_none(), "no id was sent");
    }

    #[test]
    fn a_linked_record_is_followed_by_its_kind_and_id() {
        let node: Value = serde_json::from_str(
            r#"{"inputs": [{"specimen": {"id": "5e1f", "name": "C57BL6J-729537",
                "specimenType": {"label": "brain specimen"}}, "dataAsset": null, "subject": null},
              {"specimen": null, "dataAsset": null, "subject": {"name": "729537"}}],
              "outputs": []}"#,
        )
        .unwrap();
        let groups = groups_of(Kind::Processes, &node);
        let [specimen, subject] = &groups[0].records[..] else {
            panic!("{groups:?}");
        };
        let link = specimen.link.clone().unwrap();
        assert_eq!(link, "specimens:5e1f");
        assert_eq!(
            kind_and_id(&link),
            Some((Kind::Specimens, "5e1f".to_string()))
        );
        assert!(subject.link.is_none(), "subjects are not a table");
    }

    #[test]
    fn a_followed_record_has_its_tables_columns() {
        let node: Value = serde_json::from_str(
            r#"{"id": "8c3f", "name": "Scan 1370064309 archive", "state": {"name": "SUCCESS"}}"#,
        )
        .unwrap();
        let fields = fields_of(Kind::Processes, &node);
        assert_eq!(fields.len(), Kind::Processes.headers().len());
        assert!(
            fields
                .iter()
                .any(|(_, value)| value == "Scan 1370064309 archive")
        );
    }

    #[test]
    fn a_specimen_lists_its_processes_and_leaves_out_an_empty_link() {
        let node: Value = serde_json::from_str(
            r#"{"inputTo": [{"id": "8c3f", "name": "Scan 1370064309 archive", "state": "SUCCESS",
                "createdAt": "2024-06-04T07:00:49.253Z",
                "schemaType": {"name": "Isilon to Deep Glacier Archiver"}}],
              "outputOf": []}"#,
        )
        .unwrap();
        let groups = groups_of(Kind::Specimens, &node);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].title, "Input to processes");
        let process = &groups[0].records[0];
        assert_eq!(process.name, "Scan 1370064309 archive");
        assert_eq!(
            process.detail,
            "Isilon to Deep Glacier Archiver \u{b7} SUCCESS \u{b7} 2024-06-04 07:00"
        );
        assert!(process.address.is_none());
    }

    #[test]
    #[ignore = "reads the live BKP Registry, signed in"]
    fn a_live_record_of_each_kind_is_linked() {
        use super::super::query::ask_page;
        crate::app::net::block_on(async {
            for kind in Kind::ALL {
                let landed = ask_page(STAGE, kind, 0, 50, &Default::default(), &[])
                    .await
                    .unwrap();
                let mut linked = 0;
                for node in &landed.nodes {
                    let id = node["id"].as_str().unwrap();
                    let related = ask_related(STAGE, kind, id).await.unwrap();
                    let groups = groups_of(kind, &related);
                    if let Some(group) = groups.first() {
                        println!("{kind:?} {id}: {} {:?}", group.title, group.records[0]);
                        linked += 1;
                        break;
                    }
                }
                assert!(
                    linked > 0,
                    "no {kind:?} on the first page links to anything"
                );
            }
        });
    }
}
