//! The BKP Registry: the Institute's internal record of data assets, and
//! where each one is stored.
//!
//! A separate API from the public platform in [`super::bkp`], and one that
//! answers nobody without a bearer token, got by signing in through the
//! browser ([`login`]). Only the pre-production service exists so far.
//!
//! It holds far too many assets to list — 17.8 million on 2026-09-21 — so it
//! is a searched catalog: what is typed into a picker is matched against
//! asset names, among only the types the viewer can open. Before anything is
//! typed, a picker is sent the first of those assets by name, to browse.
//!
//! Settings also has a test request, which logs a page of assets of any type.

pub mod login;

use std::collections::BTreeMap;
use std::sync::RwLock;

use bevy::prelude::*;
use futures::future::BoxFuture;
use serde::Deserialize;

use super::{Catalog, Entry, Found, Provider};
use crate::app::graphql::{self, Response};
use crate::app::net::{Fetching, fetching};
use crate::app::prefs::Preferences;
use crate::source::Category;

/// The pre-production service. Introspection is open; everything else wants
/// a token.
pub const STAGE: &str = "https://stage-bkpr.brain.devlims.org/graphql/";

/// Assets asked for by the test request.
pub const SAMPLE: usize = 25;

/// The most the registry sends in one page. Asking for more is an error, not
/// a short page.
const PAGE_MAX: usize = 50;

/// The asset types the viewer can open, from the 134 the registry declared on
/// 2026-09-21: what each is called beside an entry, and what it is drawn as.
///
/// Every OME-Zarr store sampled was Zarr v2 in a public bucket. Every table
/// sampled was in a private bucket or on a file share, so most will not open
/// until reads can carry credentials.
///
/// Types holding no assets are left out, since asking for them alone is not
/// merely empty: the registry scans every asset for them and times out at
/// 30 s. `NGFF` sounds like OME-Zarr, and `TSV`, `Parquet` and `SVG` like
/// things the viewer reads, and all four hold nothing. Nothing held is
/// Scatterbrain, so the registry has no cells to offer.
///
/// A Neuroglancer state is a `JSON` asset like 65,000 others, and is told
/// apart by its name, which is why a kind may require a word in it: the
/// light-sheet stacks' states are all `…_neuroglancer_config`. Their `MIP
/// JSON` siblings are left out until a frame can show a plane other than
/// y by x, which is the plane they are not.
const KINDS: [(&str, &str, Category, Option<&str>); 5] = [
    ("zarr fileset", "OME-Zarr", Category::Image, None),
    (
        "MIP zarr fileset",
        "OME-Zarr projection",
        Category::Image,
        None,
    ),
    (
        "JSON",
        "Neuroglancer",
        Category::Image,
        Some("neuroglancer"),
    ),
    ("CSV", "CSV", Category::Table, None),
    ("parquet", "Parquet", Category::Table, None),
];

/// The token in the preferences, copied here so a search running off the
/// main thread can send it.
static TOKEN: RwLock<Option<String>> = RwLock::new(None);

/// Keep [`TOKEN`] the one in the preferences.
pub fn sync_token(prefs: Res<Preferences>) {
    if prefs.is_changed()
        && let Ok(mut token) = TOKEN.write()
        && *token != prefs.registry_token
    {
        token.clone_from(&prefs.registry_token);
    }
}

/// A renewal in flight, and when the next may start after one failed.
#[derive(Default)]
pub struct Renewing {
    task: Option<Fetching<Result<login::Tokens, login::Failure>>>,
    not_before: u64,
}

/// How long to wait after a renewal before trying another, so a registry
/// that is down, or tokens with no expiry to read, are not asked every frame.
const RENEW_BACKOFF_SECS: u64 = 60;

/// Renew a signed-in token before it expires, including one that expired
/// while the app was closed. A pasted token has nothing to renew it.
pub fn renew_token(mut prefs: ResMut<Preferences>, mut renewing: Local<Renewing>) {
    let now = login::now();
    if let Some(task) = renewing.task.as_mut() {
        let Some(outcome) = task.take() else { return };
        renewing.task = None;
        renewing.not_before = now + RENEW_BACKOFF_SECS;
        match outcome {
            Ok(tokens) => {
                prefs.registry_token = Some(tokens.access);
                if let Some(login) = prefs.registry_login.as_mut() {
                    if tokens.refresh.is_some() {
                        login.refresh_token = tokens.refresh;
                    }
                    if tokens.email.is_some() {
                        login.email = tokens.email;
                    }
                }
                info!("renewed the BKP Registry sign-in");
            }
            Err(login::Failure(problem, login::Retry::SignInAgain)) => {
                warn!("{problem}. Sign in to the BKP Registry again in Settings.");
                prefs.registry_login = None;
            }
            Err(login::Failure(problem, login::Retry::Later)) => warn!("{problem}"),
        }
        return;
    }
    if now < renewing.not_before {
        return;
    }
    let Some(refresh) = prefs
        .registry_login
        .as_ref()
        .and_then(|login| login.refresh_token.clone())
    else {
        return;
    };
    if prefs
        .registry_token
        .as_deref()
        .is_none_or(|token| login::due(token, now))
    {
        renewing.task = Some(fetching(login::renew(refresh)));
    }
}

/// The registry, searched by name for what the viewer can open.
pub struct Registry {
    endpoint: String,
}

impl Registry {
    pub fn stage() -> Self {
        Registry {
            endpoint: STAGE.to_string(),
        }
    }
}

pub const PROVIDER: Provider = Provider {
    key: "bkp-registry",
    name: "BKP Registry",
    about: "The Institute's internal record of data assets. Pre-production, and needs signing in.",
    examples: &[],
};

impl Catalog for Registry {
    fn name(&self) -> &str {
        "BKP Registry"
    }

    fn provider(&self) -> Option<Provider> {
        Some(PROVIDER)
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn searched(&self) -> bool {
        true
    }

    fn search(
        &self,
        text: String,
        only: Option<Category>,
    ) -> BoxFuture<'static, Result<Found, String>> {
        // A category the registry holds nothing of is not worth a request.
        if categories(only).is_empty() {
            return Box::pin(async { Ok(Found::default()) });
        }
        let endpoint = self.endpoint.clone();
        let token = TOKEN.read().ok().and_then(|token| token.clone());
        Box::pin(async move {
            let token = token.ok_or("Sign in to the BKP Registry in Settings to search it")?;
            search(&endpoint, &token, &text, only).await
        })
    }
}

/// The categories asked about for `only`, in the order they are listed.
fn categories(only: Option<Category>) -> Vec<Category> {
    Category::ALL
        .into_iter()
        .filter(|category| only.is_none_or(|only| only == *category))
        .filter(|category| KINDS.iter().any(|(_, _, of, _)| of == category))
        .collect()
}

/// Assets of the categories `only` names whose names hold `text`, or the
/// first of them by name when `text` is empty.
///
/// Each category is asked for in a page of its own, splitting one page
/// between them. Asked together, tables outnumber images five to one and
/// their names sort first, so a picker showing everything would show only
/// tables.
async fn search(
    endpoint: &str,
    token: &str,
    text: &str,
    only: Option<Category>,
) -> Result<Found, String> {
    let categories = categories(only);
    let first = PAGE_MAX / categories.len().max(1);
    let mut query = String::from("query($first: Int");
    let mut fields = String::new();
    let mut variables = serde_json::Map::new();
    variables.insert("first".into(), first.into());
    for (at, category) in categories.iter().enumerate() {
        query.push_str(&format!(", $where{at}: DataAssetFilterInput"));
        fields.push_str(&format!(
            "  c{at}: dataAssets(first: $first, order: [{{ name: ASC }}], where: $where{at}) {{{ASSETS}}}\n"
        ));
        variables.insert(format!("where{at}"), filter(*category, text));
    }
    query.push_str(") {\n");
    query.push_str(&fields);
    query.push('}');
    let text = graphql::post(endpoint, &query, variables.into(), Some(token)).await?;
    let page = parse_sample(&text)?;
    Ok(Found {
        total: page.total,
        entries: page.assets.into_iter().filter_map(entry).collect(),
    })
}

/// Assets of one category, and whose names hold `text` unless it is empty.
///
/// The kinds known by type alone are asked for in one `in`, and each kind
/// that also wants a word in its name beside them.
fn filter(category: Category, text: &str) -> serde_json::Value {
    let kinds = KINDS.iter().filter(|(_, _, of, _)| *of == category);
    let types: Vec<&str> = kinds
        .clone()
        .filter(|(_, _, _, named)| named.is_none())
        .map(|(name, ..)| *name)
        .collect();
    let mut any = vec![serde_json::json!({ "type": { "name": { "in": types } } })];
    any.extend(kinds.filter_map(|(name, _, _, named)| {
        Some(serde_json::json!({ "and": [
            { "type": { "name": { "eq": name } } },
            { "name": { "containsInsensitive": (*named)? } },
        ] }))
    }));
    let of_type = if any.len() == 1 {
        any.remove(0)
    } else {
        serde_json::json!({ "or": any })
    };
    if text.is_empty() {
        of_type
    } else {
        serde_json::json!({ "and": [of_type, { "name": { "containsInsensitive": text } }] })
    }
}

/// What is read of each asset, by both the search and the test request.
const ASSETS: &str = "
    totalCount
    pageInfo { hasNextPage endCursor }
    nodes {
      id
      name
      type
      status
      tags
      instances { downloadUrl }
    }
  ";

/// An asset as a dataset to open, if it is stored anywhere that can be read.
///
/// Opened from its first copy with an address, since a copy on a file share
/// (`//host/share/...`) or a mounted path is nothing to fetch. The others are
/// kept as keywords, so it can be found by any of its buckets.
fn entry(asset: Asset) -> Option<Entry> {
    let &(_, kind, category, _) = KINDS.iter().find(|(name, _, _, named)| {
        asset.kind.as_deref() == Some(*name)
            && named.is_none_or(|word| asset.name.to_lowercase().contains(word))
    })?;
    let mut urls: Vec<String> = asset
        .instances
        .into_iter()
        .map(|instance| instance.download_url)
        .collect();
    let url = urls.remove(urls.iter().position(|url| url.contains("://"))?);
    let mut keywords: Vec<String> = vec![asset.status, asset.id];
    keywords.extend(asset.tags.into_iter().flatten().flatten());
    keywords.extend(urls);
    Some(Entry {
        name: asset.name,
        kind: kind.to_string(),
        category,
        url,
        keywords: keywords.join(" "),
        cells: None,
    })
}

/// Enough of a token to tell which one is saved, and no more.
pub fn token_hint(token: &str) -> String {
    let tail: String = token
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("\u{2026}{tail}")
}

/// A page of the registry's data assets, and how many there are in all.
#[derive(Debug)]
pub struct AssetSample {
    pub total: usize,
    pub more: bool,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize, Debug)]
pub struct Asset {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub status: String,
    #[serde(default)]
    pub tags: Option<Vec<Option<String>>>,
    #[serde(default)]
    pub instances: Vec<Instance>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub download_url: String,
}

impl AssetSample {
    /// How many assets on the page are of each type, for seeing what the
    /// registry holds before choosing what to list.
    pub fn kinds(&self) -> BTreeMap<&str, usize> {
        let mut kinds = BTreeMap::new();
        for asset in &self.assets {
            *kinds
                .entry(asset.kind.as_deref().unwrap_or("(no type)"))
                .or_default() += 1;
        }
        kinds
    }
}

/// Ask for the first [`SAMPLE`] data assets.
pub async fn sample_assets(endpoint: String, token: String) -> Result<AssetSample, String> {
    let query = format!("query($first: Int) {{ dataAssets(first: $first) {{{ASSETS}}} }}");
    let variables = serde_json::json!({ "first": SAMPLE });
    let text = graphql::post(&endpoint, &query, variables, Some(&token)).await?;
    parse_sample(&text)
}

/// A page of assets under each name it was asked for: `dataAssets`, or one
/// alias a category when searching.
type Pages = BTreeMap<String, Option<Connection>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    total_count: usize,
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Asset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
}

fn parse_sample(text: &str) -> Result<AssetSample, String> {
    let response = Response::<Pages>::parse(text).map_err(|e| {
        let start: String = text.chars().take(300).collect();
        format!("parsing BKP Registry data assets: {e}: {start}")
    })?;
    let pages = response.strict().map_err(|error| match error.code() {
        Some(code) if code.starts_with("AUTH_") => {
            "BKP Registry: not authorized. Sign in again in Settings.".to_string()
        }
        Some(code) => format!("BKP Registry: {} ({code})", error.message),
        None => format!("BKP Registry: {}", error.message),
    })?;
    let connections: Vec<Connection> = pages
        .into_iter()
        .flat_map(BTreeMap::into_values)
        .flatten()
        .collect();
    if connections.is_empty() {
        return Err("BKP Registry: the response held no data assets".into());
    }
    let mut sample = AssetSample {
        total: 0,
        more: false,
        assets: Vec::new(),
    };
    for connection in connections {
        sample.total += connection.total_count;
        sample.more |= connection.page_info.has_next_page;
        sample.assets.extend(connection.nodes);
    }
    Ok(sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written from the schema rather than captured, since nothing answers
    /// without a token. Replace with a real page once one is in hand.
    const PAGE: &str = r#"{"data":{"dataAssets":{
        "totalCount": 812,
        "pageInfo": {"hasNextPage": true, "endCursor": "Mg=="},
        "nodes": [
            {"id": "5b1e", "name": "Brain 1 OME-Zarr", "type": "zarr fileset", "status": "PUBLISHED",
             "tags": ["mouse", null],
             "instances": [{"downloadUrl": "https://example.org/brain1.ome.zarr"}]},
            {"id": "7c2f", "name": "No type yet", "type": null, "status": "PENDING_REVIEW",
             "tags": null, "instances": []}
        ]
    }}}"#;

    #[test]
    fn a_page_reads_its_assets_and_their_types() {
        let sample = parse_sample(PAGE).unwrap();
        assert_eq!(sample.total, 812);
        assert!(sample.more);
        assert_eq!(sample.assets.len(), 2);
        assert_eq!(
            sample.assets[0].instances[0].download_url,
            "https://example.org/brain1.ome.zarr"
        );
        let kinds = sample.kinds();
        assert_eq!(kinds["zarr fileset"], 1);
        assert_eq!(kinds["(no type)"], 1);
    }

    #[test]
    fn an_asset_opens_from_its_first_copy_and_is_found_by_the_others() {
        let text = r#"{"data":{"dataAssets":{
            "totalCount": 1, "pageInfo": {"hasNextPage": false},
            "nodes": [{"id": "9a", "name": "ome_zarr_image_series_1370718127",
                "type": "zarr fileset", "status": "PUBLISHED", "tags": null,
                "instances": [
                    {"downloadUrl": "s3://cortex-aav-toolbox-802451596237-us-west-2/epifluorescence/1370718127/ome_zarr_conversion/1370718127.zarr"},
                    {"downloadUrl": "s3://allen-genetic-tools/epifluorescence/1370718127/ome_zarr_conversion/1370718127.zarr/"}
                ]}]
        }}}"#;
        let mut assets = parse_sample(text).unwrap().assets;
        let entry = entry(assets.remove(0)).unwrap();
        assert_eq!(entry.kind, "OME-Zarr");
        assert!(entry.url.starts_with("s3://cortex-aav-toolbox"));
        assert!(entry.keywords.contains("s3://allen-genetic-tools"));
        assert!(entry.keywords.contains("PUBLISHED"));
    }

    #[test]
    fn each_category_is_its_own_page_and_a_file_share_is_not_an_address() {
        // Two aliases, as a search of everything asks for them.
        let text = r#"{"data":{
            "c0": {"totalCount": 18695, "pageInfo": {"hasNextPage": true}, "nodes": [
                {"id": "1", "name": "1104197092_OME_zarr_image_series", "type": "MIP zarr fileset",
                 "status": "PUBLISHED", "tags": null,
                 "instances": [{"downloadUrl": "s3://allen-genetic-tools/1104197092.zarr"}]}]},
            "c1": {"totalCount": 80639, "pageInfo": {"hasNextPage": true}, "nodes": [
                {"id": "2", "name": "codebook.csv", "type": "CSV", "status": "PENDING_REVIEW",
                 "tags": null,
                 "instances": [{"downloadUrl": "//10.128.133.8/MERSCOPENAS03_data/codebook.csv"}]},
                {"id": "3", "name": "clusters.csv", "type": "CSV", "status": "PENDING_REVIEW",
                 "tags": null, "instances": [
                    {"downloadUrl": "//10.128.133.8/share/clusters.csv"},
                    {"downloadUrl": "s3://aibs-taxonomies-internal/clusters.csv"}]}]}
        }}"#;
        let sample = parse_sample(text).unwrap();
        assert_eq!(sample.total, 18695 + 80639);
        let entries: Vec<Entry> = sample.assets.into_iter().filter_map(entry).collect();
        let found: Vec<(&str, Category, &str)> = entries
            .iter()
            .map(|entry| (entry.kind.as_str(), entry.category, entry.url.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (
                    "OME-Zarr projection",
                    Category::Image,
                    "s3://allen-genetic-tools/1104197092.zarr"
                ),
                (
                    "CSV",
                    Category::Table,
                    "s3://aibs-taxonomies-internal/clusters.csv"
                ),
            ]
        );
        assert!(entries[1].keywords.contains("//10.128.133.8/share"));
    }

    #[test]
    fn a_category_it_holds_nothing_of_is_not_asked_about() {
        assert_eq!(categories(None), [Category::Image, Category::Table]);
        assert!(categories(Some(Category::Cells)).is_empty());
        assert!(categories(Some(Category::Annotations)).is_empty());
        let browse = filter(Category::Table, "");
        assert_eq!(browse["type"]["name"]["in"][1], "parquet");
        let search = filter(Category::Image, "brain");
        assert_eq!(search["and"][1]["name"]["containsInsensitive"], "brain");
    }

    #[test]
    fn an_asset_stored_nowhere_is_not_offered() {
        let sample = parse_sample(PAGE).unwrap();
        let entries: Vec<Entry> = sample.assets.into_iter().filter_map(entry).collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn a_missing_token_points_to_the_settings() {
        // As the stage service answered without one, on 2026-09-21.
        let text = r#"{"errors":[{"message":"The current user is not authorized to access this resource.","locations":[{"line":1,"column":22}],"path":["dataAssets"],"extensions":{"code":"AUTH_NOT_AUTHENTICATED","category":"Unauthorized","statusCode":"401"}}],"data":{"dataAssets":null}}"#;
        let error = parse_sample(text).unwrap_err();
        assert!(error.contains("not authorized"));
        assert!(error.contains("Settings"));
        assert!(!error.contains("AUTH_NOT_AUTHENTICATED"));
    }

    #[test]
    fn a_token_is_only_hinted_at() {
        assert_eq!(token_hint("abc.def.ghij"), "\u{2026}ghij");
        assert_eq!(token_hint("ab"), "\u{2026}ab");
    }

    #[test]
    #[ignore = "reads the live BKP Registry, with BKPR_TOKEN set"]
    fn a_live_search_finds_stores_that_open() {
        let token = std::env::var("BKPR_TOKEN").expect("BKPR_TOKEN");
        let token = token.trim();
        let found = crate::app::net::block_on(search(STAGE, token, "1370718127", None)).unwrap();
        assert_eq!(found.total, 1);
        let entry = &found.entries[0];
        println!("{} — {}", entry.name, entry.url);
        crate::app::net::block_on(crate::formats::discover::discover(&entry.url))
            .expect("the store should open");
    }

    #[test]
    #[ignore = "reads the live BKP Registry, with BKPR_TOKEN set"]
    fn a_live_browse_sends_a_page_of_each_category() {
        let token = std::env::var("BKPR_TOKEN").expect("BKPR_TOKEN");
        let found = crate::app::net::block_on(search(STAGE, token.trim(), "", None)).unwrap();
        println!("{} in all, {} sent", found.total, found.entries.len());
        for category in [Category::Image, Category::Table] {
            assert!(
                found.entries.iter().any(|entry| entry.category == category),
                "no {category:?} browsed"
            );
        }
        let images =
            crate::app::net::block_on(search(STAGE, token.trim(), "", Some(Category::Image)))
                .unwrap();
        assert_eq!(images.entries.len(), PAGE_MAX);
    }

    #[test]
    #[ignore = "reads the live BKP Registry, with BKPR_TOKEN set"]
    fn the_stage_registry_answers_a_token() {
        let token = std::env::var("BKPR_TOKEN").expect("BKPR_TOKEN");
        let sample =
            crate::app::net::block_on(sample_assets(STAGE.to_string(), token.trim().to_string()))
                .unwrap();
        println!("{} data assets; types on the first page:", sample.total);
        for (kind, count) in sample.kinds() {
            println!("  {kind}: {count}");
        }
        for asset in &sample.assets {
            println!("  {} [{:?}] {:?}", asset.name, asset.kind, asset.instances);
        }
    }
}
