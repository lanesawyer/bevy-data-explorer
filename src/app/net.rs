//! Reads that can be given up on.
//!
//! Every fetch used to be a blocking call on a pool thread, which meant the
//! only moment to decline work was before it started: once a socket read was
//! under way it ran to completion whether or not anyone still wanted the
//! answer. Panning across a cloud therefore paid for every node the view
//! crossed, not the ones it stopped on.
//!
//! So reads run on an async runtime instead, and what a caller holds is a
//! [`Fetching`] — a handle that aborts its task when dropped. Cancelling is
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

/// Fetch a whole file.
pub async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|e| format!("fetching {url}: {e}"))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| format!("reading {url}: {e}"))
}

/// Fetch a whole file as text.
pub async fn fetch_text(url: &str) -> Result<String, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
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
