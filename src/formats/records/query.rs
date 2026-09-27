//! What is asked of the registry: a page of records, what a filter offers
//! and how many records hold each of it, and what a picked record links to.

use std::collections::BTreeMap;

use base64::Engine;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::kinds::{Asked, Facet, Kind, Vocabulary, all_of, nest};
use crate::app::graphql::{self, Response};
use crate::app::prefs::registry_token;
use crate::source::table::SortKey;

/// The most the registry sends in one page. Asking for more is an error, not
/// a short page, so a page of the table is asked for in halves of this.
const HALF: usize = 50;

/// Enough pages of a vocabulary for any the registry declares: the longest,
/// the data asset types, was 134 on 2026-09-27.
const MAX_VOCABULARY_PAGES: usize = 20;

/// Ask the registry, as whoever is signed in.
pub(super) async fn ask<T: DeserializeOwned>(
    endpoint: &str,
    query: &str,
    variables: Value,
) -> Result<T, String> {
    let token = registry_token().ok_or("Sign in to the BKP Registry in Settings to read it.")?;
    let text = graphql::post(endpoint, query, variables, Some(&token)).await?;
    Response::<T>::parse(&text)
        .map_err(|e| format!("parsing the BKP Registry's answer: {e}"))?
        .strict()
        .map_err(|error| match error.code() {
            Some(code) if code.starts_with("AUTH_") => {
                "BKP Registry: not authorized. Sign in again in Settings.".to_string()
            }
            _ => format!("BKP Registry: {}", error.message),
        })?
        .ok_or_else(|| "the BKP Registry answered with no data".to_string())
}

/// A page of records, and how many the table holds under its filters.
pub(super) struct Landed {
    pub(super) total: usize,
    pub(super) nodes: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    #[serde(default)]
    total_count: Option<usize>,
    #[serde(default)]
    nodes: Vec<Value>,
}

/// The cursor that starts a page at `offset`: the registry counts its
/// cursors from nothing, and a page begins after the one named.
fn after(offset: usize) -> Value {
    match offset.checked_sub(1) {
        Some(before) => {
            json!(base64::engine::general_purpose::STANDARD.encode(before.to_string()))
        }
        None => Value::Null,
    }
}

/// Read `rows` records from `offset`, narrowed by what is `asked` and
/// ordered by `sort`, in one request of as many halves as that takes.
pub(super) async fn ask_page(
    endpoint: &str,
    kind: Kind,
    offset: usize,
    rows: usize,
    asked: &Asked,
    sort: &[SortKey],
) -> Result<Landed, String> {
    let halves = rows.div_ceil(HALF).max(1);
    let mut declared = format!(
        "$where: {}, $order: [{}!]",
        kind.filter_type(),
        kind.sort_type()
    );
    let mut fields = String::new();
    let mut variables = serde_json::Map::new();
    variables.insert("where".into(), asked.where_value());
    variables.insert("order".into(), kind.order(sort));
    for half in 0..halves {
        declared.push_str(&format!(", $after{half}: String"));
        fields.push_str(&format!(
            "h{half}: {}(first: {HALF}, after: $after{half}, where: $where, order: $order) \
             {{ totalCount nodes {{ {} }} }}\n",
            kind.root(),
            kind.selection()
        ));
        variables.insert(format!("after{half}"), after(offset + half * HALF));
    }
    let query = format!("query({declared}) {{\n{fields}}}");
    let pages: BTreeMap<String, Option<Connection>> =
        ask(endpoint, &query, Value::Object(variables)).await?;
    let mut landed = Landed {
        total: 0,
        nodes: Vec::new(),
    };
    for half in 0..halves {
        let Some(Some(page)) = pages.get(&format!("h{half}")) else {
            continue;
        };
        landed.total = landed.total.max(page.total_count.unwrap_or_default());
        landed.nodes.extend(page.nodes.iter().cloned());
    }
    landed.nodes.truncate(rows);
    Ok(landed)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

/// Every value a filter offers, in order.
async fn vocabulary(endpoint: &str, facet: &Facet) -> Result<Vec<String>, String> {
    let (root, field) = match facet.values {
        Vocabulary::Fixed(values) => {
            return Ok(values.iter().map(ToString::to_string).collect());
        }
        Vocabulary::Listed(root, field) => (root, field),
    };
    let query = format!(
        "query($after: String) {{ v: {root}(first: {HALF}, after: $after) \
         {{ pageInfo {{ hasNextPage endCursor }} nodes {{ {field} }} }} }}"
    );
    let mut values: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_VOCABULARY_PAGES {
        let mut page: BTreeMap<String, Listing> =
            ask(endpoint, &query, json!({ "after": cursor })).await?;
        let Some(listing) = page.remove("v") else {
            break;
        };
        for node in &listing.nodes {
            if let Some(value) = node.get(field).and_then(Value::as_str)
                && !value.trim().is_empty()
                && !values.iter().any(|it| it == value)
            {
                values.push(value.to_string());
            }
        }
        if !listing.page_info.has_next_page {
            break;
        }
        cursor = listing.page_info.end_cursor;
    }
    // A process kind is listed once for each version of its schema.
    values.sort_by_key(|value| value.to_lowercase());
    Ok(values)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Counted {
    total_count: u64,
}

/// How many records each of `wheres` admits, in one request.
async fn count(endpoint: &str, kind: Kind, wheres: Vec<Value>) -> Result<Vec<u64>, String> {
    let asked = wheres.len();
    if asked == 0 {
        return Ok(Vec::new());
    }
    let declared: Vec<String> = (0..asked)
        .map(|at| format!("$w{at}: {}", kind.filter_type()))
        .collect();
    let fields: Vec<String> = (0..asked)
        .map(|at| {
            format!(
                "c{at}: {}(first: 0, where: $w{at}) {{ totalCount }}",
                kind.root()
            )
        })
        .collect();
    let query = format!("query({}) {{ {} }}", declared.join(", "), fields.join(" "));
    let variables: serde_json::Map<String, Value> = wheres
        .into_iter()
        .enumerate()
        .map(|(at, it)| (format!("w{at}"), it))
        .collect();
    let counts: BTreeMap<String, Option<Counted>> =
        ask(endpoint, &query, Value::Object(variables)).await?;
    Ok((0..asked)
        .map(|at| {
            counts
                .get(&format!("c{at}"))
                .and_then(Option::as_ref)
                .map_or(0, |it| it.total_count)
        })
        .collect())
}

/// A filter to read: where it narrows, and the values it already offers if
/// it has been read before and is only being counted again.
pub(super) struct Reading {
    pub(super) path: String,
    pub(super) known: Option<Vec<String>>,
}

/// What each filter offers, and how many records hold each value under what
/// is `asked`.
///
/// Counted under every term, the filter's own included, and under the
/// search, as a table's
/// filters are, so a value left unticked while another is ticked counts
/// none: the count is what ticking it would put on screen. All of them in
/// one request; every data asset type at once took nine seconds, which is
/// why it waits until its filter is opened.
pub(super) async fn ask_values(
    endpoint: &str,
    kind: Kind,
    readings: Vec<Reading>,
    asked: &Asked,
) -> Result<Vec<(String, Vec<(String, u64)>)>, String> {
    let mut labeled = Vec::new();
    for reading in readings {
        let values = match reading.known {
            Some(values) => values,
            None => {
                let Some(facet) = kind.facets().iter().find(|it| it.path == reading.path) else {
                    continue;
                };
                vocabulary(endpoint, facet).await?
            }
        };
        labeled.push((reading.path, values));
    }
    let base = asked.where_value();
    let wheres: Vec<Value> = labeled
        .iter()
        .flat_map(|(path, values)| {
            values
                .iter()
                .map(|value| all_of(vec![base.clone(), nest(path, json!({ "eq": value }))]))
        })
        .collect();
    let mut counts = count(endpoint, kind, wheres).await?.into_iter();
    Ok(labeled
        .into_iter()
        .map(|(path, values)| {
            let counted = values
                .into_iter()
                .map(|value| (value, counts.next().unwrap_or_default()))
                .collect();
            (path, counted)
        })
        .collect())
}

/// What a picked record links to, by kind: the processes a specimen or data
/// asset went into and came out of, and what a process took in and put out.
fn related_query(kind: Kind) -> String {
    const PROCESS: &str = "id name state createdAt schemaType { name }";
    const ASSET: &str = "specimen { name specimenType { label } } \
         dataAsset { name type status instances { downloadUrl } } \
         subject { name species { name } }";
    let links = match kind {
        Kind::Specimens | Kind::DataAssets => {
            format!("inputTo {{ {PROCESS} }} outputOf {{ {PROCESS} }}")
        }
        Kind::Processes => format!("inputs {{ {ASSET} }} outputs {{ {ASSET} }}"),
    };
    format!(
        "query($id: UUID!) {{ r: {}(first: 1, where: {{ id: {{ eq: $id }} }}) \
         {{ nodes {{ {links} }} }} }}",
        kind.root()
    )
}

/// What the record with this id links to, as the registry has it.
pub(super) async fn ask_related(endpoint: &str, kind: Kind, id: &str) -> Result<Value, String> {
    let mut answer: BTreeMap<String, Connection> =
        ask(endpoint, &related_query(kind), json!({ "id": id })).await?;
    answer
        .remove("r")
        .and_then(|it| it.nodes.into_iter().next())
        .ok_or_else(|| format!("the BKP Registry has no record {id}"))
}

#[cfg(test)]
mod tests {
    use super::super::STAGE;
    use super::super::kinds::CREATED;
    use super::*;

    #[test]
    fn a_page_starts_after_the_cursor_before_it() {
        assert_eq!(after(0), Value::Null);
        // The registry's own cursors, as it sent them on 2026-09-27.
        assert_eq!(after(1), json!("MA=="));
        assert_eq!(after(3), json!("Mg=="));
    }

    #[test]
    #[ignore = "reads the live BKP Registry, signed in"]
    fn a_live_page_of_each_kind_reads_into_rows() {
        crate::app::net::block_on(async {
            for kind in Kind::ALL {
                let sort = [SortKey {
                    column: CREATED.into(),
                    descending: true,
                }];
                let landed = ask_page(STAGE, kind, 100, 100, &Asked::default(), &sort)
                    .await
                    .unwrap();
                assert_eq!(landed.nodes.len(), 100, "{kind:?}");
                assert!(landed.total > 100);
                let rows = kind.rows(&landed.nodes);
                println!("{kind:?}: {} in all; {:?}", landed.total, rows[0]);
            }
        });
    }

    #[test]
    #[ignore = "reads the live BKP Registry, signed in"]
    fn a_live_filter_is_read_and_counted() {
        crate::app::net::block_on(async {
            let readings = vec![Reading {
                path: "specimenType.label".into(),
                known: None,
            }];
            let values = ask_values(STAGE, Kind::Specimens, readings, &Asked::default())
                .await
                .unwrap();
            let (_, counts) = &values[0];
            println!("{counts:?}");
            assert!(
                counts
                    .iter()
                    .any(|(label, count)| label == "brain specimen" && *count > 0)
            );
        });
    }

    #[test]
    #[ignore = "reads the live BKP Registry, signed in"]
    fn a_live_search_matches_names_anywhere_in_them() {
        crate::app::net::block_on(async {
            let asked = Asked {
                terms: Vec::new(),
                search: Some("NEUROGLANCER".into()),
            };
            let landed = ask_page(STAGE, Kind::DataAssets, 0, 100, &asked, &[])
                .await
                .unwrap();
            println!("{} data assets named for Neuroglancer", landed.total);
            assert!(landed.total > 100 && landed.total < 10_000);
            assert!(landed.nodes.iter().all(|node| {
                node["name"]
                    .as_str()
                    .is_some_and(|name| name.to_lowercase().contains("neuroglancer"))
            }));
        });
    }
}
