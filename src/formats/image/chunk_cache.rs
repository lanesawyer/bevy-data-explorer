//! Chunks read over the network, kept for the next read that wants them.
//!
//! A tile holds one slice, but the chunks it is cut from hold many: 128 deep
//! on the SmartSPIM stacks. Paging to the slice beside the last one reads the
//! same chunks again, and a frame cut across the stack reads them whole, so
//! without this every page downloaded tens of megabytes it had just thrown
//! away. zarrs' own chunk cache is synchronous and cannot sit in front of an
//! async read, so this sits under zarrs instead, at the store.
//!
//! What is kept is what arrived, still compressed: a page then costs a decode
//! rather than a download. One cache serves every store, so frames looking at
//! one stack three ways share the chunks where their cuts cross, and a chunk
//! two of them ask for at once is downloaded once.
//!
//! Only whole reads are kept — which zarrs makes as a read of one range from
//! the start to the end as often as through `get`. A read of part of a chunk — the blocks
//! `blocks.rs` picks out of a deep chunk — is answered from a whole one
//! already held, and otherwise goes to the network as it did, uncached:
//! fetching the whole chunk instead would give up what those reads save.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use futures::future::{BoxFuture, FutureExt, Shared};
use futures::stream::StreamExt;
use zarrs::storage::byte_range::{ByteRange, ByteRangeIterator};
use zarrs::storage::{
    AsyncMaybeBytesIterator, AsyncReadableStorageTraits, Bytes, MaybeBytes, StorageError, StoreKey,
};

use super::dataset::ReadStore;

/// Compressed chunk bytes held across every store, by default. One view of
/// the SmartSPIM stack cut across its depth reads about 700 MB of chunks, and
/// a budget that holds one such view is what lets paging it stay in memory.
pub const DEFAULT_CHUNK_CACHE_MB: usize = 1024;

type Key = (Arc<str>, StoreKey);
type Download = Shared<BoxFuture<'static, Result<MaybeBytes, String>>>;

struct Held {
    bytes: Bytes,
    last_used: u64,
}

/// A read under way, and how many are waiting on it.
struct Downloading {
    download: Download,
    waiters: usize,
    /// Tells this read from a later one of the same chunk, so a reader of
    /// one that gave up never counts itself off the other.
    id: u64,
}

struct Cache {
    held: HashMap<Key, Held>,
    /// Reads under way, so a second asking for the same chunk waits on the
    /// first rather than downloading it again.
    downloading: HashMap<Key, Downloading>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl Cache {
    fn new(budget: usize) -> Self {
        Cache {
            held: HashMap::new(),
            downloading: HashMap::new(),
            bytes: 0,
            budget,
            clock: 0,
        }
    }

    fn take(&mut self, key: &Key) -> Option<Bytes> {
        self.clock += 1;
        let clock = self.clock;
        self.held.get_mut(key).map(|held| {
            held.last_used = clock;
            held.bytes.clone()
        })
    }

    /// Keep `bytes`, dropping the least recently used until they fit. A chunk
    /// larger than the whole budget is not kept at all.
    fn keep(&mut self, key: Key, bytes: Bytes) {
        if bytes.len() > self.budget || self.held.contains_key(&key) {
            return;
        }
        self.clock += 1;
        self.bytes += bytes.len();
        self.held.insert(
            key,
            Held {
                bytes,
                last_used: self.clock,
            },
        );
        while self.bytes > self.budget {
            let Some(oldest) = self
                .held
                .iter()
                .min_by_key(|(_, held)| held.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(held) = self.held.remove(&oldest) {
                self.bytes -= held.bytes.len();
            }
        }
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::new(DEFAULT_CHUNK_CACHE_MB * 1024 * 1024)))
}

fn lock() -> std::sync::MutexGuard<'static, Cache> {
    cache().lock().unwrap_or_else(PoisonError::into_inner)
}

/// One reader waiting on a read. The read is shared, and the map of reads
/// under way holds it too, so it would outlive every reader giving up — still
/// holding a connection and one of the host's requests, unpolled, for good.
/// So the last reader to leave before it finishes takes it out of the map,
/// which drops it and aborts it, as an uncached read would be.
struct Waiting {
    key: Key,
    id: u64,
    finished: bool,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut cache = lock();
        let last = match cache.downloading.get_mut(&self.key) {
            Some(read) if read.id == self.id => {
                read.waiters -= 1;
                read.waiters == 0
            }
            _ => false,
        };
        if last {
            cache.downloading.remove(&self.key);
        }
    }
}

/// Hold at most `bytes` of chunks, from now on.
pub fn set_budget(bytes: usize) {
    lock().budget = bytes;
}

/// A store whose whole reads are kept in the shared cache, under `address`.
pub fn cached(address: &str, inner: ReadStore) -> ReadStore {
    Arc::new(CachedStore {
        address: address.into(),
        inner,
    })
}

struct CachedStore {
    /// What the store's keys are relative to, so two stores' chunks of the
    /// same name are kept apart.
    address: Arc<str>,
    inner: ReadStore,
}

impl CachedStore {
    fn key(&self, key: &StoreKey) -> Key {
        (self.address.clone(), key.clone())
    }
}

#[async_trait::async_trait]
impl AsyncReadableStorageTraits for CachedStore {
    async fn get(&self, key: &StoreKey) -> Result<MaybeBytes, StorageError> {
        let cached = self.key(key);
        let (download, mut waiting) = {
            let mut cache = lock();
            if let Some(bytes) = cache.take(&cached) {
                return Ok(Some(bytes));
            }
            cache.clock += 1;
            let id = cache.clock;
            let read = cache.downloading.entry(cached.clone()).or_insert_with(|| {
                let inner = self.inner.clone();
                let key = key.clone();
                Downloading {
                    download: async move { inner.get(&key).await.map_err(|e| e.to_string()) }
                        .boxed()
                        .shared(),
                    waiters: 0,
                    id,
                }
            });
            read.waiters += 1;
            let waiting = Waiting {
                key: cached.clone(),
                id: read.id,
                finished: false,
            };
            (read.download.clone(), waiting)
        };
        let outcome = download.await;
        waiting.finished = true;
        let mut cache = lock();
        if cache
            .downloading
            .get(&cached)
            .is_some_and(|read| read.id == waiting.id)
        {
            cache.downloading.remove(&cached);
        }
        match outcome {
            Ok(Some(bytes)) => {
                cache.keep(cached, bytes.clone());
                Ok(Some(bytes))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(StorageError::Other(e)),
        }
    }

    async fn get_partial_many<'a>(
        &'a self,
        key: &StoreKey,
        byte_ranges: ByteRangeIterator<'a>,
    ) -> Result<AsyncMaybeBytesIterator<'a>, StorageError> {
        // zarrs reads a whole chunk this way too, as one range from the start
        // to the end, so that is a whole read, and kept like one.
        let byte_ranges: Vec<ByteRange> = byte_ranges.collect();
        if let [ByteRange::FromStart(0, None)] = byte_ranges[..] {
            return Ok(self
                .get(key)
                .await?
                .map(|whole| futures::stream::iter([Ok(whole)]).boxed()));
        }
        let held = lock().take(&self.key(key));
        let Some(whole) = held else {
            return self
                .inner
                .get_partial_many(key, Box::new(byte_ranges.into_iter()))
                .await;
        };
        let size = whole.len() as u64;
        let parts: Vec<Result<Bytes, StorageError>> = byte_ranges
            .into_iter()
            .map(|range| slice(&whole, range, size))
            .collect();
        Ok(Some(futures::stream::iter(parts).boxed()))
    }

    async fn size_key(&self, key: &StoreKey) -> Result<Option<u64>, StorageError> {
        if let Some(whole) = lock().take(&self.key(key)) {
            return Ok(Some(whole.len() as u64));
        }
        self.inner.size_key(key).await
    }

    fn supports_get_partial(&self) -> bool {
        self.inner.supports_get_partial()
    }
}

fn slice(whole: &Bytes, range: ByteRange, size: u64) -> Result<Bytes, StorageError> {
    let wanted = range.to_range(size);
    if wanted.end > size {
        return Err(StorageError::Other(format!(
            "byte range {range} is past the end of a {size} byte chunk"
        )));
    }
    Ok(whole.slice(wanted.start as usize..wanted.end as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Key {
        ("https://h/a.zarr".into(), StoreKey::new(name).unwrap())
    }

    #[test]
    fn the_least_recently_used_chunk_goes_first() {
        let mut cache = Cache::new(10);
        cache.keep(key("a"), Bytes::from(vec![0; 4]));
        cache.keep(key("b"), Bytes::from(vec![0; 4]));
        assert!(cache.take(&key("a")).is_some(), "a is now the more recent");
        cache.keep(key("c"), Bytes::from(vec![0; 4]));
        assert!(cache.take(&key("b")).is_none());
        assert!(cache.take(&key("a")).is_some());
        assert!(cache.take(&key("c")).is_some());
        assert!(cache.bytes <= 10);
    }

    #[test]
    fn a_chunk_larger_than_the_budget_is_not_kept() {
        let mut cache = Cache::new(4);
        cache.keep(key("a"), Bytes::from(vec![0; 3]));
        cache.keep(key("big"), Bytes::from(vec![0; 5]));
        assert!(cache.take(&key("big")).is_none());
        assert!(
            cache.take(&key("a")).is_some(),
            "nothing was evicted for it"
        );
    }

    #[test]
    fn part_of_a_held_chunk_is_cut_from_it() {
        let whole = Bytes::from((0u8..10).collect::<Vec<_>>());
        let part = slice(&whole, ByteRange::FromStart(2, Some(3)), 10).unwrap();
        assert_eq!(&part[..], &[2, 3, 4]);
        assert_eq!(
            &slice(&whole, ByteRange::Suffix(2), 10).unwrap()[..],
            &[8, 9]
        );
        assert!(slice(&whole, ByteRange::FromStart(8, Some(5)), 10).is_err());
    }
}
