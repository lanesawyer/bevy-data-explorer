//! Reading one slice out of a deep blosc chunk without downloading all of it.
//!
//! The reference Tissuecyte stack keeps forty slices in every chunk, so a tile
//! showing one slice used to fetch forty: a level-0 tile covers sixteen chunks
//! of 2.3 MB each, 37 MB downloaded to draw one of forty slices. Blosc
//! compresses a chunk in independent blocks — here 256 KB each, eight slices
//! of one channel — and lists where each block starts in a table after its
//! header. So a slice can be read by fetching the header and table once, then
//! only the blocks that hold it: three of fifteen for a three-channel tile,
//! about a quarter of the bytes.
//!
//! Only a chunk format simple enough to be sure of is read this way: Zarr v2,
//! blosc and nothing else, C order, little-endian samples, and a fill of zero.
//! Anything else goes through zarrs as before.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zarrs::storage::byte_range::ByteRange;

use super::dataset::SharedArray;

/// Bytes of a blosc header before its block table.
const HEADER: usize = 16;

/// The header and table are read speculatively in one request of this size,
/// which holds the table for up to 252 blocks; a chunk with more is topped up.
const FIRST_READ: u64 = 1024;

/// Chunk headers remembered, so paging to another slice of the same chunk
/// costs its blocks and nothing else. Each is under a hundred bytes.
const MAX_HEADERS: usize = 65_536;

/// What a blosc chunk says about its own layout.
struct Header {
    /// The header and table exactly as stored, copied into the front of the
    /// buffer the blocks are decoded from.
    raw: Vec<u8>,
    typesize: usize,
    blocksize: usize,
    compressed: usize,
    /// Stored without compression, so the data follows the header directly.
    memcpyed: bool,
    starts: Vec<usize>,
}

impl Header {
    fn parse(bytes: &[u8]) -> Result<Header, String> {
        if bytes.len() < HEADER {
            return Err("blosc header is short".into());
        }
        // Formats 1 and 2 are c-blosc 1.x; blosc2 frames look different.
        if !matches!(bytes[0], 1 | 2) {
            return Err(format!("blosc format {} is not read by block", bytes[0]));
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let flags = bytes[2];
        let (typesize, nbytes, blocksize, compressed) =
            (usize::from(bytes[3]), word(4), word(8), word(12));
        if typesize == 0 || blocksize == 0 {
            return Err("blosc header names no block size".into());
        }
        let memcpyed = flags & 0x02 != 0;
        let blocks = if memcpyed {
            0
        } else {
            nbytes.div_ceil(blocksize)
        };
        let table = HEADER + blocks * 4;
        if bytes.len() < table {
            return Err(format!("blosc table needs {table} bytes"));
        }
        let starts = (0..blocks).map(|b| word(HEADER + b * 4)).collect();
        Ok(Header {
            raw: bytes[..table].to_vec(),
            typesize,
            blocksize,
            compressed,
            memcpyed,
            starts,
        })
    }

    /// Bytes needed from the start of the chunk to hold header and table.
    fn table_len(bytes: &[u8]) -> Option<usize> {
        if bytes.len() < HEADER || bytes[2] & 0x02 != 0 {
            return Some(HEADER);
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let blocksize = word(8);
        (blocksize > 0).then(|| HEADER + word(4).div_ceil(blocksize) * 4)
    }

    /// Where block `index` lies in the stored chunk.
    fn block_range(&self, index: usize) -> (usize, usize) {
        let start = self.starts[index];
        // The next block to start after this one, or the end of the chunk:
        // blocks are written in order, but nothing here depends on it.
        let end = self
            .starts
            .iter()
            .copied()
            .filter(|s| *s > start)
            .min()
            .unwrap_or(self.compressed);
        (start, end)
    }
}

/// Reads planes of a level's chunks block by block, when the level qualifies.
#[derive(Default)]
pub struct BlockReader {
    headers: Mutex<HashMap<Vec<u64>, Option<Arc<Header>>>>,
}

impl BlockReader {
    /// A reader for `array`, or `None` when its chunks are not plain v2 blosc.
    pub fn for_array(array: &SharedArray) -> Option<Arc<BlockReader>> {
        let metadata = serde_json::to_value(array.metadata()).ok()?;
        let plain = metadata.get("zarr_format")?.as_u64()? == 2
            && metadata.get("compressor")?.get("id")?.as_str()? == "blosc"
            && metadata
                .get("filters")
                .is_none_or(serde_json::Value::is_null)
            && metadata.get("order")?.as_str()? == "C"
            && metadata.get("fill_value")?.as_f64()? == 0.0
            && metadata
                .get("dtype")?
                .as_str()
                .is_some_and(|dtype| dtype.starts_with('<') || dtype.starts_with('|'));
        plain.then(|| Arc::new(BlockReader::default()))
    }

    /// The header of a chunk, fetched once. `None` when the chunk was never
    /// written, which the fill of zero stands in for.
    async fn header(
        &self,
        array: &SharedArray,
        chunk: &[u64],
    ) -> Result<Option<Arc<Header>>, String> {
        if let Some(known) = self.headers.lock().unwrap().get(chunk) {
            return Ok(known.clone());
        }
        let store = array.storage();
        let key = array.chunk_key(chunk);
        let first = store
            .get_partial(&key, ByteRange::FromStart(0, Some(FIRST_READ)))
            .await
            .map_err(|e| format!("reading the header of {key}: {e}"))?;
        let header = match first {
            None => None,
            Some(first) => {
                let mut bytes = first.to_vec();
                let wanted = Header::table_len(&bytes).ok_or("blosc header names no block size")?;
                if bytes.len() < wanted {
                    let rest = store
                        .get_partial(
                            &key,
                            ByteRange::FromStart(
                                bytes.len() as u64,
                                Some((wanted - bytes.len()) as u64),
                            ),
                        )
                        .await
                        .map_err(|e| format!("reading the block table of {key}: {e}"))?
                        .ok_or_else(|| format!("{key} vanished while being read"))?;
                    bytes.extend_from_slice(&rest);
                }
                Some(Arc::new(Header::parse(&bytes)?))
            }
        };
        let mut headers = self.headers.lock().unwrap();
        if headers.len() >= MAX_HEADERS {
            headers.clear();
        }
        headers.insert(chunk.to_vec(), header.clone());
        Ok(header)
    }

    /// Parts of chunk `chunk` as decoded, one for each `(offset, length)` in
    /// bytes, fetching only the blocks they lie in. `None` for a chunk never
    /// written.
    pub async fn read(
        &self,
        array: &SharedArray,
        chunk: &[u64],
        parts: &[(usize, usize)],
    ) -> Result<Option<Vec<Vec<u8>>>, String> {
        let Some(header) = self.header(array, chunk).await? else {
            return Ok(None);
        };
        let store = array.storage();
        let key = array.chunk_key(chunk);
        let fetch = |start: usize, end: usize| {
            let store = store.clone();
            let key = key.clone();
            async move {
                store
                    .get_partial(
                        &key,
                        ByteRange::FromStart(start as u64, Some((end - start) as u64)),
                    )
                    .await
                    .map_err(|e| format!("reading {key}: {e}"))?
                    .ok_or_else(|| format!("{key} vanished while being read"))
            }
        };

        if header.memcpyed {
            let reads = parts
                .iter()
                .map(|(offset, length)| fetch(HEADER + offset, HEADER + offset + length));
            let bytes = futures::future::try_join_all(reads).await?;
            return Ok(Some(bytes.into_iter().map(|b| b.to_vec()).collect()));
        }

        // Every block any part touches, each fetched once and all at once.
        let mut blocks: Vec<usize> = parts
            .iter()
            .flat_map(|(offset, length)| {
                let first = offset / header.blocksize;
                let last = (offset + length).saturating_sub(1) / header.blocksize;
                first..=last
            })
            .filter(|b| *b < header.starts.len())
            .collect();
        blocks.sort_unstable();
        blocks.dedup();
        let ranges: Vec<(usize, usize)> = blocks.iter().map(|b| header.block_range(*b)).collect();
        let fetched =
            futures::future::try_join_all(ranges.iter().map(|(start, end)| fetch(*start, *end)))
                .await?;

        // The chunk as blosc expects it, with only the blocks that are needed
        // in place: `blosc1_getitem` reads the header and table and decodes
        // the blocks the items fall in, and touches nothing else.
        let mut sparse = vec![0u8; header.compressed.max(header.raw.len())];
        sparse[..header.raw.len()].copy_from_slice(&header.raw);
        for ((start, end), bytes) in ranges.iter().zip(&fetched) {
            if bytes.len() != end - start || *end > sparse.len() {
                return Err(format!("{key}: a block came back the wrong size"));
            }
            sparse[*start..*end].copy_from_slice(bytes);
        }

        parts
            .iter()
            .map(|(offset, length)| decode(&sparse, &header, *offset, *length))
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
            .map_err(|e| format!("{key}: {e}"))
    }
}

/// Decode `length` bytes from `offset` in a blosc buffer holding at least the
/// blocks they lie in.
fn decode(sparse: &[u8], header: &Header, offset: usize, length: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; length];
    let items = |bytes: usize| i32::try_from(bytes / header.typesize).ok();
    let (Some(start), Some(count)) = (items(offset), items(length)) else {
        return Err("range too large to decode".into());
    };
    // `sparse` is a whole blosc buffer — header, table and every block the
    // range needs — sized to the compressed length the header declares.
    let written = blusc::blosc1_getitem(sparse, start, count, &mut out);
    if written < 0 || written as usize != length {
        return Err(format!("blosc could not decode the block ({written})"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header of a real chunk of the reference stack, and the first
    /// entries of its table as read from the store.
    fn reference_header() -> Vec<u8> {
        let mut bytes = vec![2, 1, 0b0010_0001, 2];
        for word in [3_932_160u32, 262_144, 2_343_375] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut start = 16 + 15 * 4;
        for size in [
            176_279u32, 176_460, 182_510, 191_368, 190_035, 194_679, 199_439, 200_388, 205_057,
            203_753, 35_092, 34_234, 91_120, 131_610, 131_275,
        ] {
            bytes.extend_from_slice(&(start as u32).to_le_bytes());
            start += size as usize;
        }
        bytes
    }

    #[test]
    fn the_reference_chunk_has_one_block_per_eight_slices() {
        let bytes = reference_header();
        assert_eq!(Header::table_len(&bytes), Some(76));
        let header = Header::parse(&bytes).unwrap();
        assert_eq!(header.starts.len(), 15);
        assert_eq!(header.blocksize, 262_144);
        // Slice 31 of the second channel: one 32 KB plane inside one block.
        let plane = 128 * 128 * 2;
        let offset = (40 + 31) * plane;
        assert_eq!(
            offset / header.blocksize,
            (offset + plane - 1) / header.blocksize
        );
        let (start, end) = header.block_range(offset / header.blocksize);
        assert!(end - start < 250_000, "one block, not the chunk");
    }

    #[test]
    fn the_last_block_ends_where_the_chunk_does() {
        let header = Header::parse(&reference_header()).unwrap();
        let (_, end) = header.block_range(14);
        assert_eq!(end, 2_343_375);
    }

    #[test]
    fn a_blosc2_frame_is_left_to_zarrs() {
        let mut bytes = reference_header();
        bytes[0] = 5;
        assert!(Header::parse(&bytes).is_err());
    }
}
