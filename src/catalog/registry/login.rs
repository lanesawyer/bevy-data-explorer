//! Signing in to the BKP Registry through the browser.
//!
//! OAuth's authorization code flow with PKCE, against the Institute's Cognito
//! pool, with the app as a public client: it opens the pool's login page, and
//! the browser comes back to a listener on this machine with a code that is
//! traded for tokens. The client is the one registered for the registry's MCP
//! server. Cognito matches a callback exactly, port and path included, and
//! `localhost:8765/callback` is the only one that client has, which is why
//! neither can be picked here.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SecureRandom;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::app::net::{Fetching, client, fetching};

/// From the pool's `.well-known/openid-configuration`, on 2026-09-24.
const AUTHORIZE: &str = "https://brain-dev-lims.auth.us-west-2.amazoncognito.com/oauth2/authorize";
const TOKEN: &str = "https://brain-dev-lims.auth.us-west-2.amazoncognito.com/oauth2/token";
const CLIENT_ID: &str = "a71dtdcbg2htfh5mf39s4e9fm";
const SCOPES: &str = "openid email";

const PORT: u16 = 8765;
const REDIRECT: &str = "http://localhost:8765/callback";

/// How long the listener waits for the browser to come back.
const WAIT: Duration = Duration::from_secs(300);

/// How long before its token expires a sign-in is renewed, so a search is
/// never the one to find it expired.
const RENEW_AHEAD_SECS: u64 = 300;

/// What a sign-in or a renewal hands back.
#[derive(Debug)]
pub struct Tokens {
    /// What the registry is sent as the bearer.
    pub access: String,
    /// What renews `access`. Cognito sends one on signing in and not on
    /// renewing, when the one held stays good.
    pub refresh: Option<String>,
    /// Whose account it is, from the ID token.
    pub email: Option<String>,
}

/// A sign-in waiting on the browser, which stops listening when dropped.
pub struct SignIn {
    /// The login page, kept so it can be opened again if its tab is lost.
    pub url: String,
    pub waiting: Fetching<Result<Tokens, String>>,
}

/// Start listening for the browser, and say where to send it.
///
/// Fails at once if the port is taken, typically by an MCP client signing in
/// with the same registration.
pub fn sign_in() -> Result<SignIn, String> {
    let listeners = bind()?;
    let verifier = random_text(32)?;
    let state = random_text(16)?;
    let url = authorize_url(&challenge(&verifier), &state);
    let waiting = fetching(async move {
        let code = tokio::time::timeout(WAIT, callback(listeners, &state))
            .await
            .map_err(|_| "BKP Registry: the browser did not come back within five minutes")??;
        let form = [
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            ("code_verifier", &verifier),
        ];
        request_tokens(&form).await.map_err(|failure| failure.0)
    });
    Ok(SignIn { url, waiting })
}

/// Why a renewal failed, and whether trying again later could help.
#[derive(Debug)]
pub struct Failure(pub String, pub Retry);

#[derive(Debug, PartialEq, Eq)]
pub enum Retry {
    /// The refresh token itself was refused: only signing in again will do.
    SignInAgain,
    Later,
}

/// Trade a refresh token for a fresh access token.
pub async fn renew(refresh: String) -> Result<Tokens, Failure> {
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", &refresh),
    ];
    request_tokens(&form).await
}

/// Whether `token` is due to be renewed: it expires within
/// [`RENEW_AHEAD_SECS`], or says nothing about when it expires.
pub fn due(token: &str, now: u64) -> bool {
    claims(token)
        .and_then(|claims| claims.exp)
        .is_none_or(|exp| now + RENEW_AHEAD_SECS >= exp)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Both loopbacks, since `localhost` may resolve to either. The IPv6 one is
/// optional: a machine without it still has the other.
fn bind() -> Result<Vec<TcpListener>, String> {
    let v4 = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, PORT))).map_err(|e| {
        if e.kind() == io::ErrorKind::AddrInUse {
            format!("BKP Registry: port {PORT} is in use, likely by another sign-in")
        } else {
            format!("BKP Registry: listening on port {PORT}: {e}")
        }
    })?;
    let mut listeners = vec![v4];
    listeners.extend(TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, PORT))).ok());
    for listener in &listeners {
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("BKP Registry: listening on port {PORT}: {e}"))?;
    }
    Ok(listeners)
}

fn authorize_url(challenge: &str, state: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT)
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .finish();
    format!("{AUTHORIZE}?{query}")
}

fn random_text(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut buffer)
        .map_err(|_| "BKP Registry: no randomness to sign in with".to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        verifier.as_bytes(),
    ))
}

/// Answer whatever the browser sends until one request is the callback.
async fn callback(listeners: Vec<TcpListener>, state: &str) -> Result<String, String> {
    let listeners = listeners
        .into_iter()
        .map(tokio::net::TcpListener::from_std)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("BKP Registry: listening on port {PORT}: {e}"))?;
    loop {
        let accepts = listeners.iter().map(|listener| Box::pin(listener.accept()));
        let (accepted, _, _) = futures::future::select_all(accepts).await;
        let Ok((stream, _)) = accepted else { continue };
        if let Some(outcome) = answer(stream, state).await {
            return outcome;
        }
    }
}

/// What one request to the listener was.
#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Refused(String),
    /// From a login page opened by an earlier sign-in.
    Stale,
    /// Not the callback at all, such as the browser asking for an icon.
    Elsewhere,
}

fn read_callback(request: &str, state: &str) -> Callback {
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        return Callback::Elsewhere;
    }
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    if get("state").as_deref() != Some(state) {
        return Callback::Stale;
    }
    if let Some(error) = get("error") {
        return Callback::Refused(get("error_description").unwrap_or(error));
    }
    get("code").map_or(Callback::Elsewhere, Callback::Code)
}

async fn answer(mut stream: tokio::net::TcpStream, state: &str) -> Option<Result<String, String>> {
    let mut request = Vec::new();
    let mut buffer = [0; 2048];
    while !request.windows(4).any(|end| end == b"\r\n\r\n") && request.len() < 16 * 1024 {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let (status, page, outcome) = match read_callback(&String::from_utf8_lossy(&request), state) {
        Callback::Code(code) => (
            "200 OK",
            "Signed in to the BKP Registry. You can close this tab and go back to the explorer.",
            Some(Ok(code)),
        ),
        Callback::Refused(reason) => (
            "200 OK",
            "The BKP Registry sign-in was refused. The explorer's log says why.",
            Some(Err(format!("BKP Registry: sign-in refused: {reason}"))),
        ),
        Callback::Stale => (
            "400 Bad Request",
            "This sign-in page is from an earlier attempt. Sign in again from the explorer.",
            None,
        ),
        Callback::Elsewhere => ("404 Not Found", "Nothing here.", None),
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>BKP Registry</title>\
         <body style=\"font-family:sans-serif;margin:4em\"><p>{page}</p>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // The code is in hand whether or not the page reaches the browser.
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    outcome
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn request_tokens(form: &[(&str, &str)]) -> Result<Tokens, Failure> {
    let later = |e: String| Failure(e, Retry::Later);
    let response = client()
        .post(TOKEN)
        .form(form)
        .send()
        .await
        .map_err(|e| later(format!("BKP Registry: asking for a token: {e}")))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| later(format!("BKP Registry: reading a token: {e}")))?;
    let parsed: TokenResponse = serde_json::from_str(&text).map_err(|e| {
        later(format!(
            "BKP Registry: a token answer that is not JSON ({status}): {e}"
        ))
    })?;
    if let Some(error) = parsed.error {
        let retry = if error == "invalid_grant" {
            Retry::SignInAgain
        } else {
            Retry::Later
        };
        let reason = parsed.error_description.unwrap_or(error);
        return Err(Failure(
            format!("BKP Registry: token refused: {reason}"),
            retry,
        ));
    }
    let access = parsed
        .access_token
        .ok_or_else(|| later("BKP Registry: the token answer held no access token".into()))?;
    Ok(Tokens {
        access,
        refresh: parsed.refresh_token,
        email: parsed
            .id_token
            .as_deref()
            .and_then(claims)
            .and_then(|claims| claims.email),
    })
}

#[derive(Deserialize)]
struct Claims {
    exp: Option<u64>,
    email: Option<String>,
}

/// A JWT's claims, read without checking its signature: the registry does
/// that, and all that is wanted here is when to renew and whose it is.
fn claims(token: &str) -> Option<Claims> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(claims: &str) -> String {
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims))
    }

    #[test]
    fn the_challenge_is_the_verifier_hashed_and_url_safe() {
        // From `openssl dgst -sha256 -binary | base64`, made URL-safe.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r2wW1gFWFOEjXk"),
            "dZrJz5b___3oA5uVny1DZqEO9NM98brdg_f4AEY03Bc"
        );
    }

    #[test]
    fn the_login_page_is_asked_for_the_registered_callback() {
        let url = url::Url::parse(&authorize_url("abc", "xyz")).unwrap();
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(pairs.contains(&("redirect_uri".into(), REDIRECT.into())));
        assert!(pairs.contains(&("code_challenge_method".into(), "S256".into())));
        assert!(pairs.contains(&("scope".into(), SCOPES.into())));
    }

    #[test]
    fn the_callback_is_told_from_everything_else_the_browser_asks() {
        let request = |target: &str| {
            read_callback(&format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n"), "s1")
        };
        assert_eq!(
            request("/callback?code=c0de&state=s1"),
            Callback::Code("c0de".into())
        );
        assert_eq!(request("/callback?code=c0de&state=old"), Callback::Stale);
        assert_eq!(
            request("/callback?error=access_denied&error_description=no+thanks&state=s1"),
            Callback::Refused("no thanks".into())
        );
        assert_eq!(request("/favicon.ico"), Callback::Elsewhere);
    }

    #[test]
    fn a_token_is_renewed_ahead_of_expiring() {
        let token = jwt(r#"{"exp":10000,"email":"a@b.org"}"#);
        assert!(!due(&token, 10000 - RENEW_AHEAD_SECS - 1));
        assert!(due(&token, 10000 - RENEW_AHEAD_SECS));
        assert!(due("not a jwt", 0));
        assert_eq!(claims(&token).unwrap().email.as_deref(), Some("a@b.org"));
    }

    #[test]
    #[ignore = "reads the live Cognito pool, and listens on the callback port"]
    fn a_made_up_code_comes_back_and_is_refused_by_cognito() {
        let mut sign_in = sign_in().unwrap();
        let url = url::Url::parse(&sign_in.url).unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        crate::app::net::block_on(async {
            let page = client().get(&sign_in.url).send().await.unwrap();
            assert!(!page.url().as_str().contains("redirect_mismatch"));
            let back = format!("{REDIRECT}?code=made-up&state={state}");
            let stale = client().get(format!("{REDIRECT}?code=x&state=old")).send();
            assert_eq!(stale.await.unwrap().status(), 400);
            assert!(
                client()
                    .get(back)
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .is_success()
            );
        });
        let problem = loop {
            if let Some(outcome) = sign_in.waiting.take() {
                break outcome.unwrap_err();
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        println!("{problem}");
        assert!(problem.contains("token refused"));
    }
}
