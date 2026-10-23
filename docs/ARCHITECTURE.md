# Architecture

Brook is a Cargo workspace with two crates. Everything that can be tested without a window lives in `brook-core`; the GPUI app in `brook` stays thin.

```
crates/
  brook-core/   storage, network, parsing, typesetting model, updates (no UI)
  brook/        the GPUI app: window, views, background work
assets/         brand art, the macOS bundle files, the Linux desktop entry
scripts/        icons, bundling, Linux build dependencies
release-notes/  one Markdown file per release
```

## brook-core

Synchronous, blocking, and small. The app decides which thread anything runs on.

| Module | What it does |
| --- | --- |
| `model` | Plain data: `Feed`, `Folder`, `Article`, `ArticleSummary`, `Scope`, `Counts`. |
| `store` | SQLite through `rusqlite` (bundled). One `Store` is one connection; open one per thread. WAL mode, so readers never wait on the refresher. |
| `parse` | RSS, Atom, and JSON Feed through `feed-rs`, normalized into `ParsedFeed`: title, link, author, excerpt, HTML body, lead image, date. |
| `http` | A shared `ureq` client: user agent, timeouts, a size cap, and conditional GET (`ETag`, `Last-Modified`). |
| `discover` | Turns typed input into feeds: the address itself, then `<link rel="alternate">` feeds, then common paths. Every candidate is fetched and parsed, so the caller gets a preview. |
| `refresh` | Fetches feeds six at a time on scoped threads; writes go through the caller's `Store`, one feed at a time. Unchanged feeds cost a `304`. |
| `opml` | Import (nested folders keep their innermost name) and OPML 2.0 export. |
| `document` | Article HTML to a `Document` of blocks (paragraphs, headings, quotes, lists, code, images, captions, video cards, rules) with inline spans (bold, italic, code, strike, links). Relative addresses resolve against the article. |
| `reader` | Reader mode: the main article from a web page, via `dom_smoothie`, a port of Mozilla's Readability. |
| `cache` | Images and site icons on disk, named by a stable hash or by host, written atomically. Icons come from each site, never a third-party service. |
| `catalog` | The Discover catalog, compiled in from `catalog.json`. |
| `settings` | Typed preferences stored as one JSON row, so new fields never need a migration. |
| `time` | Relative dates for lists, long dates for the reader, and the local start of day. |
| `update` | GitHub release checks, minisign verification, and the in-place swap. See [RELEASING.md](RELEASING.md). |
| `paths` | Per-platform data and cache directories; `BROOK_DATA_DIR` overrides both. |

### The database

Migrations are SQL strings applied in order and tracked with `PRAGMA user_version` (`store/schema.rs`). Never edit a shipped migration; append one.

- `feeds` holds the subscription and its HTTP validators. `folders` don't nest.
- `articles` holds everything about an article, reading state included (`read_at`, `starred_at`, `saved_at`): there's one reader, so there's no separate state table. `(feed_id, guid)` is unique, so a refresh updates text in place without touching reading state.
- `articles_fts` is an FTS5 index over titles and plain-text bodies, written next to each article and kept in step on delete by a trigger. Searches match every word as a prefix, with diacritics folded.
- `settings` is a key-value table; preferences are one JSON value in it.

Old read articles are *compacted*, not deleted: after 90 days their bodies are dropped but the row stays, so the next fetch recognizes the guid and doesn't bring the article back as new.

### Testing

Network code runs against `testing::TestServer`, a tiny HTTP/1.1 server on a random local port, so tests exercise real sockets, redirects, `304`s, and errors without the internet. `catalog_feeds_are_live` is the one test that does use the internet; it's ignored by default and worth running before a release:

```sh
cargo test -p brook-core catalog_feeds_are_live -- --ignored
```

## brook

| Module | What it does |
| --- | --- |
| `lib.rs` | Starts the app, builds the window (`root`), and sets up menus. |
| `library` | The `Library` entity: the app's state over the UI thread's `Store`. Selection, reading state, folders, OPML, settings, reader mode, and background refresh. Emits `Notice`s for toasts. |
| `images` | The `Images` entity: asks the disk cache for an image while rendering; the first ask starts a download and the view re-renders when it lands. |
| `updater` | The `Updater` entity: a daily check and install-and-relaunch. |
| `workspace` | The root view: title bar, sidebar, and the workspace card; every action handler. |
| `sidebar`, `article_list`, `reader`, `discover` | The views. The list is a virtualized `uniform_list`. |
| `typeset` | Lays out a `Document`: styled text runs, clickable link ranges, images, and video cards. |
| `add_feed`, `settings_view`, `dialogs` | Dialog contents. |
| `theme` | The `Palette` global and gpui-component's theme, kept in step. |
| `actions` | Commands and key bindings. Single-key shortcuts only fire in the `Reading` key context (the list and reader), so they never fight a text field. |
| `assets`, `components` | Icons and brand art; small shared elements. |

### Threads

The UI thread owns one `Store` for quick reads and small writes. Anything that touches the network runs through `cx.background_spawn`:

- **Refresh** opens its own `Store` on a background thread, runs `brook_core::refresh::refresh`, and streams each feed's outcome back over a channel so the sidebar counts and progress update live. When it finishes, the list reloads.
- **Reader mode, discovery, images, and updates** each fetch in the background and hand the result back with `cx.spawn` and `WeakEntity::update`.

### Testing

`Library` tests use GPUI's `TestAppContext` against a real database in a temp directory. Pure helpers (theme rules, assets, monogram colors) have plain unit tests.

`examples/screenshots.rs` renders the real app off-screen through Metal and saves PNGs, without needing the window on screen or screen-recording permission. It's how the README screenshots are made:

```sh
BROOK_NO_GLASS=1 cargo run -p brook --example screenshots --features screenshots -- \
    path/to/library.db path/to/cache docs/screenshots
```
