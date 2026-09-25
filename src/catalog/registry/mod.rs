//! The BKP Registry: the Institute's internal record of data assets, and
//! where each one is stored.
//!
//! A separate API from the public platform in [`super::bkp`], and one that
//! answers nobody without a bearer token, got by signing in through the
//! browser ([`login`]). Only the pre-production service exists so far.
//!
//! It holds far too many assets to list — 17.8 million on 2026-09-21 — so it
//! is a searched catalog: what is typed into a picker is matched against
//! asset names, among only the types the viewer can open.
//!
//! Settings also has a test request, which logs a page of assets of any type.

pub mod login;

use std::collections::BTreeMap;
use std::sync::RwLock;

use bevy::prelude::*;
use futures::future::BoxFuture;
use serde::Deserialize;

use super::{Catalog, Entry, Found, Provider};
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

/// The asset types that are OME-Zarr stores, from the 134 the registry
/// declared on 2026-09-21. `NGFF` sounds like one and holds no assets. Every
/// store sampled from both was Zarr v2 in a public bucket.
const OME_ZARR: [(&str, &str); 2] = [
    ("zarr fileset", "OME-Zarr"),
    ("MIP zarr fileset", "OME-Zarr projection"),
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

/// The registry, searched for OME-Zarr stores by name.
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
        // Only images are searched for so far, so any other filter has
        // nothing to ask about.
        if only.is_some_and(|only| only != Category::Image) {
            return Box::pin(async { Ok(Found::default()) });
        }
        let endpoint = self.endpoint.clone();
        let token = TOKEN.read().ok().and_then(|token| token.clone());
        Box::pin(async move {
            let token = token.ok_or("Sign in to the BKP Registry in Settings to search it")?;
            search(&endpoint, &token, &text).await
        })
    }
}

async fn search(endpoint: &str, token: &str, text: &str) -> Result<Found, String> {
    let types: Vec<&str> = OME_ZARR.iter().map(|(name, _)| *name).collect();
    let body = serde_json::json!({
        "query": SEARCH,
        "variables": { "first": PAGE_MAX, "types": types, "text": text },
    });
    let text = crate::app::net::post_json_bearer(endpoint, body.to_string(), token).await?;
    let page = parse_sample(&text)?;
    Ok(Found {
        total: page.total,
        entries: page.assets.into_iter().filter_map(entry).collect(),
    })
}

const SEARCH: &str = "query($first: Int, $types: [String], $text: String) {
  dataAssets(
    first: $first
    order: [{ name: ASC }]
    where: { and: [
      { type: { name: { in: $types } } }
      { name: { containsInsensitive: $text } }
    ] }
  ) {
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
  }
}";

/// An asset as a dataset to open, if it is stored anywhere.
///
/// Opened from its first copy. The others are kept as keywords, so it can be
/// found by any of its buckets.
fn entry(asset: Asset) -> Option<Entry> {
    let mut urls = asset
        .instances
        .into_iter()
        .map(|instance| instance.download_url);
    let url = urls.next()?;
    let kind = OME_ZARR
        .iter()
        .find(|(name, _)| asset.kind.as_deref() == Some(*name))
        .map_or("Data asset", |(_, kind)| *kind);
    let mut keywords: Vec<String> = vec![asset.status, asset.id];
    keywords.extend(asset.tags.into_iter().flatten().flatten());
    keywords.extend(urls);
    Some(Entry {
        name: asset.name,
        kind: kind.to_string(),
        category: Category::Image,
        url,
        keywords: keywords.join(" "),
        cells: None,
    })
}

const QUERY: &str = "query($first: Int) {
  dataAssets(first: $first) {
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
  }
}";

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
    let body = serde_json::json!({
        "query": QUERY,
        "variables": { "first": SAMPLE },
    });
    let text = crate::app::net::post_json_bearer(&endpoint, body.to_string(), &token).await?;
    parse_sample(&text)
}

#[derive(Deserialize)]
struct Response {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
    #[serde(default)]
    extensions: Option<Extensions>,
}

#[derive(Deserialize)]
struct Extensions {
    code: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    data_assets: Option<Connection>,
}

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
    let response: Response = serde_json::from_str(text).map_err(|e| {
        let start: String = text.chars().take(300).collect();
        format!("parsing BKP Registry data assets: {e}: {start}")
    })?;
    if let Some(error) = response.errors.first() {
        let code = error
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.code.as_deref());
        return Err(match code {
            Some(code) if code.starts_with("AUTH_") => {
                "BKP Registry: not authorized. Sign in again in Settings.".to_string()
            }
            Some(code) => format!("BKP Registry: {} ({code})", error.message),
            None => format!("BKP Registry: {}", error.message),
        });
    }
    let connection = response
        .data
        .and_then(|data| data.data_assets)
        .ok_or("BKP Registry: the response held no data assets")?;
    Ok(AssetSample {
        total: connection.total_count,
        more: connection.page_info.has_next_page,
        assets: connection.nodes,
    })
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
            {"id": "5b1e", "name": "Brain 1 OME-Zarr", "type": "OME_ZARR", "status": "PUBLISHED",
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
        assert_eq!(kinds["OME_ZARR"], 1);
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
        let found = crate::app::net::block_on(search(STAGE, token, "1370718127")).unwrap();
        assert_eq!(found.total, 1);
        let entry = &found.entries[0];
        println!("{} — {}", entry.name, entry.url);
        crate::app::net::block_on(crate::formats::discover::discover(&entry.url))
            .expect("the store should open");
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
