//! What a picked record links to: the processes a specimen or data asset
//! went into and came out of, and the specimens, data assets and subjects a
//! process took in and put out.
//!
//! Asked for one record at a time, when it is picked. Asked with the page it
//! would come for every row: one specimen alone was input to over a hundred
//! processes.

use super::query::ask_related;
use super::*;
use crate::source::table::{RelatedGroup, RelatedRecord};

use super::kinds::{Shape, read, time_of};

/// Fetch what the record picked out links to, and hand it over when it
/// lands.
pub(super) fn serve_related(
    mut sources: Query<(&mut RecordPages, &SelectedRecord, &mut RelatedRecords)>,
) {
    for (mut pages, record, mut related) in &mut sources {
        let wanted = id_of(record);
        if wanted != pages.picked {
            related.set_if_neq(if wanted.is_some() {
                RelatedRecords::Fetching
            } else {
                RelatedRecords::None
            });
            pages.linking = wanted.clone().map(|id| {
                let (endpoint, kind) = (pages.endpoint.clone(), pages.kind);
                fetching(async move { ask_related(&endpoint, kind, &id).await })
            });
            pages.picked = wanted;
        }
        let Some(answer) = pages.linking.as_mut().and_then(Fetching::take) else {
            continue;
        };
        pages.linking = None;
        *related = match answer {
            Ok(node) => RelatedRecords::Ready(groups_of(pages.kind, &node)),
            Err(e) => {
                warn!("reading what a BKP Registry record links to: {e}");
                RelatedRecords::Failed(e)
            }
        };
    }
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

fn process_of(node: &Value) -> RelatedRecord {
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
    }
}

/// Whatever a process took in or put out: a data asset, a specimen or a
/// subject. Anything else it can hold — a cell, a collection — is not read.
fn asset_of(node: &Value) -> Option<RelatedRecord> {
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
        });
    }
    if let Some(specimen) = node.get("specimen").filter(|it| !it.is_null()) {
        return Some(RelatedRecord {
            name: text(specimen, "name"),
            detail: line(&["Specimen".into(), text(specimen, "specimenType.label")]),
            address: None,
        });
    }
    let subject = node.get("subject").filter(|it| !it.is_null())?;
    Some(RelatedRecord {
        name: text(subject, "name"),
        detail: line(&["Subject".into(), text(subject, "species.name")]),
        address: None,
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
                let landed = ask_page(STAGE, kind, 0, 50, &[], &[]).await.unwrap();
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
