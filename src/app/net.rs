//! Reads that can be given up on.
//!
//! Every fetch used to be a blocking call on a pool thread, which meant the
//! only moment to decline work was before it started: once a socket read was
//! under way it ran to completion whether or not anyone still wanted the
//! answer. Panning across a cloud therefore paid for every node the view
//! crossed, not the ones it stopped on.
//!
//! So reads run on an async runtime instead, and what a caller holds is a
//! [`Fetching`] — a handle that aborts its task when dropped. Canceling is
//! then the same act as forgetting: a streamer drops the slot of a node it no
//! longer wants, and the request behind it stops.
//!
//! The runtime is `reqwest`'s to drive rather than Bevy's. Its futures register
//! with tokio's reactor, and polling them on Bevy's pool would need the runtime
//! entered across an await — a guard that is not `Send`, which a task on that
//! pool must be. Running them on tokio and polling the handle from a system
//! sidesteps that, and is what makes the abort available.

use std::sync::OnceLock;

use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

/// The runtime every read is driven by.
///
/// One for the whole app: a runtime is a thread pool, and the reads it serves
/// are waiting on sockets rather than using those threads.
fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("a tokio runtime is needed to read anything over HTTP")
    })
}

/// The client every read goes through.
///
/// Shared so that connections are too: a cloud is read as hundreds of small
/// files from one host, and a client per request would mean a TLS handshake
/// per request.
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// A read in progress, which stops when this is dropped.
///
/// Dropping a `tokio` join handle detaches its task rather than ending it, so
/// the abort is explicit here. Without it, forgetting a node would leave its
/// download running to fill a slot nobody was holding.
pub struct Fetching<T> {
    handle: JoinHandle<T>,
}

impl<T> Drop for Fetching<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl<T: Send + 'static> Fetching<T> {
    /// Take the answer if there is one.
    ///
    /// Only ever called once the task has finished, so the wait inside is not
    /// one: it is how a handle's value is taken out.
    pub fn take(&mut self) -> Option<T> {
        if !self.handle.is_finished() {
            return None;
        }
        runtime().block_on(&mut self.handle).ok()
    }
}

/// Start `work`, and hand back the handle that cancels it.
pub fn fetching<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> Fetching<T> {
    Fetching {
        handle: runtime().spawn(work),
    }
}

/// Run `work` to completion here and now.
///
/// For the command line, which opens what it was told to before there is a
/// window to draw into, and has nothing else to be getting on with.
pub fn block_on<T>(work: impl Future<Output = T>) -> T {
    runtime().block_on(work)
}

/// Whether `source` is read over HTTP rather than off disk.
pub fn is_http(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// Whether someone on another machine could open this address: anything
/// with a scheme but `file://`.
///
/// Not the same question as [`is_http`], which asks how to fetch: an `s3://`
/// address is fetched over HTTP once it is translated, but is remote as typed.
/// A Neuroglancer format prefix is looked through to the address it wraps, so
/// `zarr2://s3://bucket/key` is as remote as the bucket.
pub fn is_remote(source: &str) -> bool {
    let mut scheme = None;
    let mut rest = source.trim();
    while let Some((name, after)) = rest.split_once("://") {
        let is_scheme = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !is_scheme {
            break;
        }
        scheme = Some(name);
        rest = after;
    }
    scheme.is_some_and(|scheme| !scheme.eq_ignore_ascii_case("file"))
}

/// Read a whole file, over HTTP or off disk.
pub async fn read(source: &str) -> Result<Vec<u8>, String> {
    if is_http(source) {
        fetch(source).await
    } else {
        tokio::fs::read(source)
            .await
            .map_err(|e| format!("reading {source}: {e}"))
    }
}

/// Read a whole file as text, over HTTP or off disk.
pub async fn read_text(source: &str) -> Result<String, String> {
    if is_http(source) {
        fetch_text(source).await
    } else {
        tokio::fs::read_to_string(source)
            .await
            .map_err(|e| format!("reading {source}: {e}"))
    }
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("fetching {url}: {e}"))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| format!("reading {url}: {e}"))
}

/// Post a JSON body and read the answer as text.
///
/// The answer is read whatever the status, because a GraphQL error arrives as
/// a 500 with the reason in the body; the status is only reported when the
/// body says nothing better. A `token` is sent as a bearer.
pub async fn post_json(url: &str, body: String, token: Option<&str>) -> Result<String, String> {
    let mut request = client()
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .body(body)
        .send()
        .await
        .map_err(|e| format!("querying {url}: {e}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("reading {url}: {e}"))?;
    if !status.is_success() && !text.contains("\"errors\"") {
        return Err(format!("querying {url}: {status}"));
    }
    Ok(text)
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("fetching {url}: {e}"))?;
    response
        .text()
        .await
        .map_err(|e| format!("reading {url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn an_address_is_remote_unless_it_is_on_this_machine() {
        for remote in [
            "https://example.com/a.zarr/",
            "http://example.com/metadata.json",
            "s3://allen-genetic-tools/tissuecyte/1219090168.zarr/",
            "zarr2://s3://allen-genetic-tools/tissuecyte/1219090168.zarr/",
            "https://neuroglancer-demo.appspot.com/#!s3://bucket/state.json",
        ] {
            assert!(is_remote(remote), "{remote}");
        }
        for local in [
            "/data/local.zarr",
            "./metadata.json",
            "metadata.json",
            "file:///data/local.zarr",
            "zarr://file:///data/local.zarr",
            "",
        ] {
            assert!(!is_remote(local), "{local}");
        }
    }

    /// The property the streamers rely on: forgetting a read stops it.
    ///
    /// A task is abandoned at its next await, which for a read is between the
    /// request and the body, or partway through the body. Anything already
    /// computed is thrown away with it.
    #[test]
    fn dropping_a_read_stops_it_before_it_finishes() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();

        let reading = fetching(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            flag.store(true, Ordering::SeqCst);
        });
        drop(reading);

        std::thread::sleep(Duration::from_millis(600));
        assert!(
            !finished.load(Ordering::SeqCst),
            "the task ran to completion after being dropped"
        );
    }

    #[test]
    fn a_read_that_is_kept_does_finish() {
        // The other half: nothing about the abort is happening by accident.
        let mut reading = fetching(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            7u32
        });
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(reading.take(), Some(7));
    }

    #[test]
    fn an_unfinished_read_yields_nothing_rather_than_waiting() {
        // Systems poll this every frame; a `take` that blocked would stall the
        // window until the network answered.
        let started = std::time::Instant::now();
        let mut reading = fetching(async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            1u32
        });
        assert_eq!(reading.take(), None);
        assert!(started.elapsed() < Duration::from_millis(200));
    }
}
