//! The datasets the app ships knowing the address of, as a catalog like any
//! other.

use futures::future::BoxFuture;

use super::{Catalog, Entry};
use crate::formats::EXAMPLES;

pub struct Examples;

impl Catalog for Examples {
    fn name(&self) -> &str {
        "Examples"
    }

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        let entries = EXAMPLES
            .iter()
            .map(|example| Entry {
                name: example.name.to_string(),
                kind: example.kind.to_string(),
                url: example.url.to_string(),
                keywords: String::new(),
            })
            .collect();
        Box::pin(async move { Ok(entries) })
    }
}
