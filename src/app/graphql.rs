//! Asking a GraphQL API a question.
//!
//! Every API the app talks to past the stores is GraphQL, and each answers the
//! same way: a `data` holding whatever could be given and an `errors` list
//! beside it rather than a status. What differs is what a caller makes of
//! errors next to data — most treat any as failure, while a query of many
//! aliases may keep the ones that answered — so [`Response`] offers both.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// An answer: whatever data the API could give, and what went wrong.
#[derive(Deserialize)]
pub struct Response<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Vec<Error>,
}

/// One thing the API says went wrong.
#[derive(Deserialize)]
pub struct Error {
    pub message: String,
    #[serde(default)]
    extensions: Option<Extensions>,
}

#[derive(Deserialize)]
struct Extensions {
    code: Option<String>,
}

impl Error {
    /// The machine-readable code, such as `AUTH_NOT_AUTHENTICATED`, where the
    /// API gives one.
    pub fn code(&self) -> Option<&str> {
        self.extensions.as_ref()?.code.as_deref()
    }
}

impl<T: DeserializeOwned> Response<T> {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

impl<T> Response<T> {
    /// The data if nothing went wrong, else the first error.
    pub fn strict(self) -> Result<Option<T>, Error> {
        match self.errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(self.data),
        }
    }

    /// Whatever data came, errors beside it left for the caller; the first
    /// error only if there is no data at all.
    pub fn partial(self, endpoint: &str) -> Result<T, String> {
        self.data.ok_or_else(|| {
            self.errors.first().map_or_else(
                || format!("{endpoint} answered with no data"),
                |error| error.message.clone(),
            )
        })
    }
}

/// Send `query` with `variables` and read the answer as text, with a bearer
/// token if the API wants one.
pub async fn post(
    endpoint: &str,
    query: &str,
    variables: Value,
    token: Option<&str>,
) -> Result<String, String> {
    let body = json!({ "query": query, "variables": variables });
    super::net::post_json(endpoint, body.to_string(), token).await
}

/// Send a query and parse the answer, without judging it.
pub async fn answer<T: DeserializeOwned>(
    endpoint: &str,
    query: &str,
    variables: Value,
) -> Result<Response<T>, String> {
    let text = post(endpoint, query, variables, None).await?;
    Response::parse(&text).map_err(|e| format!("parsing {endpoint}: {e}"))
}

/// Send a query and take its data, failing on any error.
pub async fn ask<T: DeserializeOwned>(
    endpoint: &str,
    query: &str,
    variables: Value,
) -> Result<T, String> {
    answer(endpoint, query, variables)
        .await?
        .strict()
        .map_err(|error| error.message)?
        .ok_or_else(|| format!("{endpoint} answered with no data"))
}
