# Attic — agent notes

Called JotJotBoom until 2026-09-26; the `jjb` prefixes (crates, `jjb:table`,
`jjb-file:`, `JJB_*`) are deliberate and stay, see DECISIONS.md.

Hybrid-markdown notes app, Rust. A Cargo workspace since 2026-09-26:
`crates/jjb-core` (everything true of a note regardless of window; no
toolkit), `crates/jjb-md-wasm` (the markdown scanner for the browser),
`apps/jjb-cosmic` (the libcosmic app for COSMIC and GNOME, the reference
build; frozen while the Tauri app for Windows is built elsewhere).
Read `Project Handover — …md` for the original architecture,
`DECISIONS.md` for how the open questions were settled and why, and
`WINDOWS-HANDOVER.md` for the Windows-first plan and the contracts the
Tauri app must keep. `RICH-EDITOR-PLAN.md` is the plan for build step 3
(the cosmic-text editor).

## Build / run / test

Toolchain lives in `~/.cargo/bin` (rustup); make sure it's on `PATH`.

- `cargo build` / `cargo run` — debug build, launches on the current Wayland session
- `cargo test` — unit tests (note format, SQLite/FTS5 index, store round-trips)
  in the core and the app, plus the WASM parity test (skips unless the
  wasm32 target, `wasm-bindgen` CLI at the crate's pinned version and
  `node` are present; node lives in `~/.local/opt/node/bin` here)
- `just run` — release build + run; `just check` — clippy pedantic
- `just check-windows` — `cargo xwin check -p jjb-core` for
  `x86_64-pc-windows-msvc`; the core must always pass this
- `just install-user` — release build + per-user install (~/.local: binary,
  launcher entry with absolute Exec, icon, metainfo); `just uninstall-user`
- `RUST_LOG=attic=debug cargo run` for tracing output
- Visual check without a human: `tools/xshot.py out.png [--script ...]`
  runs the app on Xwayland (`XSHOT_LOG=path` keeps its log; the app is killed once the picture is taken, so for a check that needs it alive longer pass `--wait N`, a script `wait:` alone does not hold it), drives it via the `JJB_SCRIPT` hook (`step:arg` form, e.g. `--script 'new;type:Hello;wait:1000'`; steps:
  new, type, search, select, pin, trash, folder, format, selectall, dock,
  themes, theme, proof (`proof:sg`, `proof:s`, `proof:off`; loads the engine), image, imgframe, imgalign, imgwidth, imgcaption, imgmenu, imgdrag, imgmove, linkdrag, fold, font, pairing, size, docksize, section, tagmenu, renametag, nav, togglebox, marker, measure, follow, icon, coffee, tagicon, pickdir, iconset, attach, tagdrag, tagmove, addspace, weight, fontfor, buffet, sel, sweep, cell, editcell, fpick, fpickover, pickdone, draft, fill, tsel, sync, syncnow, notesdir, key (`key:backspace`, `key:left:3`; backspace, delete, enter, tab, arrows, home, end), quit, wait, exit; `;` separates steps — write `\;` inside text; a `\n` in `type:` is a real Enter, so it continues lists like the keyboard does),
  and captures the window with X auto-repeat switched off. It runs against a
  fresh scratch notes dir (`JJB_NOTES_DIR`); pass `--notes-dir ~/Documents/Attic`
  only when the real notes are needed for the picture — steps like `new`/`type`/`attach` write files. Each run also gets its own `XDG_DATA_HOME` (a scratch `index.db`): the index is shared per data dir and the folder watcher re-indexes on change, so a run sharing the real one would empty the real app's list. Portal screenshots hang unattended, and the in-app `JJB_SCREENSHOT`
  hook (iced `window::screenshot`) silently drops editor text and menu labels.
  For anything the script hook cannot reach (the menu bar, popovers, hover)
  use `--nested 1600x1000`: the app runs in a private headless mutter with its
  own Xwayland, and `--click X,Y` (window pixels, repeatable) / `--key KEYSYM`
  drive it through mutter's virtual seat; popups in their own X window are
  composited in. Use `--wait 9` there (the window is mapped late) and spend
  the first click on an empty spot (`--click 1000,600`): the X11 window gets
  focus and pointer only from a click, so the first one never reaches a widget.
  Never inject input on the session's own display: on rootless Xwayland it
  lands on the real desktop and the capture can include other apps' windows
  (the tool refuses `--click` without `--nested` for that reason).
  Drag-and-drop from a file manager: `tools/dnd-probe.py target/debug/attic
  <file> 800 500 1200 650` runs the same private mutter with a GTK 4 drag
  source (`tools/dnd-source.py`, copy/move/link/ask like Nautilus, deletes
  the file after a move) and prints the source's events, the app's
  `wl_data_*` protocol lines and the note's image lines. The seat needs a
  keyboard before the app starts or the clipboard library never creates a
  data device; the probe does that.
- The DnD destination accepts Copy only (`.action(Copy)`,
  `.preferred_action(Copy)`): libcosmic's default prefers Move, and a file
  manager then deletes the original the moment the drop finishes, before
  the copy into `assets/` has happened. Never advertise Move there.

libcosmic is pulled from git; its API moves. When in doubt, read the checkout
under `~/.cargo/git/checkouts/libcosmic-*/` rather than trusting docs.

## Layout

Paths below are relative to `crates/jjb-core/src/` for the core modules
(note, store, sync, markdown, images, links, table, platform) and to
`apps/jjb-cosmic/src/` for everything else. The app keeps `crate::note`,
`crate::store` etc. working through `use jjb_core::{…}` in `main.rs`;
`apps/jjb-cosmic/src/markdown.rs` and `images.rs` re-export the core
module and add the iced-only parts (span colours and fonts, drag payload,
palette → inks). The app's `i18n/`, `resources/` (fonts, icons, launcher
files) and `build.rs` live in `apps/jjb-cosmic/`.

- `src/note.rs` — document format (frontmatter + markdown body) and pure
  functions: title derivation, preview stripping, `#tag` and `[[link]]`
  extraction, filename slugs. Fully unit tested; keep it that way.
- `src/store/` — `fs.rs` (notes dir, atomic writes, `.trash/`), `db.rs`
  (rusqlite schema, FTS5 search, tag/link queries, oplog), `mod.rs` (the
  `Store` that ties them: reindex-from-disk, create/save/trash/restore).
- `src/app.rs` — the libcosmic `Application`: framed views/tags/notes/editor,
  dock, theme picker drawer, autosave, key binds.
- `src/retro.rs` — palettes, btop-style `frame`, style classes, swatches.
- `src/markdown.rs` — per-line markdown scanner and span → colour/font table.
- `src/editor/` — the rich editor: `content.rs` (cosmic-text buffer + the
  editor API the app uses), `style.rs` (span → attributes, active line vs
  hidden markers), `widget.rs` (drawing, caret, mouse, keys, IME, Ctrl+click).
- `src/images.rs` — image reference format, assets store, retro pixel treatments.
- `src/links.rs` — link cards: `[text](url)` / bare address / attached file on a line of its own; Open Graph scrape (`ureq`), preview cache under `assets/.links/` (derived, disposable), `JJB_LINK_FIXTURE=page.html` serves a local page instead of the network.
- `src/blocks.rs` — the note body as text/image blocks (one editor per text run).
- `src/glyph.rs` — folder icons a tag can wear instead of `#`: seven styles (Boxicons, Iconoir, Solar, Myna UI, Majesticons, Pixelarticons, Duoicons; bundled as SVG paths from Iconify, Boxicons is the fallback), chosen in Options → Icon. Generated — regenerate rather than hand-edit the bodies.
- `src/dockicon.rs` — the dock's action icons in the same seven sets, drawn in
  the user's chosen set with Boxicons as the fallback. Generated by
  `tools/dock_icons.py` (edit its table, rerun, never hand-edit the bodies;
  it writes to `apps/jjb-cosmic/src/dockicon.rs`).
- `src/icon.rs` — the launcher icon generated from a palette and written into
  the user's hicolor theme.
- `src/proof.rs` (core) — spelling and grammar on Harper, one line at a
  time: `load()` once off the UI thread, `check_line` answers `None` until
  then. The app's dock S/G toggles set `markdown::Settings::proof`; the
  editor caches per line and draws `Overlays::proof` underlines.
- `src/platform/` (core) — the one trait that differs per OS: default
  notes dir, data dir for `index.db`, keyring, open URL. `linux.rs` is
  Secret Service (blocking) + XDG dirs; `windows.rs` is the stub for
  everything else (keyring comes in Phase C). Single instance is the
  shell's job, not the trait's.
- `crates/jjb-md-wasm` — `scan_line` / `scan_text` / `kinds` exported
  through wasm-bindgen, JSON strings in and out; `tests/parity.rs`
  compares its output with the native scanner.
- `src/config.rs` — cosmic-config entry (`notes_dir`, `device_id`); path
  defaults come from the core's platform module. `notes_dir` is chosen in
  Options → Notes folder (the in-app picker in folder mode,
  `picker_folders`); `App::switch_notes_dir` reopens the store there. "" means
  the platform default.
- `src/watch.rs` — the notes folder watched (`notify`, recursive, 500 ms
  settle) for changes made by someone else: a sync client such as Dropbox
  bringing another machine's edits. `App::external_change` re-indexes and
  keeps the open note honest by the server sync's rules (edit beats delete;
  both sides changed keeps the typing here and files the other version as
  a conflict copy via `Store::keep_disk_copy`). Our own writes come through
  it too, as a cheap no-op re-scan.
- `src/desktop.rs` — which desktop we are on (`XDG_CURRENT_DESKTOP` and
  the older session variables; with none set, whoever owns
  `com.system76.CosmicSettingsDaemon` / `org.gnome.Shell` on the bus); off
  COSMIC, fonts / icon theme / title-bar buttons are read from the portal
  (`org.gnome.desktop.*`) and pushed into libcosmic's `COSMIC_TK` by
  `reassert()` (called from the header/view because libcosmic overwrites
  that struct on its config update). `watch()` is the app subscription
  that follows the portal's `SettingChanged` (own thread + runtime) and
  `apply()` adopts a change live, icon theme included. The accent colour
  (`org.freedesktop.appearance accent-color`) is read the same way, since
  libcosmic never asks the portal for it; `softened_theme()` rebuilds the
  system theme around it and GNOME's window outline whenever
  `outline_stale()` finds libcosmic has installed a fresh one. The ignored
  `portal_signal_probe` test checks the signal path against a real session.
- `src/launcher.rs` — `attic --launcher-plugin`: the COSMIC launcher
  (pop-launcher) plugin, the GNOME search provider's twin: JSON lines on
  stdin/stdout, `note <words>` lists hits from the read-only index, picking
  one launches the app on the file. `resources/launcher-plugin.ron` is the
  manifest; the installers write it plus an `attic-launcher` wrapper script
  under `pop-launcher/plugins/attic`. Logs go to stderr in this mode.
- `src/search_provider.rs` — `attic --search-provider`: the headless
  GNOME Shell search service (`org.gnome.Shell.SearchProvider2`), answering
  from `store::Index` (read-only index.db) and launching the app to open a
  hit. `resources/search-provider.{ini,service}` are installed by the
  justfile / install.sh. The app is single-instance: `app::Flags` carries
  files / `--search` / `--new` to the running instance over D-Bus
  (`dbus_activation`); set `COSMIC_SINGLE_INSTANCE=false` to bypass
  (xshot does).
- `src/secrets.rs` — keyring wrapper; holds the sync token.
- `src/sync.rs` — PocketBase client and one blocking sync cycle (`run`):
  refresh token, pull since cursor, push; `Envelope` is the opaque payload
  (file text + trashed/deleted). `src/store/sync.rs` is the store's half:
  `sync_pending` (hash diff vs `sync_state`), `apply_remote` (adopt /
  delete / conflict copy), `apply_outcome`. `src/sync/files.rs` is the
  other half of the folder (`assets/`, `.folders`) through a `files`
  collection: it moves the bytes itself from the blocking thread; the
  store only records `sync_files`, repoints notes at renamed files and
  merges `.folders`. `server/` has the PocketBase
  migration + revision hook and a setup README. The two-device
  integration test runs only with `JJB_PB_URL=http://127.0.0.1:8090`
  pointing at a PocketBase started from `server/pocketbase/`.

## Rules that matter

- Every commit updates `CHANGELOG.md` under **Unreleased** (Added / Changed /
  Fixed / For developers) in plain words for the person using the app, and
  the commit body carries the same notes. The user asked for this on
  2026-11-08 so the history reads as release notes. `DECISIONS.md` stays the
  place for the reasoning.
- Files on disk are the source of truth; `index.db` is derived and disposable.
  Never add state that lives only in SQLite (except the sync oplog).
- Nothing from libcosmic, iced or zbus goes into `crates/jjb-core`; the
  core's `markdown` and `table` must stay pure enough for the WASM build
  (no `full`-feature crates). New editor logic that is not about pixels
  belongs in the core so the Windows app gets it too.
- Never run `cargo fmt` on the whole workspace: the tree is not fmt-clean
  and it rewrites untouched files. Format new files only.
- The sync payload will be the full file text; title/body/tags must stay
  inside it. Server-visible metadata is id/revision/timestamp/device only
  (for files: a hash of the path, never the name).
- Retro styling belongs to the editor surface and themes only. Chrome stays
  stock COSMIC (which, on GNOME, means stock libcosmic wearing GNOME's fonts
  and icons — never GTK, never a second toolkit), but on the note theme's
  side: a light theme means light chrome (`App::chrome_dark` →
  `desktop::set_chrome_dark`; the "follows your desktop" theme hands the
  choice back to the desktop).
- Autosave is debounced (`AUTOSAVE_DELAY`); anything that switches or
  drops the current note must call `close_current()` / `flush()` first.
- The script harness uses the real cosmic-config: a step that writes a
  setting (`notesdir`) needs `XDG_CONFIG_HOME` pointed at a scratch dir or
  it changes the user's real app.
- Sync never clobbers: both-sides-changed becomes a conflict copy, edit
  beats delete, the open note auto-updates to the server version; a
  different file of the same name makes the local one step aside
  (`name-2.ext`) and its notes follow. Test
  runs that sign in write the token into the real keyring
  (`secret-tool clear application attic key sync-token` afterwards)
  and must set `XDG_DATA_HOME` too, since `sync_state` lives in index.db.
