---
name: probe-dataset
description: 'Verify a data format against real bytes from the live store before writing a reader for it. Use when: adding a format, a new dataset variant, debugging a reader, or checking an assumption about chunking, codecs, file layout or metadata.'
argument-hint: 'A metadata URL, store root, or dataset description'
---

# Probe Dataset

Every format in this repo was pinned down by fetching real bytes first. That
practice has repeatedly contradicted what the metadata appeared to say, and
each time the cost of finding out afterwards would have been far higher.

Write no reader code until the probe answers the questions below.

## Step 1: Fetch the metadata and read it literally

Fetch the metadata document itself rather than trusting a copy that was pasted
into the conversation. Both differ from each other in practice.

Note what it does *not* say. The layout of the data files is usually absent:
the Scatterbrain endpoints need both a column name and a reference id appended,
which no field in the JSON states.

## Step 2: Find the real file layout

If a constructed URL 404s, list the bucket rather than guessing:

```sh
curl -s "https://BUCKET.s3.REGION.amazonaws.com/?list-type=2&prefix=PREFIX/&max-keys=25"
```

CloudFront strips query strings, so list against S3 directly. A 404 body often
names the key it looked for, which is worth reading.

## Step 3: Verify sizes against the declared counts

This is the strongest single check. A file of raw records has a length that is
exactly `count * stride`. If that identity does not hold, the stride, the
element type or the count is wrong, and finding out now is free.

## Step 4: Decode one real record

Fetch one chunk, node or tile and decode it by hand — in Python is fine.
Confirm:
- Values land inside the declared bounding box or window.
- The data is not uniformly zero, which is what a wrong offset looks like.
- Any checksum validates.

## Step 5: Check the structural assumptions

Ask explicitly, because these are the ones that have been wrong:

- **Is it additive?** Do the per-node counts sum to the dataset total, meaning
  children add detail, or does each level restate the whole thing?
- **What do the index bits mean?** Verify empirically by checking that a child's
  points fall in the quadrant its index implies.
- **Is compression what the metadata names?** Blosc's header stores a *library*
  id, not the compressor id its API uses, so a plausible lookup table mislabels
  zstd as zlib.
- **Is it sharded?** If chunks are large, look for an index in the object's
  tail; reading it takes one suffix range request and avoids downloading tens
  of megabytes per tile.

## Step 6: Keep the evidence

Save the metadata as a fixture under `testdata/` and write tests against it, so
the parsing is checked against bytes the store actually served rather than an
example written from memory.

## Reporting

State what was confirmed and what was assumed, with the numbers behind each.
Flag anything the probe could not settle.
