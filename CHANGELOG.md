# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Features

- **picker:** Build the frame dropdown lazily and pick a source from a select ([b420123](https://github.com/lanesawyer/bevy-data-explorer/commit/b420123e3841b1e2e7cde25d0b1739319485aa20))
- **ui:** Make settings pages and a table's record kinds tab strips ([957d2e7](https://github.com/lanesawyer/bevy-data-explorer/commit/957d2e7b0dd5e70b82da74bef4ee5bbc8b1a69f4))
- **colors:** Offer recent colors in Feathers' swatch grid ([9e0d091](https://github.com/lanesawyer/bevy-data-explorer/commit/9e0d0917dfaea0086add23e3a4f180a104173166))
- **theme:** Tell Feathers which surface each container is ([d6f35f4](https://github.com/lanesawyer/bevy-data-explorer/commit/d6f35f44eea22a6bb00268e308ae17e3330b2217))
- **schedule:** Shuffle systems within the stages on request ([4f63713](https://github.com/lanesawyer/bevy-data-explorer/commit/4f637133d2fad2a9dab34d222d0cb2d64a6b28ee))
- **colors:** Pick colors with Feathers' color input ([42f8253](https://github.com/lanesawyer/bevy-data-explorer/commit/42f8253a6d474a226686e8bc36e87193d8d4a5f3))
- **app:** Log a panic in a release build rather than ending the session ([f1bda89](https://github.com/lanesawyer/bevy-data-explorer/commit/f1bda897328ad3e6f9bac9889a5bd28eecd36835))
- **range:** Type or scrub either end of a numeric range ([b83cd51](https://github.com/lanesawyer/bevy-data-explorer/commit/b83cd513e90340066eba8736450e4c3518381744))
- **settings:** Scale the whole interface from 90% to 150% ([3639611](https://github.com/lanesawyer/bevy-data-explorer/commit/363961129a3b964cf105e658603253081b05acfe))
- **bookmarks:** Move bookmarks to a popover in the sidebar footer ([f7c8521](https://github.com/lanesawyer/bevy-data-explorer/commit/f7c8521b5df77ce188ada94e133d1c46fe8753ab))
- **picker:** One dataset picker everywhere a dataset is chosen ([09b185a](https://github.com/lanesawyer/bevy-data-explorer/commit/09b185a9f722b7c1b974617bb018520432f94393))

### Bug fixes

- **scrollbar:** Leave a Feathers list view its own bar ([d785c6b](https://github.com/lanesawyer/bevy-data-explorer/commit/d785c6bcd5e09666a5dbe765c76ca9cd0aa27510))
- **sidebar:** Keep the title and New frame button on one line ([b69b9aa](https://github.com/lanesawyer/bevy-data-explorer/commit/b69b9aad59a2de94aa9e67a3446290453a9a5420))
- **menu:** Open a popup leftward when there is no room to the right ([33e6bb4](https://github.com/lanesawyer/bevy-data-explorer/commit/33e6bb4fb22bca4c31335913116e08deae5cd464))

### Refactoring

- **menus:** Build every menu on Feathers' menu and popover ([c2e352d](https://github.com/lanesawyer/bevy-data-explorer/commit/c2e352dc0901e71706a331addd9d67a2a5883d01))
- **modals:** Build help, settings and the lightbox on Feathers' dialog ([d27d73f](https://github.com/lanesawyer/bevy-data-explorer/commit/d27d73f0d2c45daed8aab98eda0c9b749776cb70))
- **ui:** Attach control handlers to the controls with on() ([bcc7bd3](https://github.com/lanesawyer/bevy-data-explorer/commit/bcc7bd39cc76dd2eea891e805b31a6f193f06158))
- **view:** Spawn frame chrome with the frame and despawn it with it ([3bc66fe](https://github.com/lanesawyer/bevy-data-explorer/commit/3bc66fe5d9f8b2264a919ef2a2b2ea2fff49f947))
- **formats:** Share one read queue between tiles and octree nodes ([3b13440](https://github.com/lanesawyer/bevy-data-explorer/commit/3b1344000e16de500ec1343ac15311d635083493))
- **net:** Take a finished read out of its slot with landed() ([94683cd](https://github.com/lanesawyer/bevy-data-explorer/commit/94683cd0e33cf11f6ccb49e1056ebbe3dc7cc594))
- **ui:** Note what a list was built from on the list, and take singles as Single ([a2574ca](https://github.com/lanesawyer/bevy-data-explorer/commit/a2574caea7915759bae082286d2a29ec5deba8cf))
- **widgets:** Hold a drag's cursor with HoldsCursor, and drag docks by picking's drag events ([a61dafb](https://github.com/lanesawyer/bevy-data-explorer/commit/a61dafb5985c3021ff633c023cb35a51da029f7f))
- **accordion:** Keep a section in step only when it opens or closes ([4b2871d](https://github.com/lanesawyer/bevy-data-explorer/commit/4b2871d81937f9f7c6a00b68497a33e0f0c4fd1e))
- **schedule:** Say when a system has work with run conditions ([b059213](https://github.com/lanesawyer/bevy-data-explorer/commit/b059213d7085815738fb66d999d0a8b0a6107e3a))

### Maintenance

- **deps:** Upgrade to Bevy 0.20 ([e28038b](https://github.com/lanesawyer/bevy-data-explorer/commit/e28038b46b2b438f44f28092b63d2af78bd8a668))
## [0.6.0] - 2026-09-28

### Features

- **ui:** Mark every icon button that opens a menu with a chevron ([5699009](https://github.com/lanesawyer/bevy-data-explorer/commit/56990090e9ddfff55b9fb17a017ccc8f1e58c196))
- **view:** Draw a scale bar over frames measured in a length ([d0b3ebf](https://github.com/lanesawyer/bevy-data-explorer/commit/d0b3ebfdb696fc893d0cd8b68f38a718cbaf31a0))
- **table:** Filter CSV and Parquet tables from the sidebar ([7201b22](https://github.com/lanesawyer/bevy-data-explorer/commit/7201b22eb0d2198c24d4345657de14a2d0cf06c3))
- **view:** Link frames to pan, zoom and page together ([0d0aa8c](https://github.com/lanesawyer/bevy-data-explorer/commit/0d0aa8c4658f51655244b94e787f50d92dd1c2d2))
- **cli:** Open several datasets from the command line ([6d1e8cd](https://github.com/lanesawyer/bevy-data-explorer/commit/6d1e8cd917294376482b355c55211b845a876b9a))
- **formats:** Open Neuroglancer states as linked frames ([912527a](https://github.com/lanesawyer/bevy-data-explorer/commit/912527a17399bbfa87ea72e268e15d2fee0162e2))
- **ui:** Pick a channel's color ([03476ef](https://github.com/lanesawyer/bevy-data-explorer/commit/03476ef0bd24e4a0c843b98717769b2d05a13558))
- **image:** Overlay a Neuroglancer state's stores as one image ([9efaae7](https://github.com/lanesawyer/bevy-data-explorer/commit/9efaae736e881b95b4ded633f06e9e283ddc9bf0))
- **image:** Cut stacks in any plane, open Neuroglancer states where they look, and link across planes ([0d50517](https://github.com/lanesawyer/bevy-data-explorer/commit/0d50517354919ea8ed80aec0cb92ef79c26236c7))
- **view:** Open every view of a stack at once, with crosshairs and double-click ([2b32e83](https://github.com/lanesawyer/bevy-data-explorer/commit/2b32e838605a4d79e2a15c31eea58b88331bea7d))
- **view:** Offer the default plane as a way back ([c760eea](https://github.com/lanesawyer/bevy-data-explorer/commit/c760eeaa5c4fd77281d474a8b1e2c2379fb38c8c))
- **view:** Show a stack's three slices crossing in 3D ([4fadb8f](https://github.com/lanesawyer/bevy-data-explorer/commit/4fadb8f42b9b7eea585793d388f0b3d263c5654e))
- **catalog:** Offer a Neuroglancer state among the examples ([dc03920](https://github.com/lanesawyer/bevy-data-explorer/commit/dc03920afa1cd456e854e530bc06c7fc49c77d3d))
- **widgets:** Show status messages as notices, and refuse n5 and precomputed addresses by name ([2d18f58](https://github.com/lanesawyer/bevy-data-explorer/commit/2d18f58ebb1f6b72dc7255dad99e94cb85f2d277))
- **ui:** Set the sidebar's title and footer on bands of their own ([356520b](https://github.com/lanesawyer/bevy-data-explorer/commit/356520b17a4af7e2065a5f8d5abf5ce469eb9bcb))
- **ui:** Set the selection dock's title and count on a band, shared with the sidebar's ([800c1f8](https://github.com/lanesawyer/bevy-data-explorer/commit/800c1f81b93c0b75ec1c1969e30fd665ec03bb3c))
- **view:** Name the plane the way back returns to ([f447e1c](https://github.com/lanesawyer/bevy-data-explorer/commit/f447e1c1ed0d09cf55114ffd15291f1db8b7b0d1))
- **image:** Keep a slice on screen until the next one covers it ([34c969a](https://github.com/lanesawyer/bevy-data-explorer/commit/34c969a7d60c2d3e0982b262ec511b8d8ada3689))
- **image:** Keep downloaded chunks in a shared cache, so paging decodes rather than downloads ([94c01aa](https://github.com/lanesawyer/bevy-data-explorer/commit/94c01aabfc364c0fb6883883d13bb3a2b3b77243))
- **ui:** Read several datasets at once rather than one after another ([cd39e11](https://github.com/lanesawyer/bevy-data-explorer/commit/cd39e11fd7e9d3de15c9e35a7aa3b0ca21c163b0))
- **view:** Open every frame of all views at once, each waiting on its plane ([9ccb4ea](https://github.com/lanesawyer/bevy-data-explorer/commit/9ccb4ea8f08ee27135417c273d403fa08e0df394))
- **ui:** Step a stack one slice at a time from buttons beside its slider ([4ef99ef](https://github.com/lanesawyer/bevy-data-explorer/commit/4ef99ef06b7e7f668e6bf19b9e08626319035e14))
- **catalog:** Give data sources a dashboard on the home page, starting with the BKP Registry ([cc01ab2](https://github.com/lanesawyer/bevy-data-explorer/commit/cc01ab2fd7994ad52a53a1637d46e20e7a2763e1))
- **catalog:** Give the Brain Knowledge Platform a dashboard ([e3e0867](https://github.com/lanesawyer/bevy-data-explorer/commit/e3e086736fe24ba55e61d0e5332b64e6186e6b8f))
- **ui:** Show one dashboard at a time, and lead the home page with browsing ([5981612](https://github.com/lanesawyer/bevy-data-explorer/commit/59816122c4075322c3bf1d94f089c6c66020ed3b))
- **view:** Pick out a hovered cell's type in every dataset that shares its taxonomy ([3edb90d](https://github.com/lanesawyer/bevy-data-explorer/commit/3edb90d23950bae7104e6d793108003fe1233af9))
- **ui:** Settings to turn off type highlighting, or only its reach into other datasets ([d8a9bb6](https://github.com/lanesawyer/bevy-data-explorer/commit/d8a9bb6367d5d7c6f2315ed391e926780a99fc87))
- **table:** Show a table's kinds of record one at a time, starting with BICAN's specimen types ([6ae74a4](https://github.com/lanesawyer/bevy-data-explorer/commit/6ae74a43a29f70015a40a84bfa5710f78c3eee0e))
- **table:** Open a table on the columns its source shows by default ([1a14027](https://github.com/lanesawyer/bevy-data-explorer/commit/1a1402715f802109bc23be6d44f38f0a8188502c))
- **filters:** Lay specimen tables out as the platform does, and read filters only when opened ([fbe05ff](https://github.com/lanesawyer/bevy-data-explorer/commit/fbe05ff38ede639e4826a838f006ac9436bc1b84))
- **table:** Show a clicked row whole in the inspector, with copy and open buttons ([4851d72](https://github.com/lanesawyer/bevy-data-explorer/commit/4851d72a035cd8c4d348dfb857303d9fb12d41c1))
- **specimens:** Show a picked specimen's images and files in the inspector ([258e5c6](https://github.com/lanesawyer/bevy-data-explorer/commit/258e5c666d53bf183bd82746ded4e236c77ba8e7))
- **registry:** Open the Pre-Public Data Catalog's specimens, processes and data assets as tables ([1e94555](https://github.com/lanesawyer/bevy-data-explorer/commit/1e94555c7c6c20f5806462d4c5a673aac4a2744f))
- **table:** Search tables wherever all their rows can be searched ([33cbd68](https://github.com/lanesawyer/bevy-data-explorer/commit/33cbd68c17eb2cebdf1e61afb9c6afc130a7e3ae))
- **table:** Name the columns a table's search looks in ([fc5755f](https://github.com/lanesawyer/bevy-data-explorer/commit/fc5755fc12fe26ed5bb935de4f324cf0ed3910a5))
- **export:** Export a picked row and the table page on screen as CSV or JSON ([7d9803f](https://github.com/lanesawyer/bevy-data-explorer/commit/7d9803f26181b84d5210c873c35f8fe10920de89))
- **inspector:** Follow a BKP Registry record's links from the inspector ([15f648a](https://github.com/lanesawyer/bevy-data-explorer/commit/15f648abb36cad7f51183cdefabe0729e1c20452))
- **lineage:** Draw a BKP Registry record's lineage as a graph frame ([5bfef0f](https://github.com/lanesawyer/bevy-data-explorer/commit/5bfef0f5a98e488103ebcc8d3774a5a03c3c0aba))
- **lineage:** Point a lineage and the frames opened from it at each other ([a30c231](https://github.com/lanesawyer/bevy-data-explorer/commit/a30c23193598d2a9b79de34254cd7c055c02f52e))
- **welcome:** Offer one example of each kind of data, left-aligned ([703b392](https://github.com/lanesawyer/bevy-data-explorer/commit/703b392c0f7440c114cd5d4e803883a330da62fb))

### Bug fixes

- **image:** Read each level's own slice of a volume downsampled along z ([dcdd0a6](https://github.com/lanesawyer/bevy-data-explorer/commit/dcdd0a6e8c7324b48dcaae1ae55b1ca386e8265f))
- **image:** Read a chunk whole when its blosc blocks are out of order ([a95dd91](https://github.com/lanesawyer/bevy-data-explorer/commit/a95dd91b1e885080c587ad041b20668b23070480))
- **view:** Keep the plane button a tool button's size ([e4aece4](https://github.com/lanesawyer/bevy-data-explorer/commit/e4aece4bdf287356f96e7353b529cb33411d8908))
- **bookmark:** Keep the angle a cross-sections frame was turned to ([9f37d83](https://github.com/lanesawyer/bevy-data-explorer/commit/9f37d83ca622c28edd500861a0b2a63a6bcfc452))
- **image:** Give up on a stalled read rather than a slow one ([dae9b02](https://github.com/lanesawyer/bevy-data-explorer/commit/dae9b02badf390a3d7d0f22c4644921bab751f6b))
- **image:** Share one capped client per host across every store read from it ([da1212d](https://github.com/lanesawyer/bevy-data-explorer/commit/da1212d9701cdc41c87dcb1fcaad0b37eed035a8))
- **image:** Spin only while tiles on screen are loading, not while reading ahead ([1cff37f](https://github.com/lanesawyer/bevy-data-explorer/commit/1cff37fbe0d19981b2b646cbf473231ac4ecf1c2))
- **view:** Count empty frames when placing crosshairs and scale bars ([f679988](https://github.com/lanesawyer/bevy-data-explorer/commit/f679988eb27d879fefb697d8d12f204b77af26d2))
- **widgets:** Repaint sliders when the theme changes, so they wear the accent at once ([009ec8d](https://github.com/lanesawyer/bevy-data-explorer/commit/009ec8d9bc75094ceb57811423598d4075e3b7ca))
- **image:** Read ahead only once every frame has what it shows ([a86a88c](https://github.com/lanesawyer/bevy-data-explorer/commit/a86a88cf294ee6369f95f706946bb3c3cab3cc24))
- **formats:** Open a file:// address as the path it names ([7f9b7c4](https://github.com/lanesawyer/bevy-data-explorer/commit/7f9b7c4acca0606579686f10ff781df22e3d3f93))
- **catalog:** Ask again for a Registry dashboard refused before its token renewed ([0af1260](https://github.com/lanesawyer/bevy-data-explorer/commit/0af1260cef96fd4745aaa2d81fdf701d306e8717))
- **specimens:** Draw a numeric filter across the column's exact extent ([95b8997](https://github.com/lanesawyer/bevy-data-explorer/commit/95b89973e84da8c18d0d1d98f629d1afd505629b))
- **table:** Count the columns a search looks in rather than listing every one ([54e8240](https://github.com/lanesawyer/bevy-data-explorer/commit/54e824022d9a6ee217e8def58adbc2280cccecb0))
- **lineage:** Stop zoomed cards from clipping their own text ([4fe7b82](https://github.com/lanesawyer/bevy-data-explorer/commit/4fe7b82807ad21c7c4fcf6b3c595cc702b1c1843))
- **registry:** Report a refused link read as a refusal ([c9c1f64](https://github.com/lanesawyer/bevy-data-explorer/commit/c9c1f64fda1bba4cbfa7c5280cae2bdd4832b370))
- **scrollbar:** Keep an area scrolling both ways from showing a bar it does not need ([927194a](https://github.com/lanesawyer/bevy-data-explorer/commit/927194a536c265cd89c6d8b2dbadfb92387ddafc))

### Refactoring

- **app:** Share GraphQL answers, file reads and histogram edges ([03176ab](https://github.com/lanesawyer/bevy-data-explorer/commit/03176ab27d3ad0812982799d8b2fae18ee240382))
- **formats:** Drop the placeholder numeric ranges ([918c576](https://github.com/lanesawyer/bevy-data-explorer/commit/918c57671e5898310542a3ec03adc87f9f9acdbd))
- **app:** Decide remoteness in one place, counting s3 as remote and file as local ([82da055](https://github.com/lanesawyer/bevy-data-explorer/commit/82da05514bfdd480b87cabb78eb126176dfaffea))
- **image:** Move planes and the multi-store overlay out of dataset.rs ([6bf99a0](https://github.com/lanesawyer/bevy-data-explorer/commit/6bf99a03b4ec103dc25ecee7777c1fdf58ac6b0c))
- **catalog:** Split the platform's cell service into queries, filters, counts, histograms and building ([fa771f0](https://github.com/lanesawyer/bevy-data-explorer/commit/fa771f06a400a92beaf923f90ccd8dbad86ae1a2))
- **formats:** Split specimens into the query, its plan, filters, spans, recounts and paging ([ddfaced](https://github.com/lanesawyer/bevy-data-explorer/commit/ddfaced7bbe719053655f0b0ccc7f4ed51ca14ef))
- **view:** Split the table frame into layout, headings, resizing, rows, copying and paging ([cb53f2c](https://github.com/lanesawyer/bevy-data-explorer/commit/cb53f2c6198b9ffccee1facd33d334e64315e691))
- **ui:** Build the channels section as an accordion like the others ([75869f9](https://github.com/lanesawyer/bevy-data-explorer/commit/75869f9a5d2b3166a6d9b62f36697f4c303e23ab))
- Consolidate dock reserving, searched lists and table naming ([842774b](https://github.com/lanesawyer/bevy-data-explorer/commit/842774b2a964c145670d830ff25883cf6a4659de))

### Documentation

- Explain opening the unsigned macOS release ([8dca53f](https://github.com/lanesawyer/bevy-data-explorer/commit/8dca53fb8ec6e362d6a0f246f57433107cb642c0))
- Describe coloring points by a numeric property ([aafcdd7](https://github.com/lanesawyer/bevy-data-explorer/commit/aafcdd79473b77ee99f6417c1a8d89b9c1eabb89))

### Tests

- **ui:** Cover range dragging, the color picker, tree ticks and the untested panels ([5d93667](https://github.com/lanesawyer/bevy-data-explorer/commit/5d93667135949bb4cce9d3d069bda4bc1c3fc23f))
- **layers:** Check that no layer imports from one below it ([79369c2](https://github.com/lanesawyer/bevy-data-explorer/commit/79369c226af361abc5bec5794cf1471af5fa8efa))

### Maintenance

- Spell everything the American way ([8d82531](https://github.com/lanesawyer/bevy-data-explorer/commit/8d825315235c2ce6b8a7c3b324671904c157e642))
## [0.5.0] - 2026-09-25

### Features

- **theme:** Wear the desktop's accent color ([5fb478f](https://github.com/lanesawyer/bevy-data-explorer/commit/5fb478fd95d3688d7c3c9f787a052cbfa4627a54))
- **formats:** Read Parquet files as tables ([1287350](https://github.com/lanesawyer/bevy-data-explorer/commit/1287350281b0304c6b9bd173b8b36537c25fc7d1))
- **view:** Resize table columns by dragging and copy a cell from a hover button ([34beaa5](https://github.com/lanesawyer/bevy-data-explorer/commit/34beaa507442147404a15508c7e6db71684a9c71))
- **catalog:** Sign in to the BKP Registry through the browser ([01f9755](https://github.com/lanesawyer/bevy-data-explorer/commit/01f975594326a9d7c4861a1e92466c43b5f8e80d))
- **catalog:** Browse the BKP Registry before anything is typed ([b6bf48a](https://github.com/lanesawyer/bevy-data-explorer/commit/b6bf48a015b0deba22ef69a357f5cd7cab56b527))
- **ui:** Override the color of any value points are colored by ([d313f9e](https://github.com/lanesawyer/bevy-data-explorer/commit/d313f9ec1835c04d67054a2e4c26704e5b0a2fb7))
- **ui:** Mark a value's square when its color is overridden ([705e0bc](https://github.com/lanesawyer/bevy-data-explorer/commit/705e0bc8d107ab717464f957ddacd3b580ebf85d))
- **ui:** Export colors as CSV or JSON ([4058673](https://github.com/lanesawyer/bevy-data-explorer/commit/4058673d91298096bbad44b4434698760dddc601))
- **view:** Keep a frame's browser to one data source ([c7ea2cd](https://github.com/lanesawyer/bevy-data-explorer/commit/c7ea2cd30306f2331f31d58a5cfca6838e9e3158))
- **ui:** Choose the gradient numeric coloring is drawn along ([b397ef9](https://github.com/lanesawyer/bevy-data-explorer/commit/b397ef9c8dee705a19d05af8c6e5ec19215c2cd0))

### Refactoring

- **ui:** Write chrome's nodes and text only when they differ ([d324c39](https://github.com/lanesawyer/bevy-data-explorer/commit/d324c3940c33d369bc80b619f126903a8153b918))
- **ui:** Find the selected frame's source through one SelectedSource ([8bec9d3](https://github.com/lanesawyer/bevy-data-explorer/commit/8bec9d3ff510c5eb711c7d1f724774bcd22239e8))
- **ui:** Size and place both right-hand docks through DockWidth ([6777e96](https://github.com/lanesawyer/bevy-data-explorer/commit/6777e9687eee5a156a16f3a4c225d5d7155b334a))
- **ui:** Keep both searchable value lists through one SearchedRows ([21241dd](https://github.com/lanesawyer/bevy-data-explorer/commit/21241ddf5b11f35910af7eaac2090fde69e726e3))
- **formats:** Answer hover for image, Deep Zoom and SVG through one system ([7c85455](https://github.com/lanesawyer/bevy-data-explorer/commit/7c854557fea530293a7eecf8d0d2ad604f158c2a))
## [0.4.0] - 2026-09-21

### Features

- **catalog:** Search the BKP Registry for OME-Zarr stores ([d57e6ed](https://github.com/lanesawyer/bevy-data-explorer/commit/d57e6ed66e71a9dcd0002fb2c9a96acf4753154b))
- **ui:** Find datasets in an empty frame ([6cc9935](https://github.com/lanesawyer/bevy-data-explorer/commit/6cc9935e3020c581b2d4ac943cf416dd18ef6787))
- **ui:** Open the bookmarks section by default ([9c2e998](https://github.com/lanesawyer/bevy-data-explorer/commit/9c2e99887fae52dc332b8d655b6d8897b86e65c6))
- **settings:** Split settings into pages and let data sources be turned off ([69010bd](https://github.com/lanesawyer/bevy-data-explorer/commit/69010bd564a06e27cf7a4a4420d45d99d252521c))

### Bug fixes

- **prefs:** Keep preferences in %APPDATA% on Windows ([494c33f](https://github.com/lanesawyer/bevy-data-explorer/commit/494c33fa74640c89980b2bf5e16a0eea1e7353b4))
- **ui:** Keep scrollbars in the padding, clear of the edge ([cbea028](https://github.com/lanesawyer/bevy-data-explorer/commit/cbea0286a0a01d1f4610bb66dd727ace8744769f))
- **catalog:** Point a rejected BKP Registry token to the settings ([9581ccc](https://github.com/lanesawyer/bevy-data-explorer/commit/9581ccc652cc7d0a59a38b18245daf3791551986))

### Refactoring

- **ui:** Let the selected source decide which sections show ([6bda190](https://github.com/lanesawyer/bevy-data-explorer/commit/6bda190054158a21e9f0adf9f57bc8113d2bb107))
## [0.3.0] - 2026-09-21

### Features

- **ui:** Add a help screen and attribution on the welcome screen ([2d01a31](https://github.com/lanesawyer/bevy-data-explorer/commit/2d01a31b88e73a3884119c9ccd5fcfa33eb093d9))
- **view:** Reorder a frame's layers from the sidebar and its menu ([eb6aee0](https://github.com/lanesawyer/bevy-data-explorer/commit/eb6aee06cfe887b5f3493530f013400c0b36827a))
- **view:** Offer every catalog's datasets as layers ([a40c9b3](https://github.com/lanesawyer/bevy-data-explorer/commit/a40c9b3ee5af3988deed145b1b5afa72d2026a82))
- **ui:** Switch sections between grid and single slice from the sidebar ([4317d61](https://github.com/lanesawyer/bevy-data-explorer/commit/4317d6138cd33d76906ed487e399a95d6dbd4ac9))
- **ui:** Label the sidebar's footer buttons, hiding the words in the ribbon ([1538049](https://github.com/lanesawyer/bevy-data-explorer/commit/1538049bc753e7a6b31f335083f57ba5adb59eb1))
- **catalog:** Describe BKP datasets' cells from the platform's API ([69efc43](https://github.com/lanesawyer/bevy-data-explorer/commit/69efc43cb804ebff2939f934d5f5ec122db6f57b))
- **ui:** Filter taxonomies as trees, colored by a chosen level ([57aab2d](https://github.com/lanesawyer/bevy-data-explorer/commit/57aab2db3673790bb946eefe4c656160fd028f47))
- **bookmark:** Save, restore and share what is on screen ([70aa686](https://github.com/lanesawyer/bevy-data-explorer/commit/70aa686f1c5a5295a2540e856523ac66e24c41af))
- **ui:** Color points by numeric properties along a gradient ([8613d4b](https://github.com/lanesawyer/bevy-data-explorer/commit/8613d4b30e4adb55c15a78b3ac4ccaa679aa005c))
- **ui:** Search for genes and color and filter by their expression ([93c0161](https://github.com/lanesawyer/bevy-data-explorer/commit/93c0161b81296ff652517fa035e6b74145748583))
- **ui:** Polish accordions and text fields, and offer BKP examples ([2fb29e5](https://github.com/lanesawyer/bevy-data-explorer/commit/2fb29e5d1d4078d9332f42de1c7ad21bf2a59597))
- **bookmark:** Rename saved bookmarks ([897d864](https://github.com/lanesawyer/bevy-data-explorer/commit/897d86423ad0ed54bf3d373ef6c0ceb6db5d329b))
- **ui:** Add settings, and remember panel sizes and theme ([a0dfe74](https://github.com/lanesawyer/bevy-data-explorer/commit/a0dfe7488e6ea59d84c285d22445e0ba2c548095))
- **ui:** Make the log selectable, and timestamp its lines ([1be04c8](https://github.com/lanesawyer/bevy-data-explorer/commit/1be04c87f7f16c07c0d97423af4e38a86c1a29b5))
- **points:** Draw filtered-out points, with a color picker and default ([3ca0d42](https://github.com/lanesawyer/bevy-data-explorer/commit/3ca0d42dd571468074a6e5cc602e10ce47ed74c8))
- **cells:** Recount values and histograms under the current filters ([7987967](https://github.com/lanesawyer/bevy-data-explorer/commit/798796713a8cce712c30e923dbfe05a178e7e11f))
- **cells:** Count, slide, pick and swap on numeric ranges ([d5bc962](https://github.com/lanesawyer/bevy-data-explorer/commit/d5bc9627db003c05bf0f1dcfd632b783c29c1e16))
- **cells:** Search and scroll long value lists, with skeletons while loading ([b5b6d03](https://github.com/lanesawyer/bevy-data-explorer/commit/b5b6d03b8fba9117a1859cecadd06ee25ec445c1))
- **cells:** Say when a service fails to describe cells, and offer a retry ([f7353c9](https://github.com/lanesawyer/bevy-data-explorer/commit/f7353c924e305e8e161c2b5b5b63d7c21c8d5ed1))
- **selection:** Summarize the cells inside a dragged rectangle ([db830fc](https://github.com/lanesawyer/bevy-data-explorer/commit/db830fc49e84892307c746bd27fdfe784d41ef98))
- **widgets:** Give every scrolling area a scrollbar ([77f2e58](https://github.com/lanesawyer/bevy-data-explorer/commit/77f2e58607e307a89f167b4f0fe8beb2ac5d2191))
- **cellpanel:** Show what each value is made of in the colors on screen ([d47a543](https://github.com/lanesawyer/bevy-data-explorer/commit/d47a543d03bd74ccb045f264cbe2ed72bfa7e69d))
- **table:** Show CSV and TSV files as a table filling its frame ([ff74f56](https://github.com/lanesawyer/bevy-data-explorer/commit/ff74f56e975c1ad113bb0d222c97486d794a31d8))
- **specimens:** Read a BKP project's specimens as a table ([7282399](https://github.com/lanesawyer/bevy-data-explorer/commit/728239957cc30028f5ce394f5794df9c3f9f4354))
- **catalog:** Offer every BKP project whose specimens are a table ([8a9165c](https://github.com/lanesawyer/bevy-data-explorer/commit/8a9165c8dbc087075a5b464f19cd3dd2ccdc451b))
- **table:** Read a table a page at a time ([2ae9aeb](https://github.com/lanesawyer/bevy-data-explorer/commit/2ae9aeb8d92095d5025168b7721f0979eb7927c0))
- **table:** Narrow a table's rows from the sidebar ([234940c](https://github.com/lanesawyer/bevy-data-explorer/commit/234940c6ecc7db0ae8b36bc7e3bdcd0e9a1d7f5d))
- **table:** Narrow a column of numbers by a span of it ([adcb1a0](https://github.com/lanesawyer/bevy-data-explorer/commit/adcb1a095d1d9b7318b78cdda5a0978804e3eac9))
- **table:** Filter a table the way cell properties are filtered ([9b04afd](https://github.com/lanesawyer/bevy-data-explorer/commit/9b04afdd16a472c1e483f6da56cb5abef0551883))
- **bookmark:** Save a table's page and filters ([671b3f2](https://github.com/lanesawyer/bevy-data-explorer/commit/671b3f25c495f2a052150d90c98a547087b369cb))
- **table:** Count a table's filters under the filters in force ([6c89bd9](https://github.com/lanesawyer/bevy-data-explorer/commit/6c89bd9b35d48f45b129a218a10fa253cc66674b))
- **table:** Sort by headings and pick which columns are drawn ([ef1386b](https://github.com/lanesawyer/bevy-data-explorer/commit/ef1386b0e1e8ea058ebfa4b92395ce97fc239227))
- **ui:** Polish the sidebar, settings and welcome screen ([e72461f](https://github.com/lanesawyer/bevy-data-explorer/commit/e72461fd8cbd9451a879e7b1e6de32ae9f1eb834))
- **ui:** Hold new frames when the grid is full, and more polish ([167ce35](https://github.com/lanesawyer/bevy-data-explorer/commit/167ce35aeaf15663eeadc3f7ff97b71e6603bfcb))
- **ui:** Center modals over the frames and list a specimen table as BKP ([dce5a69](https://github.com/lanesawyer/bevy-data-explorer/commit/dce5a6958e9835034fbd46ba678b22c9cd7f675e))

### Bug fixes

- **image:** Pass is_reading directly rather than through a closure ([3df0560](https://github.com/lanesawyer/bevy-data-explorer/commit/3df0560d4178ad9ffeb076e68ee414d14e45694e))
- Satisfy clippy's midpoint and contains lints ([15d788a](https://github.com/lanesawyer/bevy-data-explorer/commit/15d788a5b806cc663128a89d1d0c10309d473596))
- **ui:** Size labels with TextFont, which Feathers labels do not ignore ([b948ed7](https://github.com/lanesawyer/bevy-data-explorer/commit/b948ed7c527761f1ef0c64906b5ff2cc4b5c2d2e))
- **cellpanel:** Show a value's color only while it is the coloring ([fed14dc](https://github.com/lanesawyer/bevy-data-explorer/commit/fed14dc65529fcb81446e8fed7541679c98c7fe1))
- **table:** Offer every column the platform can group by ([a5ae82e](https://github.com/lanesawyer/bevy-data-explorer/commit/a5ae82e38d2a55eca6d9c9db612487c2600ba678))
- **table:** Select a table's frame when the table is pressed ([48a433a](https://github.com/lanesawyer/bevy-data-explorer/commit/48a433ac0540d79e98387dfe9c67c5b09c19ccae))
- **table:** Count a column's values under its own filter too ([5f4f620](https://github.com/lanesawyer/bevy-data-explorer/commit/5f4f620f7ebf424e117c50569178206bd6469a46))
- **table:** Never layer a table or layer anything over one ([0d60998](https://github.com/lanesawyer/bevy-data-explorer/commit/0d60998d8013710a62cf8593598cc8b16c3fc5b9))
- **ui:** Line up the frame buttons and steady the bookmark rows ([2eda1df](https://github.com/lanesawyer/bevy-data-explorer/commit/2eda1df11dbd9c4d8d08ec961a5f29113ac7c9d2))

### Refactoring

- **formats:** Stream Zarr and Deep Zoom tiles through one shared cache ([2ad9c8e](https://github.com/lanesawyer/bevy-data-explorer/commit/2ad9c8edbe63971ec799ade36819fd8144dbe973))
- **formats:** Keep both Scatterbrain streamers' nodes in one NodeCache ([850ab73](https://github.com/lanesawyer/bevy-data-explorer/commit/850ab734be7b691d16642f580b5b37db1ab1bfc9))
- **slices:** Page sections through the source's SliceStack ([e401fe6](https://github.com/lanesawyer/bevy-data-explorer/commit/e401fe6967b478c1e148a52cf8bcd568401f92af))
- **ui:** Drag the sidebar, inspector and log panel through one Dock ([00940e0](https://github.com/lanesawyer/bevy-data-explorer/commit/00940e016a64cdc5ffe37f10745ffbdef3a9a6ff))
- **render:** Apply source display settings from one module ([f6d0a6d](https://github.com/lanesawyer/bevy-data-explorer/commit/f6d0a6df84f1b9e67c455380767e079376a864a3))
- Spell colour as color throughout ([eb2d2d1](https://github.com/lanesawyer/bevy-data-explorer/commit/eb2d2d11c80a6c0ef72591e2ed2c7892124d135a))
- Keep ShowsSource in source and BlocksFrameInput in widgets ([182a48b](https://github.com/lanesawyer/bevy-data-explorer/commit/182a48b2e93508a5e5c35d0643d8f6a3a513edb2))
- **view:** Toggle channels by number key with the other frame keys ([e91d8d2](https://github.com/lanesawyer/bevy-data-explorer/commit/e91d8d2c25520cff9c09080c6be89112abe12d1b))
- **widgets:** Give every widget a file of its own ([42f08f3](https://github.com/lanesawyer/bevy-data-explorer/commit/42f08f3edad70a1d523a35bdf0c51250551229b4))
- **widgets:** Size every piece of text from one named scale ([7818cf7](https://github.com/lanesawyer/bevy-data-explorer/commit/7818cf7bd0aa233be4828f0badb7c765c7468f18))
- Tidy the table code and name ui modules with underscores ([148be16](https://github.com/lanesawyer/bevy-data-explorer/commit/148be1635d7a81cdaeaa3b4631351a5525d70efb))
- **ui:** Name every spacing from one scale ([49de8e8](https://github.com/lanesawyer/bevy-data-explorer/commit/49de8e882f1eadd64a048df2f2de6d7ad33e834a))

### Documentation

- **agents:** Add catalog, bookmark-setting and ui-control skills ([5d57049](https://github.com/lanesawyer/bevy-data-explorer/commit/5d57049617c5133e9f0b4de1a0d32f10b5af8438))
- **agents:** Run clippy in the verify loop, as CI does ([49e447a](https://github.com/lanesawyer/bevy-data-explorer/commit/49e447a57d9b527576024c7b03031ff411daed92))
- **agents:** Describe the layers in the order they import, and app's two halves ([e31f428](https://github.com/lanesawyer/bevy-data-explorer/commit/e31f4284335668bf676d3a296a7ea9718d3df50c))

### Maintenance

- **formats:** Drop key hints and the channel list from status lines ([dcaf67b](https://github.com/lanesawyer/bevy-data-explorer/commit/dcaf67b5e6e1c1f7f154690e2fb3e329b372b51c))
## [0.2.0] - 2026-09-18

### Features

- Stream OME-Zarr images with a Bevy viewer ([aaeebd7](https://github.com/lanesawyer/bevy-data-explorer/commit/aaeebd785795811465ae5d65c9da059ced12af10))
- Add Scatterbrain point clouds in a second panel ([d3b6c69](https://github.com/lanesawyer/bevy-data-explorer/commit/d3b6c69642e3c0afa66bb29047082919c2884d94))
- Add sectioned Scatterbrain datasets as a third panel ([bb0d875](https://github.com/lanesawyer/bevy-data-explorer/commit/bb0d875de86ed3c32caeb435f7ae98658bdb699b))
- **panel:** Duplicate panels into a 4x2 grid ([6ba3824](https://github.com/lanesawyer/bevy-data-explorer/commit/6ba3824fd24daa3db73ebdee9a1e26eaacef5205))
- **panel:** Add a close button alongside duplicate ([5024bdc](https://github.com/lanesawyer/bevy-data-explorer/commit/5024bdcf47ea7f31edc98f8d01d6d6b292b6568b))
- Add a resizable, collapsible sidebar ([e571436](https://github.com/lanesawyer/bevy-data-explorer/commit/e57143642196e07238c4e13e0dda934e05fba06c))
- **sidebar:** Show a resize cursor over the drag handle ([da76038](https://github.com/lanesawyer/bevy-data-explorer/commit/da760383eb610df82ebc1360d78822680715d031))
- Add sidebar accordions with a view configuration section ([3ed29c1](https://github.com/lanesawyer/bevy-data-explorer/commit/3ed29c11299fc164d7a898b0166b728c6166e559))
- Add an edit layout menu, built on Feathers widgets ([8e8077d](https://github.com/lanesawyer/bevy-data-explorer/commit/8e8077d3f9e7c70a6dfa03429fd52d13eb68b121))
- Scroll long menus, and start maximized ([4deb8ec](https://github.com/lanesawyer/bevy-data-explorer/commit/4deb8ec4e489521c7b513d91437eb2de77c14f4b))
- **sidebar:** Move the collapse toggle to the bottom ([dbe139b](https://github.com/lanesawyer/bevy-data-explorer/commit/dbe139b8c0fe898c8425c3c942f3a3ff03bf8923))
- Size points, with a control per point-cloud source ([8328d0f](https://github.com/lanesawyer/bevy-data-explorer/commit/8328d0f4642dfe02887d0c6ec280d030eedd369f))
- Add a docked inspector, opened from a frame's info button ([0333d39](https://github.com/lanesawyer/bevy-data-explorer/commit/0333d396a1ba1d1f85dec6b740998bbc6d75624c))
- Add a cell properties section with colouring and filters ([03e455c](https://github.com/lanesawyer/bevy-data-explorer/commit/03e455ca04f4123cd7a2126d63244c3cfe7f2434))
- Add the SEA-AD dataset, with numeric properties and histograms ([442468f](https://github.com/lanesawyer/bevy-data-explorer/commit/442468f27f7d01c9601ecc02b8dbabe0b7ca17b9))
- **cellpanel:** Pick values to filter by, and offer to clear them ([23cca25](https://github.com/lanesawyer/bevy-data-explorer/commit/23cca252ea1339fb51d2403f25b64e283025549d))
- **sidebar:** Show the build version in the footer ([86e2bc8](https://github.com/lanesawyer/bevy-data-explorer/commit/86e2bc8d903c0e5998a780b1bb2cc083abd0eb8a))
- Give each frame a header naming its dataset ([81a827d](https://github.com/lanesawyer/bevy-data-explorer/commit/81a827d16a9b78d879a815acbf55ab199288f7fc))
- **widgets:** Truncate accordion titles, and widen the sidebar ([cb018ac](https://github.com/lanesawyer/bevy-data-explorer/commit/cb018ac7b14d3089b7edc08e21dd4d70cff5344b))
- **sidebar:** Raise the minimum width ([1e070a3](https://github.com/lanesawyer/bevy-data-explorer/commit/1e070a39b9457657f8c274c478abf6d5433d1858))
- **hover:** Identify what is under the pointer, and enlarge cells sharing its value ([54096c1](https://github.com/lanesawyer/bevy-data-explorer/commit/54096c1f5777f8a45d68a8cb4f04dc3d35bdde36))
- **cellpanel:** Pick which cell properties the panel lists ([3da3374](https://github.com/lanesawyer/bevy-data-explorer/commit/3da3374ddeb1fa59e9ded0c8f6288dd8cf622ce1))
- **ui:** Open a dataset by URL, recognising its format ([ec41a2b](https://github.com/lanesawyer/bevy-data-explorer/commit/ec41a2b0e0766e2873021be9ee190937302593d3))
- **image:** Open Zarr v2 stores alongside v3 ([2c8c9f4](https://github.com/lanesawyer/bevy-data-explorer/commit/2c8c9f4244d1c6261cc9e57402903d5e0e32174f))
- **ui:** Start empty, and offer a way in ([c918e31](https://github.com/lanesawyer/bevy-data-explorer/commit/c918e31e728c294989cf3054913da1ece9be360d))
- **view:** Save a picture of a frame ([26b2762](https://github.com/lanesawyer/bevy-data-explorer/commit/26b276292423ddfa8d5635f82542078803471626))
- **app:** Follow the desktop's theme, and offer the other one ([f40ed55](https://github.com/lanesawyer/bevy-data-explorer/commit/f40ed5585461ec14d24f31d150202f634391e9db))
- **ui:** Show the log in the app, and say what the user just did ([9cf252d](https://github.com/lanesawyer/bevy-data-explorer/commit/9cf252db77e0ee5291ffb8590e862cb769cf597a))
- **ui:** Offer every dataset the app knows, and nothing when nothing is selected ([5c32862](https://github.com/lanesawyer/bevy-data-explorer/commit/5c3286232a84382708f633fca50327e20662335c))
- **image:** Page through a volumetric image ([1916ee0](https://github.com/lanesawyer/bevy-data-explorer/commit/1916ee0bcb0ab3f05debfc868d2ecb28bf11cd25))
- **ui:** One list of datasets, opened rather than fetched twice ([bd1a9ff](https://github.com/lanesawyer/bevy-data-explorer/commit/bd1a9ff7545d6ecf6c50dcd456193b2560220ccb))
- **formats:** Open Deep Zoom images, and load nothing off screen until the view has landed ([c5a74de](https://github.com/lanesawyer/bevy-data-explorer/commit/c5a74dedbbe3195130bb96a18e6e28697d580993))
- **view:** Stack datasets as layers in a frame, and draw SVG annotations over slides ([9dd654f](https://github.com/lanesawyer/bevy-data-explorer/commit/9dd654fa395afaa0ba00940ccf1127c71ee8c1a9))
- **view:** Layer any known dataset without opening it first, and tidy the frame menu ([0733e5d](https://github.com/lanesawyer/bevy-data-explorer/commit/0733e5d49fd965887a4f650cc0a8a2da028285b2))
- **view:** Make layers truly see-through by drawing each into its own image ([a4fd1cb](https://github.com/lanesawyer/bevy-data-explorer/commit/a4fd1cbd49daa73d39bdeb0c159694e18c4fac40))
- **view:** Switch a frame's dataset from a searchable dropdown on its title ([c9ecc96](https://github.com/lanesawyer/bevy-data-explorer/commit/c9ecc9611183fdec4534a1d28fed4afb61ad4e32))
- **view:** Look at image stacks in 3D where the data has real depth ([0a8e1bd](https://github.com/lanesawyer/bevy-data-explorer/commit/0a8e1bdd2f1de356ffec73a8fcfabcc8218d1c34))
- **view:** Read finer detail for what 3D frames see, and zoom to the pointer ([7692215](https://github.com/lanesawyer/bevy-data-explorer/commit/7692215a63d9c9068f16b0a90e7cad931cca2b18))
- **ui:** Channel controls in View configuration, mixed on the GPU ([7174f04](https://github.com/lanesawyer/bevy-data-explorer/commit/7174f049ff6abadb1b1ebf38a3953997ce4bac30))
- **catalog:** List Brain Knowledge Platform datasets in the dataset picker ([a9de85f](https://github.com/lanesawyer/bevy-data-explorer/commit/a9de85ffaae43423389d867ee87cc428d93d3d18))
- **ui:** Draw button icons from the Lucide icon font ([d060c81](https://github.com/lanesawyer/bevy-data-explorer/commit/d060c81ae86e8f3536ddc685949e7f3d1c66629a))
- **view:** Sweep a loading bar across frames that are fetching ([e88140c](https://github.com/lanesawyer/bevy-data-explorer/commit/e88140c8944c10126c2a3520364fe5203bea3ab9))
- **ui:** Drag the log panel taller by its top edge ([9a8786b](https://github.com/lanesawyer/bevy-data-explorer/commit/9a8786b057282f8d9dc989208b1839adaab59404))

### Bug fixes

- **panel:** Keep a drag in the panel it started in ([e3471a7](https://github.com/lanesawyer/bevy-data-explorer/commit/e3471a7e885c0b6f7f6cfe814ede15a626cfb011))
- **widgets:** Align the slider thumb with its fill ([8c11176](https://github.com/lanesawyer/bevy-data-explorer/commit/8c1117617ee6b0a2aa48eb6efc4165cba3c54551))
- **viewconfig:** Fade by dimming instead of alpha ([8b18a3b](https://github.com/lanesawyer/bevy-data-explorer/commit/8b18a3bf9a933342261cfe89b669ebeeeab88586))
- **panel:** Draw the selection outline above the rules between frames ([9197cfb](https://github.com/lanesawyer/bevy-data-explorer/commit/9197cfb400e6fee5fbaf47b7888946d54c630308))
- **viewconfig:** Make the menu's buttons work, and render their glyphs ([366a3ec](https://github.com/lanesawyer/bevy-data-explorer/commit/366a3ec91080db9312b03b2afcf4b0c337a88594))
- **widgets:** Make the Feathers slider redraw as it is dragged ([b97203b](https://github.com/lanesawyer/bevy-data-explorer/commit/b97203b73fe38f2181ca2fe00d71ab1d9a49edcb))
- **widgets:** Keep the menu open, so its buttons can be pressed ([f445c43](https://github.com/lanesawyer/bevy-data-explorer/commit/f445c436ddd738a3bb96095aa56a05de9a40bb84))
- **panel:** Restore clearing and unique cells after closing frames ([649636a](https://github.com/lanesawyer/bevy-data-explorer/commit/649636a737f726b9b5ace22ffb4353981bb1a058))
- **slices:** Frame a newly opened sections view to the whole grid ([2c9cba5](https://github.com/lanesawyer/bevy-data-explorer/commit/2c9cba58b9c0ff412fb2b5660d184deb3ef6e986))
- **points:** The size setting was a radius, not a diameter ([2753024](https://github.com/lanesawyer/bevy-data-explorer/commit/275302468b280f1ae24302caaa4a3e149b7ebdf0))
- **slices:** Show every slice in the grid again ([2ff7068](https://github.com/lanesawyer/bevy-data-explorer/commit/2ff706822ae6d7fb2c31d33925ec797145108329))
- Stop the inspector driving the frame behind it, and from closing by drag ([b701fe3](https://github.com/lanesawyer/bevy-data-explorer/commit/b701fe398c5718c00decf3f463fafb98a64e8082))
- **widgets:** Do not draw a closed section before hiding it ([578c9b9](https://github.com/lanesawyer/bevy-data-explorer/commit/578c9b9e3f1e318837c5893c842f12d0f37fe8ed))
- **cellpanel:** Let a numeric range be dragged, not nudged ([2b58293](https://github.com/lanesawyer/bevy-data-explorer/commit/2b58293a1e8aa1dc2c62964cdfa96a6a2b02f7de))
- **cellpanel:** Do not despawn a rebuilt entity twice ([c1b9e2b](https://github.com/lanesawyer/bevy-data-explorer/commit/c1b9e2bb2938386f7b8b348c26c8779edf8aed10))
- **panel:** Stop decorative overlays swallowing pointer events ([97e4f3f](https://github.com/lanesawyer/bevy-data-explorer/commit/97e4f3f200d15872cf8cbb8d6c5d1a9eaacb7bae))
- **cellpanel:** Show the per-property clear button, and put clearing first ([18252f1](https://github.com/lanesawyer/bevy-data-explorer/commit/18252f1a325490eca73d9fae67a6f5369f451808))
- **cellpanel:** Actually spawn the per-property clear button ([7e36a79](https://github.com/lanesawyer/bevy-data-explorer/commit/7e36a79d52a07f1f667af305bccba178a6304824))
- **cellpanel:** Stop the checkboxes flashing ticked on every click ([8ca63d6](https://github.com/lanesawyer/bevy-data-explorer/commit/8ca63d6d8ea8f331e5f00544ab04a938c4b06ef6))
- **formats:** Keep the last picture until the new one is whole ([f2cdb0e](https://github.com/lanesawyer/bevy-data-explorer/commit/f2cdb0ec89592fd63d10a040ab2f5cc7b89f796a))
- **formats:** Spawn nothing for a node the filters emptied ([cf9e8a3](https://github.com/lanesawyer/bevy-data-explorer/commit/cf9e8a35af10e29fe36e81f2505760922586e011))
- **ui:** Put a slider where the track is clicked, and stop offering invented ranges ([60bed50](https://github.com/lanesawyer/bevy-data-explorer/commit/60bed50555303387f31829dd11578ca385c8c5d3))
- **view:** Keyboard shortcuts act on the selected frame only ([b99f42d](https://github.com/lanesawyer/bevy-data-explorer/commit/b99f42d6a8ef25e82876813794c66d354607b92f))
- **image:** Name an image after its store when OME names it "/" ([af98613](https://github.com/lanesawyer/bevy-data-explorer/commit/af986130e549e31a89b82ee34c376e45ec53ea0d))
- **view:** Stop a layer's first resize from quitting the app ([92191fe](https://github.com/lanesawyer/bevy-data-explorer/commit/92191fe3f656d2d0e72bca98403b09c47f033f5a))
- **ui:** Keep the channel reset from shifting the layout ([9bc4ce9](https://github.com/lanesawyer/bevy-data-explorer/commit/9bc4ce9b9043e0e5b7c3dbb086129730808d9d7c))

### Performance

- **image:** Read one slice of a deep chunk block by block ([390bd6e](https://github.com/lanesawyer/bevy-data-explorer/commit/390bd6e1709bef356e955b017db6a7ec878ac78a))

### Refactoring

- Make each data source a Bevy plugin ([03d7f98](https://github.com/lanesawyer/bevy-data-explorer/commit/03d7f986a70ae6c0ab7947d901fda530f36a72df))
- **panel:** Spawn button children with the children! macro ([b10440f](https://github.com/lanesawyer/bevy-data-explorer/commit/b10440f9090490264b71af318259e32c0c0c55a7))
- Declare frames and UI with BSN ([23ca15b](https://github.com/lanesawyer/bevy-data-explorer/commit/23ca15b72ebd4c9eb8b77c31cf1e0e48f13b8ca3))
- Put every button on Feathers, and make its hover visible ([8b9f972](https://github.com/lanesawyer/bevy-data-explorer/commit/8b9f97219f6facace628445b1962424747f6ebfd))
- Stop drag targets pretending to be buttons ([5a09a36](https://github.com/lanesawyer/bevy-data-explorer/commit/5a09a3603efe5a4db152cb69fa3f41dde1e23917))
- Organize modules by feature into a nested tree ([416b1e8](https://github.com/lanesawyer/bevy-data-explorer/commit/416b1e8cf2e1355791502041b8aaeff60a1cf2dd))
- **schedule:** Name the frame's stages in one place ([4ed661b](https://github.com/lanesawyer/bevy-data-explorer/commit/4ed661b4d501dc1d228b93ee5a8ba6b733e78bab))
- **plugins:** Give every module its own plugin ([212a660](https://github.com/lanesawyer/bevy-data-explorer/commit/212a6602bbcc04c843d276442194b9c0b91837a3))
- **app:** Reduce main to parsing arguments and adding format plugins ([0e5710a](https://github.com/lanesawyer/bevy-data-explorer/commit/0e5710a45dbe1a8eb5a24c4fdd712c4981d4ad0d))
- **sources:** Make every streamer a component of its source entity ([f27ffe4](https://github.com/lanesawyer/bevy-data-explorer/commit/f27ffe405e4c713fb859062c770b8a8b9e9dc49d))
- **source:** Stop the source layer depending on the layers above it ([809f72e](https://github.com/lanesawyer/bevy-data-explorer/commit/809f72e1e0902207bef6bffe16f185085029552c))
- **view:** Split the grid, and give the shared code an owner ([b5a4ac5](https://github.com/lanesawyer/bevy-data-explorer/commit/b5a4ac52b7fe5ac3a93839979539f47a846eaf37))
- **formats:** Read point clouds asynchronously, and give up on what is not wanted ([f41abd0](https://github.com/lanesawyer/bevy-data-explorer/commit/f41abd0cd23990295864b5aa4d087e8f5d26fd38))
- **image:** Read tiles asynchronously, and abandon the ones panned past ([c6d16ac](https://github.com/lanesawyer/bevy-data-explorer/commit/c6d16ac159754980b53f911b6212c688d84aa1ae))
- **view:** Place chrome through one cell helper, and bring stale docs up to date ([00cedc1](https://github.com/lanesawyer/bevy-data-explorer/commit/00cedc187f35e83564786b7ad12c6502e0525cef))
- **cli:** Open everything named on the command line the way a typed URL is ([2abd4ff](https://github.com/lanesawyer/bevy-data-explorer/commit/2abd4ff636c9a0cac8f4efe2ac719798ba11e08d))
- **ui:** Draw the sidebar's sections as Feathers panes and groups ([e48c072](https://github.com/lanesawyer/bevy-data-explorer/commit/e48c0722eaad998045866757541a286fa581cfcb))

### Documentation

- Add agent guidance and project skills ([505d265](https://github.com/lanesawyer/bevy-data-explorer/commit/505d2650093ab0fd4f44277ebc87c5b3f4126550))
- Make AGENTS.md the canonical file, not the symlink ([c4d1600](https://github.com/lanesawyer/bevy-data-explorer/commit/c4d16005c1696aa043d9e48e1d7b815ecab8aa06))
- Describe the layered tree and the ordering rule ([145c884](https://github.com/lanesawyer/bevy-data-explorer/commit/145c88488a8ae69ed3925312e11cb11fd2f6b680))

### Maintenance

- License ([e0403c1](https://github.com/lanesawyer/bevy-data-explorer/commit/e0403c1d05fc5577b0bd1988e200dc7124e21368))
- Add bug and feature request templates ([fd53760](https://github.com/lanesawyer/bevy-data-explorer/commit/fd53760cd3faa14e5aba96e2b5937d3a42274601))
- Pull Request template ([a87b129](https://github.com/lanesawyer/bevy-data-explorer/commit/a87b129b2d6c03835fcbb24a69f37d8813ed8c65))
- Build and publish a release on a version tag ([b32a0e4](https://github.com/lanesawyer/bevy-data-explorer/commit/b32a0e4b6a79566097cd27f45bac7fe552aeb16c))
- Check pull requests, and keep the dependencies moving ([56855f1](https://github.com/lanesawyer/bevy-data-explorer/commit/56855f1b1379561718409925b13c320ce5b19858))
- **lint:** Fix clippy warnings, set lint levels in Cargo.toml, and run clippy in CI ([623d45f](https://github.com/lanesawyer/bevy-data-explorer/commit/623d45f13e3efafa954e11caa37ea074e589948a))
<!-- generated by git-cliff -->
[Unreleased]: https://github.com/lanesawyer/bevy-data-explorer/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/lanesawyer/bevy-data-explorer/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/lanesawyer/bevy-data-explorer/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/lanesawyer/bevy-data-explorer/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/lanesawyer/bevy-data-explorer/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/lanesawyer/bevy-data-explorer/releases/tag/v0.2.0

