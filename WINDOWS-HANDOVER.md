# Attic — Windows handover

The app was called JotJotBoom until 26 September 2026; the `jjb` prefixes in
crate names, the `<!-- jjb:table -->` comment, the `jjb-file:` sync key and
the `JJB_*` environment variables are deliberate leftovers and stay.

Written 26 September 2026 at the end of Phase A on the Linux box, for the
Claude Code session that builds the Windows app. That session cannot read
this machine's memory; everything it needs to know that is not in the
code is here, in `DECISIONS.md` (the running decision log, read the last
entries first) and in `CLAUDE.md` (agent notes: build, layout, rules).
Start every Windows session by reading `CLAUDE.md` and this file.

## The decision

Windows is the primary platform from 26 September 2026. The app is a Rust
core (`crates/jjb-core`) plus one shell per platform. The Windows shell is
Tauri 2 on WebView2 with Fluent UI React v9 chrome and a CodeMirror 6
editor, driven by the markdown scanner compiled to WASM
(`crates/jjb-md-wasm`). GNOME is served second by the same Tauri app on
WebKitGTK. Mac is third, later. The libcosmic app (`apps/jjb-cosmic`) is
frozen as the reference build on the Linux box: it is what "the current
behaviour" means when this document says the new editor must match it.

Why: the two real audiences are Windows and GNOME, not COSMIC. One primary
platform during the feature rush. Tauri makes the GNOME version a thin
platform layer rather than a second shell.

## The crate map and what may depend on what

```
Cargo.toml                 workspace (resolver 3); shared dependency versions
crates/jjb-core            the notes model, store, sync, scanner, images,
                           links, tables, platform trait.  No toolkit.
crates/jjb-md-wasm         wasm-bindgen wrapper around the scanner.
apps/jjb-cosmic            the libcosmic app, package name `attic`.
apps/jjb-win               (Phase C) the Tauri app.
server/                    PocketBase migrations, revision hook, README.
examples/notes             42 example notes and their photos (`just
                           install-examples` copies them into the notes dir).
tools/xshot.py             the Linux screenshot harness; tools/wshot.ps1 is
                           its Windows twin, to be written.
```

Rules:

- `jjb-core` never depends on libcosmic, iced, zbus or any UI crate. Its
  `full` feature (on by default) pulls disk, network and OS crates
  (rusqlite, ureq, image, dirs, open, uuid, blake3, chrono and, on Linux
  only, secret-service). Without `full`, only `markdown` and `table`
  remain, and that is what the WASM crate compiles.
- `just check-windows` (`cargo xwin check -p jjb-core --target
  x86_64-pc-windows-msvc`) passes on the Linux box; on Windows, `cargo
  test -p jjb-core` is the equivalent and must stay green.
- New behaviour that is not about pixels goes into the core, so both
  shells get it. The COSMIC shell keeps `crate::note`, `crate::store`
  and so on as paths through `use jjb_core::{…}` in its `main.rs`.
- The core's `markdown::scan_line` returns plain `Span { range, kind }`
  values with serde derives; the WASM crate serialises them to JSON.
  Colours and fonts per kind live in the shell (`apps/jjb-cosmic/src/
  markdown.rs::style_for` here, CSS classes there).
- The core's `images::process` takes `Inks` (five RGB triples: bg, mute,
  dim, fg, accent) rather than a palette; the shell converts.

## The platform trait

`jjb_core::platform::Platform` is the one place the OS shows:

```rust
fn name(&self) -> &'static str;                        // "linux", "windows"
fn default_notes_dir(&self) -> PathBuf;                // Documents/Attic
fn data_dir(&self, app_id: &str) -> PathBuf;           // where index.db lives
fn store_secret(&self, key: &str, label: &str, secret: &[u8]) -> Result<()>;
fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>>;
fn delete_secret(&self, key: &str) -> Result<()>;
fn open(&self, target: &str) -> Result<()>;            // URL or file, detached
```

`platform::current()` picks the implementation by target OS.
`platform::notes_dir(configured)` resolves the user's setting: the
`JJB_NOTES_DIR` environment variable wins (the harness uses it), then a
non-empty setting with `~/` expanded, then the platform default.
`platform::index_path(app_id)` is `data_dir(app_id)/index.db`.

`linux.rs` is the code the COSMIC app used: Secret Service over D-Bus
(blocking API), items tagged `application=attic`, `key=<key>`; XDG
directories through `dirs`. `windows.rs` is the stub: folders and
open-URL already work through `dirs` and `open`; `store_secret` fails
with "the keyring is not wired up on this platform yet", `get_secret`
returns `None`.

What Windows must implement in Phase C:

- Secrets through the `keyring` crate (Windows Credential Manager),
  service `attic`, account = the key. The one key in use is
  `sync-token` (`jjb_core::sync::TOKEN_KEY`), the PocketBase bearer
  token. Nothing else is stored in the keyring yet.
- `default_notes_dir` is `%USERPROFILE%\Documents\Attic`; `dirs`
  already gives that. If that folder is missing and a `JotJotBoom`
  sibling exists (the name before September 2026), the core uses the
  old folder and moves nothing.
- `data_dir` is `%LOCALAPPDATA%\<app_id>` (`dirs::data_local_dir`
  would be the closer match than `data_dir`, which maps to Roaming on
  Windows; the index is derived and must not roam).
- Single instance is not on the trait. Use `tauri-plugin-single-instance`
  and hand the second launch's arguments (files to open, `--search`,
  `--new`) to the running window, the way `app::Flags` does over D-Bus
  in the COSMIC app.

The keyring calls are synchronous; the COSMIC app wraps them in
`tokio::task::spawn_blocking` (see `apps/jjb-cosmic/src/secrets.rs`).
Tauri commands can do the same, or be declared `async` and spawn.
## The compatibility contract: files, index, sync

Everything below is what the Linux app writes and reads today (`crates/jjb-core`). The Windows shell sits on the same crate, so most of this is inherited for free; it is written down so that nothing in the Tauri layer, the settings file or a future rewrite drifts from it. File names, field names and table names are quoted from the code.

### 1. The notes folder

The notes folder is the source of truth. Default location: `<Documents>/Attic` (`platform::NOTES_FOLDER`), overridden by the `notes_dir` setting (a leading `~/` or `~\` is expanded) and, above both, by the `JJB_NOTES_DIR` environment variable (the test and screenshot harness uses it).

```
Attic/
  Kyoto.md                 one note per .md file, root only (subfolders are the user's, never scanned)
  Kyoto (2).md             title collision suffix " (n)", n from 2; past 9999 it is "Title <uuid>.md"
  .Kyoto.md.tmp            atomic-write temp file (dot-prefixed, so the scan never sees it)
  .trash/                  trashed notes, same file format, same naming rules
  .folders                 tags created as empty folders: one normalised tag per line, sorted, LF, trailing newline
  assets/                  pictures and attached files (import naming below); subfolders allowed
  assets/.links/           link-card preview cache: <20 hex chars of blake3(url)>.txt and .png (derived, disposable, not synced)
  assets/.incoming/        sync download staging: <record id>.part (derived, not synced)
```

- Only files whose extension is exactly `md` and whose name does not start with `.` count as notes, in the root and in `.trash/`.
- A note's `modified` time is the file mtime; it is not stored in the file.
- `index.db` does not live here. It lives in the per-user data dir (section 7).

### 2. The note file

Frontmatter is optional; when present it starts on the very first line with `---` and ends at the next `---` line. `parse_document` reads `id`, `created` (RFC 3339) and `pinned` (`true`, `yes` or `1`); values may be quoted. Every other frontmatter line (for example an Obsidian `tags: [x]`) is kept verbatim in `extra_frontmatter` and written back unchanged. Unterminated frontmatter means the whole text is body.

`serialize_document` always writes this exact shape, in this order:

```
---
id: 019234ab-7c1e-7d3a-9f10-2b4c6d8e0f12
created: 2026-09-05T08:14:02Z
pinned: true
---
# Kyoto

#travel/japan #todo

Night one at the ryokan. See [[Osaka]] and [[Tokyo|the capital]]. ==Book early==.

- [ ] reserve the train
- [x] pack

![lanterns](assets/kyoto-lanterns.jpg){frame=film align=left w=420}

https://www.japan-guide.com/e/e2158.html

[Itinerary](assets/itinerary.pdf)

| Day | Cost |
| --- | ---: |
| 1 | $120 |
| Total | =SUM(B2:B2) |
<!-- jjb:table cols=120,0 -->

---
```

- `id` is a UUID v7 (`uuid::Uuid::now_v7()`), lowercase. The welcome note has the fixed id `00000000-0000-7000-8000-000000000001` and `created: 2026-08-29T00:00:00Z` on every install so devices agree on it.
- `created` is written with second precision and a `Z` suffix (`to_rfc3339_opts(Secs, true)`).
- `pinned: true` is written only when pinned; the line is absent otherwise.
- The body follows the closing `---` and is written with a trailing `\n` when non-empty.
- A file found without `id` or `created` gets them stamped in on first index (the only time the app rewrites a file it did not author).
- Title (`derive_title`): the first non-blank line that is not tag-only, with leading `#` heading markers, `* _ ` ~`, `==`, images (with their `{...}`), link syntax and wiki-link brackets stripped; capped at 200 chars; fallback `Untitled`.
- Filename (`slug_filename`): the title with control chars and `/ \ : * ? " < > |` replaced by `-`, whitespace collapsed, leading/trailing `.`, space and `-` trimmed, capped at 100 chars; empty means `Untitled`. The file is renamed when the title changes, but `Title (n).md` is kept while the stem still matches.
- Preview (`preview`): the markdown-stripped text after the title line, whitespace collapsed, 140 chars then `…`, cut on a word boundary; fences, rules, tag-only lines and table separators are skipped; table rows read as cells joined by ` · `.

### 3. Things in the body

- **Tags**: `#name` at the start of a line or after whitespace or one of `( [ { , ;`; characters are alphanumeric plus `_ - /`; at least one letter; no `//`; stray `/ - _` trimmed from both ends; stored lowercase (`#Work` and `#work` are one tag). Ignored inside ``` or ~~~ fences and inside backtick spans. `# Heading` (hash then space) is not a tag. Nested tags use `/`. A line made only of tags is metadata, not a title. Folder names are normalised the same way (`normalize_tag`: lowercase, whitespace to `-`).
- **Wiki links**: `[[Target]]` or `[[Target|shown text]]`, no newline inside; the target is a note title, matched case-insensitively (`links.to_title COLLATE NOCASE`). Ctrl+click on a missing target creates `# Target`.
- **Tasks**: a list marker (`- `, `* `, `+ `, or `1. ` / `1) `) followed by `[ ]` (open) or `[x]` (done). Any 1 to 12 byte mark without spaces counts as done (`[✓]`, `[🦆]`); the `task_marker` setting chooses what the app writes. The box must be followed by a space or end the line; `[]` is not a box.
- **Images**: only a line that is entirely `![alt](path){attrs}` is an image block; an image inside a sentence stays plain markdown. `path` is relative to the notes folder (usually `assets/...`) and must not contain `://`. Attributes, space separated inside one `{}`: `frame=` one of `plain`, `box` (default), `tint`, `dither`, `bezel`, `print`, `ascii`, `film`, `pixel`, `comic`; `align=` `center` (default; `centre` also parses), `left`, `right`; `w=` pixels clamped to 96..2000 (absent means as wide as the text column; presets small 240, medium 420, large 720). Legacy `size=s|m|l` reads as 240 / 420 / none. Defaults are omitted on write, so `![a](assets/x.jpg)` is the minimal form.
- **Asset import** (`import_asset`): copied to `assets/<stem>.<ext>` where stem is `slug_filename` of the source stem, spaces to `-`, lowercased; ext lowercased (`png` when absent); on collision `<stem>-2.<ext>`, `-3`, ... Image extensions: png, jpg, jpeg, webp, gif, bmp.
- **Link cards**: a line that is only `https://...` / `http://...` (no whitespace), or only `[text](target)` where text is non-empty without `[` and target is a web URL or a relative path (no `://`, not starting with `#`). `[[...]]` and `![...]` are not link cards. The preview cache line format is `url=`, `title=`, `description=` (one per line) plus a 320 px thumbnail.
- **Tables**: GitHub pipe tables; header row, separator row with `---`, `:---:` (centre) or `---:` (right); `|` inside a cell escaped as `\|`; every row padded to the header's column count. Formulas are stored raw (`=SUM(B2:B5)`, `+ - * /`, `SUM AVG MIN MAX COUNT`); money cells like `$42.50` stay text. Manual sizes ride in an HTML comment on the line after the table: `<!-- jjb:table cols=120,0 rows=0,40 -->` (0 = auto).
- **Rules**: `---`, `***` or `___` (three or more) alone on a line. **Highlights**: `==text==`.

### 4. index.db (derived, disposable)

SQLite at `<data dir>/<app id>/index.db`, WAL mode, `PRAGMA user_version = 3` (`SCHEMA_VERSION`). On a schema bump the derived tables are dropped and rebuilt from disk; the sync tables survive. Deleting the file is always safe apart from one extra reconcile pass. Timestamps are RFC 3339 with microseconds and `Z`.

| Table | Columns | Holds |
|---|---|---|
| `notes` | `id`, `path` (absolute, unique), `title`, `preview`, `created`, `modified`, `pinned`, `trashed`, `hash`, `revision` | one row per file; `hash` = blake3 hex of the whole file text |
| `tags` | `id`, `name` (unique) | tag names in use |
| `note_tags` | `note_id`, `tag_id` | tag membership |
| `links` | `from_id`, `to_title` (NOCASE) | wiki-link targets for backlinks |
| `notes_fts` | `id UNINDEXED`, `title`, `body` | FTS5, `tokenize = 'unicode61 remove_diacritics 2'`; query = every word quoted, last word prefix `*`, ranked `bm25(notes_fts, 4.0, 1.0)` |
| `oplog` | `seq`, `note_id`, `revision`, `modified_at`, `device_id`, `hash`, `synced` | one row per local save; the local revision counter |
| `sync_state` | `note_id`, `record_id`, `revision`, `hash`, `trashed` | what the server last agreed to per note; empty `hash` = tombstone |
| `sync_meta` | `key`, `value` | `cursor`, `account` (`<url>|<user id>`), `files_cursor`, `folders_base` |
| `sync_files` | `path`, `record_id`, `revision`, `hash`, `local_hash`, `size`, `mtime` | per asset: agreed hash plus a cache of the local hash keyed by size and mtime (nanoseconds) |

Reindex on open: scan the folder; a file whose mtime is within 1 s of the indexed value is skipped, otherwise it is re-hashed and re-parsed if the hash changed; files gone from disk are dropped. The `account` key changing (different server or user) clears all three sync tables.

### 5. Sync protocol

A self-hosted PocketBase (`server/`). One cycle (`sync::run`, blocking, on a worker thread) = refresh token, pull notes since cursor, push pending notes, then the files half. Triggered 3 s after a save, every 60 s, on sign-in and by Sync now. HTTP `User-Agent: Attic/<version>`, bearer token on every call.

**Auth**: `POST /api/collections/users/auth-with-password` `{identity, password}`; sign-up `POST /api/collections/users/records` `{email, password, passwordConfirm}` (8+ chars) then sign in; `POST /api/collections/users/auth-refresh` at the start of every cycle (PocketBase rotates the token; 401/403/404 means sign in again). The token is the only secret: keyring key `sync-token` (`sync::TOKEN_KEY`), label `Attic sync`. Server URL and email are ordinary settings.

**`notes` collection** (`pb_migrations/1757000000_notes.js`): `owner` (relation to `users`), `note` (text, max 64: the frontmatter id), `revision` (int, server-owned), `device` (text 128), `modified` (text 64, the writer's RFC 3339 time), `blob` (text, max 20,000,000), `created`, `updated` (autodate). Unique index on (`owner`, `note`); rules restrict every operation to the owner.

**Envelope** (`blob`, JSON, missing fields default): alive `{"v":1,"trashed":false,"deleted":false,"text":"---\nid: ...\n---\n# Kyoto\n..."}`; tombstone `{"v":1,"trashed":false,"deleted":true,"text":""}`. `text` is the full file, frontmatter included.

**Revision hook** (`pb_hooks/notes.pb.js`, both collections): create sets `revision = 1`; update reads `base_revision` from the body (number or string) and answers `409 revision conflict` with `data.revision` when it differs from the stored value, else stores `revision + 1`. An update without `base_revision` is not checked; clients always send it.

**Pull**: `GET /api/collections/notes/records?sort=updated,id&perPage=200&page=N&skipTotal=1&filter=updated >= "<cursor>"` (no filter on first run). Cursor = the largest `updated` seen (a string compare). A record whose (`record_id`, `revision`) equals `sync_state` is an echo and is skipped.

**Push**: pending = every `notes` row whose `hash` or `trashed` differs from `sync_state`, plus a tombstone for every `sync_state` row with a non-empty hash whose note is gone. Create: `POST` `{owner, note, device, modified, blob}`. Update: `PATCH /records/<id>` `{device, modified, blob, base_revision}` with `base_revision` = `sync_state.revision` (0 when new). A note that also came down in this cycle is not pushed (it is a conflict). `409`, or `400` with `validation_not_unique` on create, means someone wrote first: the server copy is fetched by `filter=note = "<id>"` and treated as incoming. `404` on update means create next time.

**Applying a record** (`Store::apply_remote`), with `local_changed` = local hash or trash state differs from `sync_state` (or there is no state):
- tombstone and local changed: keep the note, record the tombstone's revision as base so the edit goes back up (edit beats delete); tombstone otherwise: delete the file.
- same hash and trash state: just record agreement (a reinstall, or both sides made the same edit).
- local changed: the local text is first saved as a new note with a fresh id and the title suffixed ` (conflict, <device name>)` (a heading line gets the suffix; otherwise `# Title (conflict, x)` is put in front), then the server text takes over the note id. The open note auto-updates to the server version; the editor never switches to the copy. Device name: `/etc/hostname`, else `$HOSTNAME`, else `another device` (Windows: the computer name).
- otherwise adopt: the server text is written byte for byte under the note id (re-serialised only if its frontmatter lacks that id or `created`), moved between the root and `.trash/` as `trashed` says, and renamed for its title exactly as a local save would.

**Files half** (`sync::files`, `pb_migrations/1758000000_files.js`): collection `files` with `owner`, `key` (text 64, unique per owner) = blake3 hex of the string `jjb-file:<path>`, `revision`, `device`, `modified`, `meta` (text 100,000), `data` (file, max 268,435,456 bytes, protected), `created`, `updated`. `meta` is JSON `{"v":1,"path":"assets/pic.jpg","hash":"<blake3 hex of the bytes>","size":1234}`; `path` is `/`-separated and relative to the notes folder. Upload is multipart: `device`, `modified` (RFC 3339 from the mtime), `meta`, and a `data` part whose filename is always `blob` (`application/octet-stream`); create adds `owner` and `key`, update adds `base_revision`. Download: `POST /api/files/token`, then `GET /api/files/files/<record id>/<stored name>?token=...` into `assets/.incoming/<record id>.part`, verified against `meta.hash`, then renamed into place. Syncable paths: `.folders`, or `assets/...` with no empty or dot-prefixed component and no `\`; symlinks are skipped. Pull then push, smallest file first, for about 20 s, then the cycle reports `more` and another is scheduled; files over 256 MB are reported and stay local. Reconcile rules: not here, adopt; same hash, agree; local hash equals the last agreed hash (untouched), replace; otherwise ours steps aside to the first free `name-2.ext`, `name-3.ext`, ... and every note containing `](old)` as a whole target is rewritten to the new name (`note::repoint_asset`) before the server file lands. `.folders` is never written by the files cycle; the store merges it three ways against `sync_meta.folders_base` (kept if both have it or either added it; removed if one side removed it; sorted) and the merged list goes up next cycle.

**Deliberately not synced**: `assets/.links/`, `assets/.incoming/`, any dot-named file, `.md` files in subfolders (not scanned at all), `index.db`, settings, the token, and asset deletions (a file deleted by hand stays on the server and on other devices).

**What the server sees**: user id, note id, revision, device id, the writer's `modified`, its own `updated`; for files the path hash, the size of the upload and the bytes. Title, body, tags, trash state and file names travel only inside `blob` and `meta`. They are plain JSON today, so a server admin could read them; the schema is designed so encrypting `blob`, `meta` and the upload later changes nothing else. Keep it that way: never add a server field that reveals content.

### 6. Settings keys

On Linux these are cosmic-config entries under app id `io.github.reddemo0110.Attic` (version 1). On Windows they become a JSON settings file with the same keys. Only `notes_dir`, `device_id`, `sync_url` and `sync_email` affect compatibility; the rest are appearance and may be ignored by a shell that lacks the feature.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `notes_dir` | string | `""` | notes folder; empty = `<Documents>/Attic` |
| `device_id` | string | `""` | per-installation UUID v7, generated on first run; stamped into `oplog` and sent as `device` |
| `sync_url` | string | `""` | PocketBase address; empty = sync off |
| `sync_email` | string | `""` | account email (the token is in the keyring) |
| `theme` | string | `""` | retro theme key; empty = default |
| `show_markers` | bool | false | draw markdown markers dimmed instead of hidden |
| `hide_nav` | bool | false | collapse the views/tags column |
| `hide_list` | bool | false | collapse the notes list |
| `collapsed_tags` | string[] | `[]` | tags whose sub-tags are folded (full paths) |
| `editor_font` | string | `""` | editor font key; empty = system monospace |
| `ui_font` | string | `""` | sidebar + list font key |
| `list_font` | string | `""` | notes-list font key; empty = follow `ui_font` |
| `title_font` | string | `""` | pane-title font key; empty = VT323 |
| `editor_font_size` | u16 | 0 | editor text size px; 0 = default |
| `sidebar_font_size` | u16 | 0 | sidebar text size px |
| `list_font_size` | u16 | 0 | notes-list text size px |
| `dock_size` | string | `""` | `small`, `medium`, `large`, `wow`; empty = medium |
| `task_marker` | string | `""` | what a done task box contains; empty = `x` |
| `text_width` | string | `""` | `narrow`, `medium`, `wide`, `full` |
| `icon_theme` | string | `""` | launcher icon theme key; empty = follow the colour theme |
| `coffee_unlocked` | bool | false | hidden theme found |
| `tag_icons` | string[] | `[]` | `tag=icon` entries for folder icons |
| `icon_set` | string | `""` | folder icon style (boxicons, iconoir, ...) |
| `link_previews_off` | bool | false | do not fetch web link previews |
| `tag_order` | string[] | `[]` | sidebar order of top-level tags; `""` = spacer line |
| `animation` | string | `""` | glide time ms; `"0"` = snap |
| `animation_ease` | string | `""` | ease-out exponent in tenths (`"10"`..`"60"`) |
| `editor_weight` | u16 | 0 | 200, 300, 400 or 500; 0 = 400 |
| `buffet_on` | bool | false | paint with the colour buffet pairing |
| `buffet_highlight` | string | `""` | buffet highlight key; empty = tomato |
| `buffet_dark` | string | `""` | buffet dark plate key |
| `buffet_mode` | string | `""` | `light` or anything else = dark |
| `buffet_light` | string | `""` | buffet paper plate key |
| `buffet_ink` | string | `""` | buffet ink key |
| `table_toolbar_off` | bool | false | hide the table toolbar |
| `rule_size` | string | `""` | `---` weight: small, medium, large, family |
| `card_line` | string | `""` | link/file card border weight, same scale |
| `tag_line_size` | string | `""` | sidebar spacer weight, same scale |
| `card_border_off` | bool | false | no border on cards |
| `window_width` | u32 | 0 | last window size, logical px; 0 = let the WM choose |
| `window_height` | u32 | 0 | same |

### 7. The platform trait on Windows

`jjb_core::platform::Platform` (`crates/jjb-core/src/platform/mod.rs`) is everything that differs per OS. The section "The platform trait" above lists the methods; on Windows they resolve to:

- `name()` -> `"windows"`.
- `default_notes_dir()` -> `dirs::document_dir()` + `Attic`, i.e. `%USERPROFILE%\Documents\Attic` (falls back to home, then `.`). Already works.
- `data_dir(app_id)` -> `dirs::data_local_dir()` + app id, i.e. `%LOCALAPPDATA%\io.github.reddemo0110.Attic\index.db` for the index. Local, not Roaming, on purpose: the index is derived and must never follow a profile between machines. Already works.
- `store_secret(key, label, bytes)`, `get_secret(key)`, `delete_secret(key)` -> Windows Credential Manager through the `keyring` crate (service `attic`, account = `key`); the only key in use is `sync-token` with label `Attic sync`. Today the stub returns an error on store and `None` on get, which degrades to a sign-in that lasts for the session.
- `open(target)` -> `open::that_detached` (ShellExecute), already fine.

Single-instance and argument forwarding are not in the trait; they belong to the shell (Tauri's single-instance plugin).

### 8. Invariants a Windows build must keep

- Note files are UTF-8 without BOM and use LF only. Never write CRLF, not in notes, not in `.folders`. The parser tolerates `\r\n` in frontmatter lines, but the body keeps any `\r`, the hash changes, and every other device sees a modified note.
- The content hash is blake3 over the exact bytes of the whole file text (`note::content_hash`), lowercase hex; file hashes are blake3 over the bytes; the file key is blake3 of `jjb-file:<path>` with a `/`-separated path. Same bytes on every device means the same hash, which is what lets a reinstall re-adopt quietly.
- Writes are atomic: write `.<name>.tmp` in the same directory, `sync_all`, then rename over the target (`std::fs::rename` replaces on Windows). Never truncate a note in place.
- Serialise frontmatter in the fixed order `id`, `created`, `pinned`, extra lines; RFC 3339 with `Z`; body ends with one `\n`. Only rewrite a file to stamp a missing `id` or `created`; otherwise leave files you did not change alone.
- Filenames follow `slug_filename` and the ` (n)` collision rule; the ` (n)` suffix survives later saves. Windows-reserved characters are already replaced, but a title such as `CON` or a trailing dot is not guarded; strip those in the shell if it matters.
- Paths inside note bodies and in `meta.path` use `/`, never `\`; `safe_path` refuses backslashes. Convert at the filesystem boundary only. `notes.path` in the index is absolute in native form and is local to the machine.
- Reindex trusts mtime within 1 s; keep file mtimes honest (do not touch files you did not change).
- Autosave is debounced (600 ms); anything that switches or drops the current note flushes first. Never push a note whose editor buffer is unsaved.
- Sync never clobbers: both sides changed becomes a conflict copy, edit beats delete, a different file of the same name steps aside as `name-2.ext` and its notes follow, `.folders` merges three ways. Never send a `PATCH` without `base_revision`.
- `index.db` holds nothing that cannot be rebuilt from the folder except the sync tables (`oplog`, `sync_state`, `sync_meta`, `sync_files`); do not add state that lives only in SQLite.

## How the editor behaves

Everything below is read from the code (`apps/jjb-cosmic/src/editor/`, `markdown.rs`, `blocks.rs`, `app.rs`, `crates/jjb-core/src/{markdown,table,images,links,note}.rs`). Where `DECISIONS.md` says something older, the code wins and is flagged.

### The model

- The buffer text is the note body, byte for byte. Formatting is attributes plus a draw-time overlay; nothing is transformed before layout. Files stay the truth.
- A note body is a list of blocks: text runs, images, rules, link cards, tables. One editor instance per text run; text runs always bracket the non-text blocks (possibly empty). Caret and selection never cross a block. Rejoining: empty text runs contribute no line, the body always ends with `\n` (a blank line between two adjacent images is normalised away; the one known lossy case).
- Body split rules (`images::split`): a line is its own block when it is an image line `![alt](path){...}`, a rule (`---` / `***` / `___`, three or more of one char, spaces allowed), a link-card line (see Links), or the first line of a pipe table (a line starting with `|` whose next line is a `| --- |` separator row; rows continue while lines start with `|`; an optional `<!-- jjb:table ... -->` line follows). Inside a ``` or ~~~ fence nothing splits.
- Line height is 1.5 × font size. Default size 15 px (range 10 to 48). Editor padding 6 px vertical, 10 px horizontal. Text column measure: 560 / 720 (default) / 920 px / unlimited (`text_width`).

### Hybrid markdown rendering (per line, `scan_line` + `style.rs`)

- Every line is scanned into spans. Two looks per line: the *revealed* line shows its markers dimmed; every other line hides them. "Ghost" is the marker colour on a revealed line: `mute` at 45 % alpha, or `dim` when View → Show markdown syntax (Ctrl+Shift+M) is on. Code note: that toggle only changes the revealed line; other lines collapse markers regardless (DECISIONS' older "dim instead of ghosted everywhere" predates the rich editor).
- A line is revealed only by: double-click on it; typing one of `* _ ` ~ [ ] > #`; Backspace/Delete that would eat a hidden marker (Marker, LinkUrl, ListMarker, TaskBox, TaskDone, QuoteMarker); paste is not a reveal. A single click just places the caret. Reveal is dropped when the caret leaves the line or the editor loses focus. After `[]` expands or a box is clicked, the line renders at once (no raw `- [ ] ` flash).
- Hiding has two flavours. *Collapsed* (transparent, 0.5 px font so ~zero width): `Kind::Marker` (`**`, `*`, `` ` ``, `~~`, `==`, `# `, `[[`, `]]`, `[`, `](`, `)`, fences, rule lines) and `Kind::LinkUrl`. *Transparent at full width* (glyphs keep their space so an overlay is drawn there and caret columns are unchanged): `ListMarker` (`- `, with 0.36 em letter spacing on bullet lines so the dot gets a gutter; not on task lines), `TaskBox`, `TaskDone`, `QuoteMarker`.
- Span kinds and paint (colour slot / font): `Bold` fg bold; `Italic` fg italic; `BoldItalic` fg bold italic; `Code`, `CodeBlock` accent2, monospace family; `Heading` bold, accent for H1/H2, `h3` slot for H3 to H6 (accent on classic themes, plain fg on minimal themes); `Tag`, `Link` accent2; `LinkUrl` ghost; `ListMarker`, `NumMarker` accent; `Quote` dim italic; `Done`, `Strike` fg at 45 % alpha; `Mark` fg (the band carries the colour); `TaskBox` dim; `TaskDone` accent bold; `Marker`, `QuoteMarker` ghost.
- Heading sizes: `#`..`######` + space, scale 1.6 / 1.35 / 1.15 / 1.0 (H4+). The whole line takes the bigger metrics (marker and inline bold share it); heading line height = max(1.5 × size × k, size × k × 1.2).
- Inline scanner rules: `*`/`_` runs of 1 to 3 (1 italic, 2 bold, 3 bold-italic), the opener must be followed by non-whitespace and the closer preceded by non-whitespace, closer is the same delimiter; `~~x~~` strike and `==x==` mark, doubles only; `` `code` `` to the next backtick; `[[Target|alias]]`; `[label](url)`; `![alt](path){attrs}` anywhere on a line is one ghost `LinkUrl` span; `#tag` at line start, after whitespace, or after `( [ { , ;`, chars alnum `_ - /`, must contain a letter. Unmatched markers are literal.
- Line starts: heading; `>` (optional space) quote; `- `/`* `/`+ ` list, then optional task box; `1. `/`1) ` numbered (digits stay visible in accent, never turned into a dot). Fences ``` / ~~~ toggle fence state; fenced lines are `CodeBlock`.
- Overlays, drawn in this order: code-block row backgrounds (accent2 at 8 %, full width); code-span backgrounds (accent2 at 15 %, radius 4, height 1.25 × size, 3 px side padding); highlighter bands (accent at 32 %, radius 3, height 1.35 × size, 2 px side padding; one band per unbroken stretch, two marks on a row are two bands); quote bars (dim, 3 px wide, radius 1.5, inset 2 px top and bottom, at the `> ` span); selection (`sel`); glyphs; strikes (from the strike attribute: done text at fg 55 % alpha, `~~` at 80 %, leading blanks trimmed); task boxes; tag icons; bullets; Ctrl-hover underline; drop line; caret.
- Bullet: a `•` glyph in accent at the `- ` span's x + 2, vertically centred. Only for `- `/`* `/`+ ` without a box.
- Task box: square side = 1.1 × size (or the span width if narrower), border 1.5 px radius 3, positioned over the `- ` (1 px in). Open: dim border, no fill. Done: accent border, accent fill at 18 %, the mark drawn centred in accent at 0.8 × box height; `x`/`X` is shown as `✓`, any other mark as itself. Emoji marks (code point ≥ U+1F000) cannot be recoloured, so the real glyph stays visible and the box is widened to 1.5 × size around it.
- Task box syntax (`task_box`): `[ ]` open; `[` + mark + `]` done, mark 1 to 12 bytes, no spaces; must be followed by a space or end the line. The user's chosen done mark is written into the file: `- [✓] milk`, `- [🦆] milk`. Choices: x, ✓, ✔, –, •, ★, 🦆, 🔥, 💀, 🍕, 🐈, 🚀, 👍, 🍺 (config `task_marker`, empty = x). Any single mark from other apps counts as done. Done text is the rest of the line: fg 45 % plus strike.
- Tag icons: a tag whose path (or leaf) has a folder icon shows the icon (0.95 × size, accent2) drawn over its `#`, which is made transparent, off the revealed line only. Coffee-word tags (`coffee`, `espresso`, ...) wear ☕.
- Rules are blocks, not text: full-width line in the muted colour, thickness 1 / 2 / 4 / 8 px by `rule_size` (Medium = 2 default).
- Caret: 1.5 px wide, `caret` colour, full line height at the caret, blinks every 500 ms, hidden when the window is unfocused. Placeholder text in dim when the block is empty.

### Formatting toggle engine (`apply_format`)

- Inline formats and marker pairs: Bold `**`/`**`, Italic `*`/`*`, Mark `==`/`==`, Code `` ` ``/`` ` ``, Link `[[`/`]]`.
- Single-line selection with non-blank content: leading/trailing whitespace is left outside. If the selection itself starts and ends with the markers, strip them and select the inner text. Else if the selection is *surrounded* by the markers (`wrapped_in`: Bold needs ≥ 2 `*` on both sides, Italic needs an odd count on both sides, others exact string) eat the surrounding markers and keep the words selected. Else wrap and select the inner words. Star-run counting is what lets `***word***` nest instead of eating each other.
- Multi-line selection: strip if it starts and ends with the pair, else wrap the whole selection; replaced by paste, selection not kept.
- No selection: if the text after the caret starts with the closing marker and the text before contains the opener, step the caret right over the closing marker (Ctrl+B, type, Ctrl+B = start bold, stop bold). Else if the caret touches a word (alnum, `_`, `'`, caret inside or at either edge): unwrap it when wrapped (caret shifts left by the marker length) or wrap it (caret shifts right by the marker length). Else insert the pair with the caret in the middle.
- Mark has its own path first (`mark_selection`): caret inside a `==mark==` and no selection rubs it out; with a selection, each line is handled separately (a mark cannot span lines), the start is clamped past the line's lead-in (heading hashes, list/quote/task prefix, a typed `• ◦ ▪ ‣ –` bullet), whitespace trimmed; if every part is already inside a mark, all are unmarked, otherwise each is marked, merging marks it overlaps or touches (a space away counts) and dropping stray `==` so marks never nest. Keyboard on one line: words stay selected. Sweep or multi-line: caret lands just past the closing `==`. If nothing applies it falls through to the word/pair rules above.
- Line formats: H1 `# `, H2 `## `, Bullet `- `, Todo `- [ ] `, Quote `> `. Applied to every line the selection touches (or the caret's line). The first line decides: if it already carries the same prefix, the prefix is removed on every line; otherwise any existing prefix (`# `, `## `, `> `, `- `/`* `/`+ ` with or without a box) is removed and the new one inserted. Caret ends at End of the last line. Note: `line_prefix` matches at column 0 only and does not know numbered lists.
- Tag: inserts `#`. Rule: on an empty line inserts `---` + newline at its end; otherwise newline + `---` + newline after the line. Table: inserts a starter table (header + 2 rows, 3 columns) after the caret's line (on it if blank) plus a newline; the enum doc says "at the end of the note", the code inserts after the current line.
- Every format action is one undo step, sets dirty, and re-splits blocks (rule, image, link, table separator lines become blocks).

### Keyboard behaviour

- Widget key table: Ctrl+C/X/V/A; Shift+Insert paste, Ctrl+Insert copy, Shift+Delete cut; Ctrl or Alt + any letter is left to the app; Enter; Backspace; Delete; Escape unfocuses; Tab indents, Shift+Tab unindents (cosmic-text: spaces to the next tab stop, tab width 8, every line of a selection); arrows, Home, End, PageUp, PageDown, with Ctrl widening (word / document) and Shift selecting.
- Enter on a list line (`continue_list`, only for a plain `Edit::Enter`, not paste): carries the marker onto the new line with the indent kept: `- `, `* `, `+ `, `1.`/`1)` counts up keeping the punctuation, `- [x] ` comes back as `- [ ] `. Enter on an item with nothing after its marker clears the marker (ends the list). A selection, a non-list line, or a caret inside the marker gives a plain Enter. Mid-item Enter splits the text and carries the marker.
- `[]` typed at the start of a line (indent and a `- ` allowed) expands on the `]` to `indent- [ ] `; the single space typed next at exactly that spot is swallowed (`task_space`), anything else clears the hint.
- Clicking a task line anywhere from the list marker through the `]` flips the box (`[ ]` ↔ `[mark]`), caret to End, one undo step.
- Block seams: Backspace at (0,0) with no selection deletes the image / rule / card / table above and merges the text, caret at the seam. ↑ on line 0 hops to the previous text block (caret at its end); ↓ on the last line hops to the next (caret at its start). With the editor unfocused, ↑/↓ open the previous/next note in the list.
- After Enter or paste, if the focused block now holds a rule, image, link-card or table-separator line, the block is re-split live with the caret preserved.
- Opening a note puts the caret at the end of the first line (the title; byte 0 sits before the hidden `# `), view at the top. A new note starts as `# ` with the caret after it; a body of only heading markers counts as blank and is deleted on close.
- Caret scroll-into-view: any edit, Move or Select flags a reveal; after layout the widget reports the caret line's rectangle and the note's scrollable scrolls just enough with a margin of 2 × font size. Clicks and drags do not trigger it.
- Ctrl+click on `[[Title]]` opens the note by title, or creates it (`# Title`, blank line, caret on the third line) unless the trash view is open; Ctrl+click on `#tag` selects that tag view. Hover: hand cursor over task boxes always, over links/tags while Ctrl is held, with a 1.5 px accent2 underline.
- Middle-button drag is the highlighter pen: press places the caret, drag selects, release with a selection runs Mark with `sweep = true` (`snap_to_words`: the start goes back to its word's first letter, the end to the nearer edge of its word).
- IME: preedit shown at the caret; commits arrive as paste.
- Undo/redo: a step is a snapshot of the whole body + focused block + caret; 200 steps; stacks reset on note switch; redo cleared by any new edit; exact-repeat bodies skipped. Grouping: Insert/Enter/Indent = typing, Backspace/Delete/Unindent = deleting; consecutive same-kind edits within 700 ms are one step; anything else (paste, format, image ops, box toggle, cell edits) starts a new step. Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y.

### Shortcut table (`key_binds`, every MenuAction)

- File: Ctrl+N new note; Ctrl+Shift+N new folder; Ctrl+Shift+I add image; Ctrl+Shift+A attach file; Ctrl+F search; Ctrl+Shift+P pin; Ctrl+Shift+D trash; Ctrl+Q quit.
- Edit: Ctrl+Z undo; Ctrl+Shift+Z and Ctrl+Y redo.
- Format: Ctrl+B bold; Ctrl+I italic; Ctrl+E code; Ctrl+H highlight; Ctrl+1 H1; Ctrl+2 H2; Ctrl+L bullet; Ctrl+T to-do; Ctrl+K wiki link; Ctrl+Shift+3 tag; Ctrl+R rule; Ctrl+Shift+Q quote. Table has no key (Format menu / dock only).
- View: Ctrl+Shift+1 toggle sidebar; Ctrl+Shift+2 toggle notes list; Ctrl+Shift+0 editor only; Ctrl+`+`, Ctrl+Shift+`+`, Ctrl+`=` zoom in; Ctrl+`-` zoom out; Ctrl+0 zoom reset; Alt+, options drawer; Ctrl+Shift+M show markdown syntax; Ctrl+Shift+H shortcuts overlay. About has no key. (Ctrl+Shift+Enter is the coffee sign, not in the map.)
- Zoom: 1 px per step, 10 to 48, reset 15, persisted as `editor_font_size`.

### Images

- Line format: `![alt](assets/x.png){frame=tint align=left w=420}`; defaults omitted (frame `box`, align `center`, no `w` = as wide as the text column). Frames: plain, box, tint, dither, bezel, print, ascii, film, pixel, comic (plain is the bare picture across the text column; choosing it in the menu also clears `w` and `align`). Width presets small 240 / medium 420 / large 720, min 96, max 2000; drag the `◢` grip, committed on release. `align=left|right` floats the following paragraphs beside the picture until the next image; `center` is a full-width block capped by `w`. Alt text is the caption (box title, print caption, comic narration box).
- Insert (drop, Ctrl+Shift+I, picker) copies into `assets/` with a unique readable name and puts the line after the caret's line (on it if blank); focus moves to the text block below at (0,0).
- Click opens the ⋯ menu (frames, align, width presets + full, caption, open, remove). Press and travel more than 6 px starts a drag: the note switches to drop mode (every body line drawn plain with a top half = drop above, bottom half = drop below), an accent 2 px drop line labelled "▼ picture drops here" glides to the target, Esc cancels, dropping mid-paragraph splits it, a strip under the last line reaches the end. One undo step. Dragging a file in from outside shows the same line; the file is inserted at the caret then moved to the line.
- Images are downscaled to ≤ 720 px, processed off-thread, cached by path|mtime|style|theme.

### Tables

- File form: GitHub pipe table (`| a | b |`, `| --- | :---: | ---: |` alignment row, `\|` escapes a pipe), optionally followed by `<!-- jjb:table cols=120,0 rows=0,40 -->` with manual px sizes (0 = auto), written only when some size is manual. Rows are rectangular; parse pads/truncates cells to the header width.
- Hand-typed tables convert to a grid the moment the separator row is finished (Enter/paste re-split). Format → Table / dock `⊞` inserts a 3 × (1 + 2) starter.
- Cells: click opens an in-place single-line input with the raw text (formulas show their text, the grid shows values). Enter commits and moves down; on the bottom row it adds a row first. Tab commits and moves right (wraps to the next row's first cell; stays put at the last cell). Shift+Tab moves left (stays at column 0). ←/→ roam cells only while the draft is untouched and no modifier is held; the first keystroke hands them back to the text caret; every arrival resets to roaming. ↑/↓ walk cells (nothing at the edges). Escape drops the draft and returns focus to the editor. Clicking back into note text, or switching notes, commits.
- Toolbar while a cell is open (hidable with `table_toolbar_off`): + row, + column, − row, − column (the header row cannot be removed; a table keeps at least 2 rows and 1 column), `$ ¤` money cycle, `fx` hint. Column and row edges are drag handles; auto columns share the width, a dragged column becomes fixed.
- Formulas: cell text starting with `=`: numbers, `+ - * /`, unary minus, parentheses, refs `B2` (row 1 = header), ranges `B2:B4` only inside functions, `SUM AVG AVERAGE MEAN MIN MAX COUNT` (case-insensitive), `,` or `;` separates arguments. Text/empty cells are skipped in ranges, an error in a direct ref. Bad input, unknown function, empty AVG/MIN/MAX and circular refs show `#ERR`. Values shown in accent2; integers stay integers, others rounded to 4 decimals.
- Money: `$42.5`, `€`, `£`, `¥`, commas allowed, optional leading `-`; still a number to formulas; displayed normalised with two decimals and thousands grouping (`-$1,234.50`). Money spreads: a formula touching a money cell shows money in the first symbol met; `=$SUM(...)` (symbol right after `=`) forces it. `$ ¤` cycles the open cell plain → $ → € → £ → ¥ → plain by rewriting its text.
- Formula pointing: while the draft is a formula whose last non-blank char is `= ( + - * / , ; :`, clicking another cell appends its ref, sweeping appends `A1:B2`; swept cells tint; release returns the caret to the end of the input.
- Fill handle: the open cell has an accent dot at its bottom-right; drag it and on release the source is copied over the swept run clamped to one axis (whichever moved further), formulas translated relative to each target (`=SUM(C2*C3)` one row down → `=SUM(C3*C4)`), off-edge refs become `#REF`, values copy verbatim.
- Cell selection: press-drag across cells rubber-bands (a press that never leaves its cell is a click-to-edit); Delete or Backspace empties every selected cell; Escape or any click drops it. Any table interaction blurs all text editors.
- Previews render rows as `a · b · c` and skip separator and size lines.

### Links and cards

- A line that is only `https://...` / `http://...` (no whitespace), or only `[text](target)` (text non-empty and without `[`, target a web URL or a relative path without `://` and not starting with `#`; `[[` excluded) is a card block. Links inside a sentence stay inline. Cards appear when the line is finished (Enter / paste).
- Web card: picture, title, description, `⌁ domain/path` (scheme and `www.` stripped, 60 chars). Preview from Open Graph / Twitter / `<title>` tags, page cap 1.5 MB, picture cap 8 MB, thumbnail 320 px, cached as `assets/.links/<20 hex of blake3(url)>.txt` (`title=`, `description=` lines) + `.png`; derived, disposable. Once a bare address knows its title the line is rewritten to `[title](url)` (dirty, no undo step). Option "Fetch link previews" (`link_previews_off`): off shows only the address, cache still honoured.
- File card: `[name.ext](assets/name.ext)` written when a non-image file is dropped or attached (Ctrl+Shift+A); shows a kind badge from the extension (`PDF`, `ZIP`, `FILE` fallback), a kind label and human size. Double-click opens it.
- ⋯ on a card: open, copy link, refresh preview, remove. Backspace at the start of the next block removes it; drag moves it like a picture. Card border weight 1 / 2 / 4 / 6 px (`card_line`), or frameless (`card_border_off`).

### Autosave, folding, misc

- Autosave: 600 ms after the last edit (`AUTOSAVE_DELAY`), driven by a tick. The editor header shows a quiet `✓`; clicking it shows "✓ saved hh:mm" for four seconds; on failure the badge reads "✗ not saved, last saved hh:mm" (tooltip has the error) and retries every ~5 s. Anything that switches or drops the note flushes first.
- Folding: there is no heading folding. The only folding is the sidebar tag tree (▸/▾ on parent tags, persisted as `collapsed_tags`).
- Wiki links `[[Target|alias]]`: the hotspot target is the text before `|`; `extract_links` indexes targets for backlinks ("linked from" pane).
- Titles and previews strip markup: the title is the first line that still says something after stripping (an image line is skipped), the preview starts after it, clipped on a word boundary at 140 chars; `==` doubles are stripped, a lone `=` kept.
- Tag rename rewrites `#old` and `#old/...` across every note (code fences, inline code and `#oldx` untouched).

## The retro look: palettes, fonts, frames

Source of truth today: `apps/jjb-cosmic/src/retro.rs` (palettes, fonts, sizes, frames), `crates/jjb-core/src/images.rs` (picture treatments), `apps/jjb-cosmic/src/glyph.rs` and `dockicon.rs` (icon sets), `apps/jjb-cosmic/src/icon.rs` (launcher icon). On Windows every value below becomes a CSS custom property on `:root` (`--jjb-bg`, `--jjb-panel`, `--jjb-fg`, ... one per Palette slot, plus `--jjb-font-title`, `--jjb-font-ui`, `--jjb-font-body`, `--jjb-rule-px`, `--jjb-card-px`, `--jjb-measure`, `--jjb-dock-icon`), switched by setting the theme key on `<html data-theme>`; the treatments become canvas work (the core's `images::process` already returns RGBA, so a Tauri command can do them). Keep the keys exactly as listed, they are what config files and the script harness use.

The rule that has held since day one: retro styling belongs to the note surface and the themes; the window chrome stays stock. On COSMIC that means the header bar and window buttons are native and everything below is ours. On Windows: the Tauri title bar stays native (Fluent), the whole webview below it is the retro surface.

### Palette slots

| Slot | Used for |
|---|---|
| `bg` | window and sidebar background; ASCII and bordered card background |
| `panel` | notes list, editor, dock pill, box frame background (one tonal step above `bg`) |
| `fg` | body text, bold, italic, marked text, caret on the classics |
| `dim` | quotes (italic), task boxes, visible markers, pane badges, sidebar spacer lines, placeholder text; pane titles on minimal themes |
| `mute` | hidden markers (at 45% alpha), the `---` divider, hover row, search box border |
| `accent` | H1/H2, list bullets and numbers, done ticks, dock `+` and picture button, drop/slot lines, ASCII art, film frame number, tint highlight, focused search border |
| `accent2` | tags, links, inline code and code blocks (the "tag colour" the folder icons are drawn in) |
| `border` | 1px hairlines between panes, card borders, dock border, box frame border |
| `sel` | selected row background, selected text fill, 3px bar in the swatch |
| `selfg` | text on `sel` |
| `caret` | editor caret (classics: `fg`; minimal: `accent`) |
| `h3` | H3 to H6 text (classics: `accent`; minimal: `fg`, so only H1/H2 carry colour) |
| `title` | pane header text (classics: `accent`; minimal: `dim`) |

Fixed drawing constants: pane padding 10/12 px, pane title 21 px in the title face; box frame 1px `border`, radius 6, padding 14/12/10/12, title cut into the top edge on a `panel` chip; hairlines 1px `border`; dock pill `panel` with 1px `border`, radius 10; rows radius 3; search input transparent, 1px border, radius 3; buffet chips 24px circles with a 2px ring; sidebar text 13px, badges 12px.

### Classic themes (`caret` = `fg`, `h3` = `title` = `accent` on every one)

| key | name | bg | panel | fg | dim | mute | accent | accent2 | border | sel | selfg |
|---|---|---|---|---|---|---|---|---|---|---|---|
| phosphor (default) | The Terminalator | #050806 | #070b08 | #b9f2bf | #57a36a | #264a2e | #4dff8f | #9ad3ff | #1f5a2f | #0f3a1c | #d7ffe0 |
| amber | Amber Dawn | #0b0704 | #0e0905 | #ffc978 | #b3772c | #4a3010 | #ffb02e | #ffe9a8 | #5c3c12 | #3a2408 | #fff1d6 |
| wordperfect | Big Trouble in Little Blue | #1c2874 | #1c2874 | #e6e8ee | #9ea7d8 | #4f5ca8 | #f0e07c | #85d4da | #7c86c6 | #2f3d94 | #ffffff |
| paper | Monochromando | #0a0a0b | #0f0f11 | #e8e8e8 | #8c8c8c | #4c4c4c | #ffffff | #c9d3ff | #3c3c3c | #2c2c30 | #ffffff |
| white | Ghostwriters | #ffffff | #fbfbfb | #0c0c0c | #5c5c5c | #b4b4b4 | #0c0c0c | #3b5b8c | #d2d2d2 | #e8e8e8 | #0c0c0c |
| plasma | Lethal Plasma | #180500 | #1f0700 | #ff8f3c | #cb682a | #5e2c0e | #ffb66b | #ffdcbb | #6e3414 | #4c2009 | #ffe6cf |
| c64 | Escape from C64 | #443c86 | #443c86 | #bdb8ee | #9c96d8 | #655cb0 | #d0d68e | #9cd29a | #7c75c0 | #5a52a8 | #ffffff |
| gameboy | Handhelder | #a9b58a | #a1ad82 | #1f261a | #4d5a3c | #7d8a63 | #2c3a24 | #445236 | #6e7a58 | #6e7a58 | #dbe3c4 |
| synthwave | Streets of Neon | #120823 | #190c2f | #f2dcff | #8f72b8 | #4d3778 | #ff6ec7 | #00e5ff | #5e3d94 | #3b2063 | #ffffff |
| goldfish | Big Fish in Little China | #0f1a1e | #13222a | #e0e4cc | #8fbcbc | #2f4d56 | #fa6900 | #69d2e7 | #2f5260 | #1f3d48 | #ffffff |
| goldfish-muted | Little Fish in Big China | #0f1a1e | #13222a | #e0e4cc | #8fbcbc | #2f4d56 | #c9773f | #69d2e7 | #2f5260 | #1f3d48 | #ffffff |
| provoking | Blade Thinker | #2a1520 | #311a27 | #f0dcb0 | #b08a6a | #5e3a4a | #d95b43 | #8ab4b8 | #6d3f52 | #542437 | #ffffff |
| emokid | Cheer Up, Karate Kid | #1e252b | #242c33 | #e6edf0 | #8f9ea9 | #3d4a54 | #4ecdc4 | #c7f464 | #455360 | #323e49 | #ffffff |
| oceanfive | The Abyss Five | #0d1b1e | #122226 | #f2e8d0 | #a39a7c | #31494e | #eb6841 | #00a0b0 | #2f4f55 | #1e3d43 | #ffffff |
| adrift | Adrift in Dreamscape | #081f2d | #0c2737 | #dfe8d8 | #79a99a | #244d5c | #cff09e | #79bd9a | #2a5d6d | #1a4a5c | #ffffff |
| longblack (hidden) | Long Black | #160f0b | #1d1410 | #f1e2c8 | #b08a63 | #5a4030 | #e0a262 | #f0c9a0 | #3d2b1f | #4a3222 | #fff3e0 |
| cosmic | Desktop Cop | system | system | system | fg @55% | fg @30% | system accent | = accent | system divider | = accent | on-accent |

`longblack` is not in `Theme::ALL`; it unlocks by typing `coffee` in the search box. `cosmic` reads the desktop theme (bg = container base, fg = container text, accent = system accent); on Windows map it to the Windows accent colour on the system light/dark base, or leave it out.

### Minimal themes and the colour buffet

Every minimal theme is one highlight colour on a shared charcoal: `minimal(accent) = minimal_with(accent, #121214, #d7d7d5)`. The buffet is the same function with a chosen plate and ink: dark side `buffet_dark(accent, plate) = minimal_with(accent, plate, #d7d7d5)`, light side `buffet_light(accent, plate, ink) = minimal_with(accent, plate, ink)`. The highlight appears only as the selection band, tags and links, the caret, H1/H2 and the small marks that already ride the accent; every neutral is a blend of the plate toward the ink, so any plate works.

`minimal_with(accent, bg, fg)`, with `mix(a, b, t) = a + (b - a) * t` per RGB channel and `luma = 0.2126 R + 0.7152 G + 0.0722 B` on 0..1:

| slot | value | | slot | value |
|---|---|---|---|---|
| bg | plate | | accent2 | accent |
| panel | mix(bg, fg, 0.03) | | border | mix(bg, fg, 0.12) |
| fg | ink | | sel | mix(bg, accent, 0.28) |
| dim | mix(bg, fg, 0.63) | | selfg | luma(bg) < 0.5 ? #f2f2f0 : fg |
| mute | mix(bg, fg, 0.29) | | caret | accent |
| h3 | fg | | title | mix(bg, fg, 0.63) |

Highlights (the 19 minimal theme keys, picker order): tomato #ce2939 Tomato, steelblue #35637c Steel Blue, gojiberry #b22e3d Goji Berry, coquelicot #ec4908 Coquelicot, shamrock #71a56a Shamrock Green, seoulyellow #f6b65a Seoul Yellow, burntbrandy #f29538 Flame of Burnt Brandy, bluefjord #648ba8 Blue Fjord, azure #0099ff Azure, mikado #ffc40c Mikado, apricot #ffab40 Apricot, cyan #00a6cb Cyan, fuchsia #ff0080 Fuchsia, jade #9cd5c2 Jade, hollyhock #aa89bd Hollyhock, thistle #d8bfd8 Thistle, canyonclay #cf8578 Canyon Clay, slateblue2 #466e88 Slate Blue, bluestone #587284 Bluestone.

Dark plates (`Dark`, default richblack): night #0e0e10, richblack #171717, jetblack #1a1a1a, graphite #1f2124, ink #212529, onyx #353839.

Light papers (`Light`, default pearlwhite, picker order): lightgray #f5f5f5, antiflash #f2f3f4 Antiflash White, nearlywhite #fafafa, subtleoffwhite #f8f7f7 Subtle Off-White, warmwhite #fffdf7, pearlwhite #fbfcf8, milk #fffff5, ivorywhite #fffcef, offwhite #faf9f6, cream #fafaf2, porcelain #f6f6f6, coastalivory #f6f1e8, moon #edede9, quartz #e1e6ea, bone #d4d4ce.

Inks (`Ink`, default ink): midnight #060b11, ink #212529, navy #023246, night #495057, slate #57666d Steel Slate.

Config: `buffet_on`, `buffet_mode` (dark/light), `buffet_highlight` (a minimal theme key), `buffet_dark`, `buffet_light`, `buffet_ink`. Choosing a theme in the Colour section switches the buffet off; clicking any chip switches it on.

### Picture frames

Markdown form, whole line only: `![alt](assets/x.png){frame=tint align=left w=300}`; default attributes are omitted (`size=s|m|l` is the legacy form: 240, 420, full). Pictures are downscaled to `DISPLAY_MAX` = 720 px on the long edge before treatment (`plain`: 1600 px, `display_max(style)`). `align`: center (default, a full-width block centred when narrower), left, right (following paragraphs flow beside). `WIDTH_PRESETS`: small 240, medium 420, large 720; `w` clamps to 96..2000; no `w` means the text column width. Inks passed to the treatments are `bg`, `mute`, `dim`, `fg`, `accent` as 0..255 RGB; `tint`, `dither` and `ascii` depend on the theme (cache key includes it).

| key | label | pixel treatment | card |
|---|---|---|---|
| plain | plain | none | none: the bare picture, as wide as the text column; the ⋯ chip and resize grip still overlay it |
| box (default) | box frame | none | box frame: `panel` fill, 1px `border`, radius 6, alt text as the title |
| tint | phosphor tint | greyscale, `luma^0.9`, mapped bg -> accent (alpha kept) | 1px `border` on `bg`, radius 3, 3px padding |
| dither | dithered | Floyd-Steinberg to 4 shades bg/mute/dim/fg (luma quantised to 4 levels, error 7/16 right, 3/16 down-left, 5/16 down, 1/16 down-right) | same thin border |
| pixel | chunky pixels | nearest-neighbour down to 96px on the long edge and back up | same thin border |
| bezel | CRT bezel | every third row (y % 3 == 0) x0.72; radial vignette beyond r 0.7, up to 55% darker at the corners | inner: black, 1px `mute`, radius 8, 2px pad; outer: black @90%, 1px `accent` @35%, radius 14, 12px pad |
| print | instant print | none | paper #e9e6da, 1px #d6d2c4, radius 2, padding 10/10/14/10, alt text as caption in VT323 20px ink #3a3630 |
| film | film strip | none | strip #0b0f0c, 1px `border`; 12 sprocket holes 9x7 #1f1f22 radius 2 above and below; "▶ 01A" (index) in VT323 15px `accent`, right aligned |
| comic | comic book | desaturate 20%, 45° halftone on a 5px cell; dot radius 5·sqrt(cover/π)·1.12 where cover = 1 - mean tone of the 3x3 around the cell centre; inside a dot the colour x0.5, between dots colour·0.8 + paper(0.96,0.925,0.86)·0.2 | paper #f2e8cd with 1px #d9cfae, 8px pad; panel 3px ink #18121a; caption uppercased in VT323 18px on yellow #f6d85a with 2px ink border, tucked top-left |
| ascii | ASCII | ramp `" .:-=+*#%@"` by luma; cols = clamp((w-16)/7.2, 48, 140), glyph size = clamp((w-16)/(cols·0.62), 5, 14) px, rows = h/w · cols · 0.5 | monospace `accent` text, line height 1.05, on `bg` with 1px `border`, radius 3, 6px pad |

A picture mid-drag gets a 2px `accent` outline; the drop line is a 2px `accent` rule with a VT323 16px label in `bg` on an `accent` chip.

### Fonts

All bundled under `apps/jjb-cosmic/resources/fonts/<licence>/<family>/`; static Regular/Bold/Italic/BoldItalic files, never variable fonts. Licences by folder: `ofl` = SIL OFL 1.1, `ufl` = Ubuntu Font Licence 1.0, `apache` = Apache 2.0. Faces marked "no bold" ship Regular only, so titles in them are set at normal weight (`has_bold`). The Tauri app should ship the same files as web fonts.

| key | face | folder | notes |
|---|---|---|---|
| system | System monospace | (desktop) | default body and UI; on Windows use Cascadia Mono, then Consolas |
| vt323 | VT323 | `VT323-Regular.ttf` (OFL) | the title face; no bold |
| plex, plexserif | IBM Plex Mono, IBM Plex Serif | ofl | |
| fira | Fira Mono | ofl | Regular, Bold |
| ubuntu, ubuntusans | Ubuntu Mono, Ubuntu | ufl | |
| anonymous | Anonymous Pro | ofl | |
| space | Space Mono | ofl | |
| courier | Courier Prime | ofl | |
| b612 | B612 Mono | ofl | |
| lato | Lato | ofl | + Light |
| opensans | Open Sans | ofl | + Light, Medium |
| ptsans, ptserif | PT Sans, PT Serif | ofl | |
| atkinson | Atkinson Hyperlegible | ofl | |
| spectral | Spectral | ofl | |
| dmserif | DM Serif Display | ofl | Regular, Italic; no bold |
| specialelite | Special Elite | apache | no bold |
| montserrat, lora, bitter, raleway, playfair | Montserrat, Lora, Bitter, Raleway, Playfair Display | ofl | |
| sourcesans | Source Sans 3 | ofl | + ExtraLight, Light, Medium |
| oswald | Oswald | ofl | Regular, Bold |
| roboto | Roboto | ofl | + ExtraLight, Light, Medium |
| mulish, nunitosans, hankengrotesk | Mulish, Nunito Sans, Hanken Grotesk | ofl | + ExtraLight, Light, Medium |
| commissioner | Commissioner | ofl | Regular, Bold + ExtraLight, Light, Medium |
| oldstandard | Old Standard TT | ofl | Regular, Bold, Italic |
| abril | Abril Fatface | ofl | no bold |

`BASE_FONTS` (shown first in the picker): system, opensans, mulish, nunitosans, hankengrotesk, commissioner, lato, sourcesans, roboto, atkinson, ptsans; the rest sit under "more fonts". The body weight setting (`weight`, 200/300/400/500) needs the extra Light/Medium files above. Config: `title_font`, `ui_font`, `editor_font`.

Pairings (`PAIRINGS`, key: title / sidebar+list / note): attic (default): vt323 / system / system; plex: plexserif / plex / plexserif; editorial: spectral / lato / spectral; magazine: dmserif / lato / lato; paratype: ptserif / ptsans / ptserif; hyperlegible: atkinson throughout; ubuntu: ubuntusans / ubuntusans / ubuntu; typewriter: specialelite / courier / courier; opensans: opensans throughout; montserrat: montserrat / opensans / opensans; playfair: playfair / opensans / opensans; lora: montserrat / opensans / lora; bitter: bitter / sourcesans / opensans; oswald: oswald / opensans / opensans; raleway: raleway / opensans / opensans; roboto: roboto / roboto / opensans; oldstandard: abril / opensans / oldstandard.

### Size scales

| scale | keys | values |
|---|---|---|
| editor text | `editor_font_size` | default 15 px, 10..48, 1 px a step (Ctrl+plus/minus, Ctrl+0 resets) |
| sidebar and list text | `sidebar_font_size`, `list_font_size` | default 13 px, 9..30 |
| `Measure` (text column max width) | narrow, medium (default), wide, full | 560, 720, 920, none; labels 🐟 to 🐟🐟🐟🐟 |
| `DockSize` | small, medium (default), large, wow | icon 14/17/22/30 px; button and pill padding [2,4]/[3,6]/[5,9]/[8,14] |
| `LineSize` (`---` divider, `rule_size`; sidebar spacers, `tag_line_size`) | small, medium (default), large, family | 1/2/4/8 px |
| `LineSize` (link and file card border, `card_line`) | same keys | 1/2/4/6 px; `card_border_off` hides it |
| `TASK_MARKERS` | x ✓ ✔ – • ★ 🦆 🔥 💀 🍕 🐈 🚀 👍 🍺 | the mark written inside `- [ ]` when a task is done |

### Icons

Folder icon sets (`IconSet`, config `icon_set`): boxicons (Boxicons Solid, MIT, default), iconoir (Iconoir, MIT), solar (Solar Bold, CC BY 4.0, 480 Design), mynaui (Myna UI Solid, MIT), majesticons (Majesticons Solid, MIT), pixelarticons (Pixelarticons, MIT), duoicons (Duoicons, MIT). Bundled as Iconify SVG path bodies drawn in `currentColor`. The rule: Boxicons covers all 59 meanings; a set that lacks one borrows the Boxicons drawing. A tag wears an icon in place of its `#` when assigned (config entries `tag=key`, matched by full path first, then by leaf name; a tag containing a coffee word gets the coffee cup), drawn in `accent2` in the sidebar, the picker and over the hash in a rendered note. Icon keys: coffee book camera home work music heart star plane food idea code money gift leaf gear flag pin bug game beer cart car bell calendar envelope phone moon sun cloud film pencil key lock brain cat dog palette wrench trophy rocket wine pizza bank medal truck bag movie bookmark folder user pram paint tree ship train bed cake drink.

Dock icons (`DockIcon`, generated by `tools/dock_icons.py`) use the same set with the same fallback: bold, italic, mark (highlighter), code, table, h1, h2, bullet, todo, quote, link, tag, rule, plus, image. `plus` and `image` are drawn in `accent`, the format icons in `fg`. Sets without a native heading-1/2 glyph get the set's heading glyph with a stroked numeral. The SVG path bodies in `glyph.rs` and `dockicon.rs` can be lifted straight into the web app.

Launcher icon (`icon.rs`): a 64x64 squircle (tile at 80% of the canvas) with a vertical gradient from `mix(bg, fg, 0.10)` at the top to `bg` at the bottom; two dots (r 3.9 at x 16.22 and 26.43, y 38.21) and a slightly skewed `#` of four rounded bars, all in `fg`. Config `icon`: a theme key or "follow" (the current theme, buffet included). On Windows generate the `.ico` from the same SVG per theme.

## The screenshot harness: script steps

`tools/xshot.py out.png [--script '...'] [--notes-dir DIR] [--wait 4] [--settle 1.5] [--keep] [--binary target/debug/attic]` is the Linux harness; `tools/wshot.ps1` on Windows must honour the same contract, driving the WebView2 window through WebDriver (tauri-driver + msedgedriver) instead of Xwayland.

### Launch

- `JJB_SCRIPT` carries the script (below). `JJB_NOTES_DIR` points the run at a fresh scratch folder made per run (`jjb-notes-*` in the temp dir) unless `--notes-dir` names a real one; steps like `new`, `type`, `attach`, `image` write files there. `JJB_NOTES_DIR` overrides the configured notes dir (default `~/Documents/Attic`).
- `COSMIC_SINGLE_INSTANCE=false` bypasses single-instance activation, otherwise a running Attic would be handed the launch and no window would open. The Tauri build needs an equivalent switch (same variable name is fine).
- `XSHOT_LOG=path` keeps the app's stdout/stderr (tracing writes to stdout); unset or empty means discard. `RUST_LOG` defaults to `attic=info,warn`.
- xshot.py does not set `XDG_CONFIG_HOME` or `XDG_DATA_HOME`, so a run inherits the real config (theme, fonts, sizes) and the real `index.db` under the data dir; only the sync tests set `XDG_DATA_HOME` (`sync_state` lives in index.db). wshot.ps1 should isolate both (a scratch `APPDATA`/`LOCALAPPDATA`, or a `JJB_CONFIG_DIR`) so captures start from defaults.
- Other hooks: `JJB_LINK_FIXTURE=page.html` serves that file for every link preview (no network); `JJB_PB_URL` is the PocketBase for the sync integration test; `JJB_SCREENSHOT=path` is the in-app iced capture and is not trusted (it drops editor text and menu labels).
- Sequence: launch with `DISPLAY` set and `WAYLAND_DISPLAY` removed; sleep `--wait` (4 s); find the top-level window whose class contains `attic`, viewable, wider than 100 px (retry up to 6 times with a 3 s timeout each); sleep `--settle` (1.5 s); capture; terminate the app unless `--keep` (5 s grace, then kill). X keyboard auto-repeat is switched off for the whole run and restored after, so a stuck key cannot type into the window.
- The script starts 1200 ms after launch. The window opens at the last saved size (`window_width`/`window_height`, minimum 480x320).

### Capture

One PNG of the whole window at its current geometry (XGetImage on Linux, BGRX to RGB), printed as `saved out.png WxH`. It must show everything a person would see: editor text, pane titles, dock icons, open menus and popovers, the theme drawer. On Windows capture the window's pixels (PrintWindow/Graphics.CopyFromScreen of the HWND), not a DOM screenshot, so native menus and the title bar are included.

### Script form

`JJB_SCRIPT="new;type:Hello #tag;wait:1500;exit"`. Steps are `name` or `name:arg`, separated by `;`. Write `\;` for a literal semicolon inside text; `\n` inside `type:` (and `search:`, `folder:`, `draft:`, `cell:` text, `follow:`, `imgcaption:` text) is unescaped to a real newline. A step's leading whitespace is trimmed, a trailing space in `type:` survives. Unknown step names are logged and skipped; a step whose argument fails to parse is dropped. Every step runs through the app's normal messages, exactly as real input would.

| step | argument | effect |
|---|---|---|
| `new` | | create a note (Ctrl+N), editor focused |
| `type:TEXT` | text | insert at the caret, one character at a time; each `\n` is an Enter through the editor (continues lists, indents), typed one line per tick |
| `search:TEXT` | text | set the search box text (typing `coffee` unlocks Long Black) |
| `select:N` | 0-based index | select the N-th note in the current list; no-op past the end |
| `pin` / `trash` | | toggle pin / move the current note to `.trash/` |
| `folder:NAME` | tag name | create the folder tag (written to `.folders`) and switch to it |
| `format:KEY` | bold italic code h1 h2 bullet todo link tag mark table rule quote | the dock action on the selection or current line |
| `selectall` | | select all in the focused editor block |
| `dock` | | toggle the dock's `+` section |
| `undo` / `redo` | | one step |
| `shortcuts` | | toggle the shortcuts overlay |
| `savedinfo` | | click the save tick ("saved hh:mm") |
| `themes` | | open the Appearance drawer |
| `solo` | | toggle editor-only layout |
| `section:NAME` | colour (or color), font, tasks, icon, links, buffet, tables, sync; anything else = size | fold/unfold that Appearance section |
| `theme:KEY` | theme key | switch theme (buffet off) |
| `buffet:H,DARK` | highlight key, dark plate key | colour buffet, dark side |
| `buffet:H,PAPER,INK` | highlight, paper key, ink key | colour buffet, light side |
| `font:KEY` | editor font key | set the editor face (target pane from `fontfor`) |
| `fontfor:PANE` | tags, notes, anything else = editor | point the font picker at a pane |
| `pairing:KEY` | pairing key | apply a designer pairing |
| `weight:N` | 200, 300, 400, 500 | editor body weight |
| `size:PANE:±N` | sidebar, list, anything else = editor; signed step | grow/shrink that pane's text by N px |
| `docksize:KEY` | small medium large wow | dock size |
| `measure:KEY` | narrow medium wide full | text column width |
| `marker:MARK` | one of `TASK_MARKERS` | the finished-task mark |
| `icon:KEY` | theme key or `follow` | launcher icon colours |
| `iconset:KEY` | icon set key | folder icon style |
| `tagicon:TAG:KEY` | tag, icon key or `none` | give a tag an icon (or clear it) |
| `coffee` | | toggle the neon coffee sign |
| `image:PATH` | absolute file path | import as if dropped on the window (copied into `assets/`) |
| `attach:PATH` | absolute file path | attach a file as a card |
| `pick` / `pickdir:DIR` | | open the picture picker / point it at a folder |
| `imgframe:N:KEY` | N-th image (0-based), frame key | set its frame |
| `imgalign:N:KEY` | left, center, right | set its alignment |
| `imgwidth:N:PX` | px, 0 = text column | set its width |
| `imgcaption:N:TEXT` | | set its alt text/caption |
| `imgmenu:N` | | open its ⋯ menu |
| `imgdrag:N:LINE` | image, body line | show it mid-drag with the drop line before that line |
| `imgmove:N:LINE` | | the same, then release: the picture moves |
| `linkdrag:N:LINE` | N-th link card | show the card mid-drag |
| `sel:L,C` / `sel:L,C,L2,C2` | line and byte column | place the caret; or select from (L,C) anchor to (L2,C2) caret |
| `sweep:L,C,L2,C2` | | the highlighter pen (middle-drag) over that range: wraps whole words in `==` |
| `key:NAME[:TIMES]` | backspace delete enter tab left right up down home end | press that key in the focused editor, TIMES times (default 1) |
| `togglebox:L:C` | line, column | toggle the task box at that position |
| `cell:R,C,TEXT` | row, col, text | set a cell of the first table (grows the table as needed) |
| `editcell:R,C` | | open a cell for editing and leave it open |
| `draft:TEXT` | | set the open cell's draft text (`draft:=SUM(`) |
| `fpick:R,C` / `fpick:R,C,R2,C2` | | formula pointing: click a cell, or sweep a range (mouse left down for capture) |
| `fpickover:R,C` | | extend the active pick to a cell (its own step, so a render happens between points) |
| `pickdone` | | release the mouse |
| `fill:R,C,R2,C2` | | drag the fill handle from one cell to another |
| `tsel:R,C,R2,C2` | | rubber-band select cells (mouse left down) |
| `fold:TAG` | tag path | fold/unfold its sub-tags |
| `tagmenu:TAG` | | open the tag's right-click menu |
| `renametag:OLD:NEW` | | rename a tag in every note |
| `tagdrag:N:SLOT` | sidebar entry, drop slot | show the entry mid-drag with the slot line |
| `tagmove:N:SLOT` | | the same, then drop |
| `addspace` | | append a spacer line to the tag list |
| `nav:±N` | signed | walk the notes list like ↑/↓ |
| `follow:TITLE` | note title | follow a `[[wiki link]]` as Ctrl+click would |
| `sync:URL,EMAIL,PASSWORD[,new]` | | sign in to a sync server (`,new` creates the account); writes the token to the real keyring |
| `syncnow` | | run one sync cycle |
| `wait:MS` | milliseconds | pause (lets autosave and previews run) |
| `quit` | | the File menu's quit path: flush, close the window |
| `exit` | | flush the current note and exit the process |

A run normally ends with `wait` long enough for the last render, then the harness captures and kills the app; `exit` is for tests that only need the files on disk.

## The Windows build, step by step

The order the current editor grew is the order to rebuild it; each
milestone is usable on its own.

1. **Core on Windows.** Clone, `cargo test -p jjb-core` green. Implement
   the platform pieces above. `cargo test` with `JJB_PB_URL` against a
   PocketBase proves sync works from Windows (see `server/README.md`).
2. **Shell scaffold.** `apps/jjb-win`: Tauri 2, Vite, React, Fluent UI
   React v9. Tauri commands wrap `jjb_core::store::Store` (open the notes
   dir, list, search, tags, create, save, trash, restore) and
   `jjb_core::sync::run`. One `Store` behind a mutex in Tauri state.
   Autosave debounce stays in the shell (the COSMIC app uses 600 ms).
3. **Milestone 1.** Notes list, tags tree, search, a plain textarea in a
   Fluent window, saving to disk, syncing from the same server as the
   Linux box. From here on the same notes folder can be opened by both
   apps (never at the same time on one machine).
4. **The harness.** `tools/wshot.ps1`: launch the app with `JJB_SCRIPT`
   and `JJB_NOTES_DIR`, drive it through WebDriver (msedgedriver +
   tauri-driver), capture the window to PNG. Same script form as
   `xshot.py` (section below). Claude needs this to check its own work;
   build it before the editor, not after.
5. **Milestone 2.** The CodeMirror 6 editor: spans from the WASM
   scanner, active line shows markers, other lines hide them, retro
   palettes as CSS custom properties, the formatting toggle engine
   (port `apply_format` from `app.rs`; the rules are in the editor
   section below).
6. **Then in order:** images and pixel treatments (the core's
   `images::process` gives RGBA; draw to a canvas or serve a data URL),
   link cards, tables, folding, the theme picker and colour buffet, the
   dock and its icons, text zoom, the Windows title bar and Mica
   backdrop.
7. **Packaging.** MSIX and a Start menu entry.
8. **Two chrome themes from the start**, Fluent and an Adwaita-flavoured
   one, even if the second is rough. It stops Windows assumptions
   leaking into layout before the GNOME build.

## What is deliberately left out

- The COSMIC shell is frozen: bug fixes only, no new features there.
  When the Tauri build is the better GNOME app, it retires (record it in
  `DECISIONS.md`).
- Mac is untouched. The platform stub covers it for compiling only.
- End-to-end encryption is built on the parked `drive-sync` branch
  (`sync/sealed.rs`, Argon2id + XChaCha20-Poly1305) but not merged and
  not wired into any UI. The README's claim that the server "never
  reads" notes is only true once that lands. The Google Drive backend on
  that branch is blocked on an OAuth client the user has not created.
- Asset deletes do not sync (deliberate v1). No realtime subscription:
  the app polls every 60 s and 3 s after a save.
- The GNOME search provider and the D-Bus single instance are COSMIC-
  shell features; the Tauri app on GNOME will need its own (later).
- Nothing is packaged for Linux beyond `just install-user`.

## Working habits that paid off here

- Dogfood with scripted writer flows through the harness, then `cat -A`
  the file: most editor bugs showed up as wrong bytes before they showed
  up on screen.
- One harness step per event for drag-like flows, so a render happens
  between them; injecting several messages in one step skipped the
  frame where handlers were wired.
- Keep the user's real notes out of test runs (`JJB_NOTES_DIR` to a
  scratch folder). Copy real notes into a scratch dir when a rendering
  check needs them.
- The user tests between turns and reads `DECISIONS.md`; write the why
  there for every behaviour choice, dated, before moving on.
- Never reformat files you did not touch.
