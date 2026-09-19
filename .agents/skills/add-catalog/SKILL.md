---
name: add-catalog
description: 'Add a catalog of datasets to bevy-data-explorer: an implementation of Catalog listed in the dataset picker, and optionally a DescribeCells service that supplies real cell labels, colors and counts. Use when: listing datasets from a new portal, API or index; offering more datasets in the dropdown; giving a catalog''s datasets real cell properties.'
argument-hint: 'The catalog''s name, and the API or index it lists from'
---

# Add Catalog

A catalog answers one question: what is there to open? It lists names and
URLs, and nothing more. Opening an entry goes through `formats::discover` like
a typed URL, so **a catalog never names a format**, and a format never knows
which catalog listed it. When it's done properly the change touches
`src/catalog/<name>/` and one line in `main`.

## Step 1: Probe the API first

Query the live API and save one real response as a fixture before writing the
parser. Check:

- how it pages (cursor, offset), the largest page it accepts, and whether
  asking for more fails or truncates. BKP returns an error, not a short
  page;
- how it reports errors. A GraphQL error comes back as a 500 with the reason
  in the body, so read the body whatever the status is;
- whether its order is stable. BKP's isn't, so the entries are sorted;
- that every URL it hands out actually opens. See step 5.

If the entries point at a format the app doesn't read yet, that's a job for
`add-source-plugin`, not something to handle here.

## Step 2: Implement `Catalog`

Put it in `src/catalog/<name>/mod.rs` (or `<name>.rs` if it has no cell
service), modeled on `catalog/bkp/mod.rs`:

```rust
impl Catalog for MyCatalog {
    fn name(&self) -> &str { "My Portal" }   // shown in search and the log

    fn list(&self) -> BoxFuture<'static, Result<Vec<Entry>, String>> {
        Box::pin(list(self.endpoint.clone()))
    }
}
```

- `list` runs once, off the main thread, through `app::net`. Use
  `crate::app::net::client()` for HTTP rather than building a client.
- Keep parsing a plain function over the response text (`parse_page` in BKP),
  so it can be tested without the network.
- Bound any paging loop (`MAX_PAGES`), so a cursor that never ends can't
  keep a listing going forever.
- `Entry::name` is what people know the dataset by. `name_sources` renames an
  opened source to match, so a file whose address is an opaque id still shows
  its title. `kind` is the words shown beside it, and `keywords` is anything
  else worth searching on, such as the full title a short name abbreviates.
- One dataset with several views is one entry per view, each with its own URL.
- Return `Err` with a message that names the catalog. A failed catalog loses
  only its own entries and is logged, and it never blocks the others.

## Step 3: Register it

In `main.rs`, add it beside the others:

```rust
.add_catalog(catalog::bkp::Bkp::production())
.add_catalog(catalog::mine::MyCatalog::production())
.add_catalog(catalog::examples::Examples);
```

The registration order is the order in the dropdown. Each catalog keeps its own
slot, so a slow one never reorders those listed before it.

## Step 4 (optional): Describe its cells

If the catalog's service knows more about a dataset's cells than the files do
(labels for codes, colors, which properties matter, counts), implement
`DescribeCells` and put a `CellService` on each entry. `catalog/bkp/cells.rs`
is the worked example.

- `describe(columns)` gets `CellColumns`, the columns *the files* hold. Only
  those can be read, so leave out or hide any column the service knows and the
  files lack, and never invent one. Return a whole `CellProperties`. Build
  categorical values with codes that match the files. A taxonomy whose levels
  nest becomes one `PropertyKind::Tree`, and a numeric column becomes a
  `NumericRange` with the service's extent and a histogram if it has one.
- `count(properties)` is asked separately, after the labels are showing,
  because counting is slow. Return `CellCounts` keyed by column id and code.
  If some columns fail, return what did succeed, and fail only when nothing
  did.
- Answer `describe` quickly. Run independent queries together (`try_join`,
  `join_all`), and let a histogram that fails drop out with a `warn!` rather
  than failing the whole description.

Nothing else changes. `catalog/cells.rs` matches a source to its entry by
`SourceUrl`, whether it was opened from the dropdown, a typed URL, the command
line or a bookmark. It swaps out the format's placeholder properties and marks
the source `Described`. Bookmark restores already wait on `Described`. Don't
touch a format, the cell panel or the bookmark code for this.

A source matches only if its `SourceUrl` is **byte-for-byte** the entry's
`url`. If the service's URLs differ from what people paste (trailing slash,
scheme, CDN host), normalize them in the catalog, not in `cells.rs`.

## Step 5: Test

- Unit tests on the parser over a fixture string: every entry comes out, the
  last page has no cursor, and an API error is reported instead of being
  listed as nothing. BKP's three tests are the template.
- For a cell service: tests on the build step from fixture responses. Check
  the order, labels, colors, tree nesting and a numeric extent.
- A live test, `#[ignore = "reads the live <name> API"]`, that lists every
  entry and runs `formats::discover::discover` on each. This is what catches
  an entry the app can't open:

  ```sh
  cargo test every_live_entry -- --ignored --nocapture
  ```

  If the catalog has a cell service, add a second ignored test that describes
  one real dataset.

## Step 6: Verify

Use the `verify` skill. The log should show `<name>: N datasets`. Then open an
entry from the dropdown and check that its title replaces the address-derived
name. If it has a cell service, check that the log shows `asking <name> about
the cells of …` followed by `described N cell properties`.
