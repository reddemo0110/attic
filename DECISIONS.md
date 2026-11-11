# Decisions log

Running record of decisions made during implementation. The handover doc
(`Project Handover — …md`) holds the locked-in architecture; this file
records the open questions as they get settled, plus anything discovered
while building. Newest at the bottom.

## 2026-08-29 — Open questions from the handover, settled

### App name: **JotJotBoom** (working name, from the project folder)
App id `io.github.jotjotboom.JotJotBoom`, binary `jotjotboom`. Change with a
single `grep -rl` / `sed` pass — it appears in `Cargo.toml`, `justfile`,
`build.rs`/xdgen metadata, and `src/app.rs` (`APP_ID`).

### Storage: **markdown files on disk + SQLite as a derived index**
- Notes live as `<title>.md` under a user-visible notes directory
  (default `~/Documents/JotJotBoom/`; configurable via cosmic-config).
- Each file starts with a small YAML frontmatter block carrying what the
  filesystem can't: `id` (UUIDv7), `created`, `pinned`. Title is the H1 /
  first line. Body below is the source of truth.
- Filename tracks the title (slug-safe); renames happen on save with a
  numeric suffix on collision. Trash = moved to `<notes dir>/.trash/`.
- SQLite (`$XDG_DATA_HOME/jotjotboom/index.db`) holds `notes`, `tags`,
  `note_tags`, `links`, `oplog`, and an FTS5 table. It is rebuilt from disk
  on startup (mtime + content hash diff) and written through on every save,
  so deleting the DB is always safe.
- Why files: grep/Syncthing/git-friendly for Linux users, per the handover's
  lean. Why an index anyway: FTS5 search, backlinks, tag tree, and the oplog
  the sync design needs.
- Sync payload implication: the opaque blob is the full file text
  (frontmatter + body). Title/body/tags all live inside it — satisfies the
  E2E metadata boundary. Plaintext side-channel is id/revision/timestamp/
  device only.

### Note list previews: **markdown-stripped text**
First ~120 chars of the body with markdown syntax removed, whitespace
collapsed. Computed on index write, stored in the `notes` table so the list
never re-parses.

### Attachments / images: **deferred** (not in v1)
Wiki-link and tag parsing will ignore `![…](…)` so nothing breaks when they
arrive later.

## 2026-08-29 — Environment notes
- Machine runs COSMIC 1.7 on Wayland; the app can be launched locally.
- No sudo available from the agent session. Rust installed via rustup into
  `~/.cargo`; `just` and `cargo-generate` via `cargo install`. `cmake` is
  not installed system-wide — if a crate demands it, drop a Kitware release
  tarball into `~/.local` or `sudo pacman -S cmake`.

## 2026-08-29 — Verification tooling (after step 1 landed)
- Unattended UI checks: `tools/xshot.py out.png --script '...'` runs the app
  on Xwayland, drives it via the `JJB_SCRIPT` env hook (`src/debug_script.rs`),
  and grabs the window with XGetImage. Chosen because every other route failed
  without a human: portal screenshots hang waiting for approval, XTEST under
  Xwayland blocks on the libei portal, and iced's own `window::screenshot`
  (the `JJB_SCREENSHOT` hook) silently drops text-editor contents and menu
  labels — it looked like two rendering bugs that didn't exist.
- Both hooks are env-gated and cost nothing when unset; they stay in the
  binary for now so the CRT/editor work can be checked the same way.
- Steps 1 and 2 of the build order are done together: FTS5 search, tag
  parsing and the nested tag tree fell out of the storage design cheaply.

## 2026-08-29 — Retro direction: Option A (full terminal), btop-inspired
User feedback after seeing step 1: "needs more retro with a hint of btop".
Two directions were mocked up on a design canvas; the user picked **A**:
everything below the COSMIC header bar lives in btop-style frames (1px
rounded border, title cut into the top edge, right-hand badge), monospace
throughout, tag tree drawn with `└─` connectors. This supersedes the
handover's "retro confined to the editor surface" rule — the header bar and
window controls stay native COSMIC, the content area does not.
- Themes: green phosphor (default), amber, WordPerfect blue, and a
  "COSMIC" variant that derives its palette from the system theme. Chosen
  under View, persisted in cosmic-config (`theme`).
- Title face: VT323 (SIL OFL, bundled in `resources/fonts/`); body stays the
  system monospace for now — bitmap editor fonts are still a step-5 item.
- Built-in libcosmic nav bar replaced by our own two frames (`views`, `tags`)
  so the nav could be styled; `nav_model()` returns None.
- Still to come from the mockup: CRT scanline/glow overlay (needs a custom
  shader widget — step 5) and focus mode (needs the hybrid editor — step 3).

## 2026-08-29 — Dock and folders
- **Dock**: a floating pill at the bottom of the content area with the
  markdown format actions (bold, italic, code, H1, H2, bullet, to-do,
  wiki-link, tag, rule) and a `+`. Wrapping actions wrap the selection or
  insert an empty pair with the cursor inside; line actions toggle the
  prefix on the current line (and swap one list/heading prefix for another).
  Focus returns to the editor afterwards.
- **Folders are tags.** `+` → `folder` creates a tag with no notes yet. They
  are persisted in `<notes dir>/.folders` (one per line) — not in the
  index, which stays disposable — and merged into the tag tree with a count
  of 0. Creating a note while a folder is selected pre-fills `#folder` on
  its own line; a line made only of tags is neither the title nor "content",
  so an abandoned pre-filled note is still dropped and its title stays
  Untitled until the user types one.
- Folder names are normalised to tag form (lowercase, spaces → `-`,
  `parent/child` allowed). Removing a folder is `Store::remove_folder`, not
  yet exposed in the UI.

## 2026-08-29 — Inline markdown rendering and eight retro themes
- **Rendering without a custom editor.** iced's `text_editor` accepts a
  per-span highlighter (colour + font, no size). `markdown.rs` scans each
  line — headings, quotes, lists/tasks, fences, rules, `**`/`*`/`` ` ``/`~~`
  emphasis, `[[wiki]]`, `[md](links)`, `#tags` — and paints bold as bold,
  italic as italic, headings in the accent, tags/links in the second accent,
  done tasks dimmed, and every syntax marker in a near-invisible "ghost"
  colour. The markers still occupy space (the cursor walks over them), and
  headings cannot be larger than body text. Both limits go away with the
  cosmic-text editor in step 3; this gets 90% of the hybrid-editor feel today.
  View → "Show markdown syntax" paints markers dim instead of ghosted.
- **Nine palettes**: eight retro (green phosphor, amber, WordPerfect blue,
  paper white, plasma orange, Commodore 64, Game Boy, synthwave) plus a
  COSMIC one derived from the system theme. Picker lives in the context
  drawer (View → Theme colours, or the `◐` on the dock) as swatch cards
  drawn in each theme's own colours.
- Gotcha recorded: `JJB_SCRIPT` splits on `;` — escape as `\;` in note
  text or the tail of the step is silently dropped. Multi-line `type:` now
  types one line per tick so the highlighter sees keystroke-like edits.

## 2026-08-29 — Theme names are 80s action-movie puns (user request)
Keys in config are unchanged (`phosphor`, `amber`, …); only labels/blurbs
moved. Contrast pass at the same time: `dim` lifted on phosphor, amber,
WordPerfect, plasma and C64 (previews/timestamps were ~3:1), Game Boy accent
separated from its foreground.

## 2026-08-29 — Collapsible panes, dock inside the editor
- Views/tags column and notes list each toggle (header buttons, View menu,
  Ctrl+1 / Ctrl+2); Ctrl+0 / View → Editor only collapses both or brings
  both back. Persisted (`hide_nav`, `hide_list`, inverted so the default
  is shown).
- The dock moved into the editor frame as a centred pill at its foot; the
  `+` section opens as a second pill underneath so the main row never
  overflows. Notes list narrowed 340 → 320 so the dock fits at the 1024px
  minimum-ish width.
- Icon lookup only searches the Cosmic/Pop themes (Adwaita-only names such
  as `view-pin-symbolic` silently render nothing) — use `pin-symbolic`,
  `sidebar-places-symbolic`, `view-list-symbolic`.

## 2026-08-29 — Images
User picked *all* frame styles from the canvas, so every one is a per-image
option; placement is a setting with all three feasible layouts.
- **Format**: whole-line `![alt](assets/x.png){frame=tint size=m}`; the
  braces are Pandoc-style attributes, ignored by other markdown tools.
  Default frame/size are omitted so a plain `![alt](path)` stays plain.
- **Assets** live in `<notes dir>/assets/`, copied on import with readable
  unique names (`my-photo.png`, `my-photo-2.png`). Import via drag-and-drop
  onto the window or the `⧉` dock button (portal file picker). Clipboard
  paste is not wired yet (`images::import_bytes` is ready for it).
- **Treatments** (`images.rs`): tint (luma → bg…accent ramp), dither
  (Floyd–Steinberg to bg/mute/dim/fg), pixel (nearest to ~96px and back),
  bezel (every 3rd row darkened + vignette, in a rounded shell), ASCII
  (72 cols, accent text), print (off-white card + VT323 caption), film
  (sprocket rows + frame number), box (btop frame + size badge). Images are
  downscaled to ≤720px, processed on a blocking thread, cached by
  path|mtime|style|theme; theme-dependent styles re-render on theme change.
- **Placement**: View → Images: right rail (default) / top strip / bottom
  strip, persisted; a note can override with `images: top` in frontmatter.
  Sizes S/M/L per image. Each card has chips: cycle frame, cycle size, open
  in the system viewer. Inline flow waits for the custom editor.

## 2026-08-29 — Image picker and drag-and-drop, second pass
- The xdg-portal file chooser (`cosmic::dialog::file_chooser`) never
  returned on this COSMIC 1.7 session — no error, no dialog, the future just
  hangs — so the `⧉` button now opens an **in-app picker** in the context
  drawer: Home / Pictures / Downloads shortcuts, `..`, folders first, then
  image files. Self-contained, no portal dependency.
- winit's `FileDropped` only fires on X11. On Wayland libcosmic delivers
  drops through `widget::dnd_destination`; the editor frame is now wrapped
  in `dnd_destination_for_data::<UriList>` (`text/uri-list`, percent-decoded
  `file://` URLs) with a "drop to add" badge while hovering. Both paths stay
  so the X11 capture harness and real Wayland use both work.
- Lesson: the script harness proves logic, not integration with the
  compositor/portals — anything touching those needs a hands-on check.

## 2026-08-29 — Inline images, magazine style (third pass)
User: the strip "looks like an attachment to an email"; wants pictures sitting
in the text, resizable, with a click-for-options menu.
- **Block editor** (`blocks.rs`): the body is split at image lines into
  text runs and images; every text run is its own text editor widget, images
  render between them exactly where their `![…]` line is. Cursor/selection
  do not cross blocks (images are atomic blocks); ↑/↓ at a block edge hop
  over the picture; Backspace at the start of the block below an image
  deletes it. The body is re-joined on save, so files are unchanged in form.
- `align=left|right` puts the paragraphs that follow the image beside it
  (a two-column region until the next image); `center` is a full-width block
  capped by `w=`. Widths are pixels: drag the `◢` grip (global mouse
  tracking, committed on release) or pick a preset in the menu.
- The `⋯` menu is a popover: 8 frame styles, sits left/centre/right, width
  presets + full, open, remove. Clicking the picture opens it too.
- Rail/top/bottom placements removed. Empty text runs around images
  contribute no lines when joining (a blank line between two adjacent
  images is normalised away — the one known lossy case).
- Gotcha: a text editor emits `ClearSelection` when it loses focus; only
  real interaction (click/drag/edit/move/select) may claim block focus.
- Gotcha: `retro::frame` was height Fill; inside a shrink row that collapses
  to nothing → `frame_sized(.., Length::Shrink, title_size)` for cards.

## 2026-08-29 — Image style feedback
- ASCII now follows the width like every other style: columns are derived
  from the display width (`ascii_layout`), glyph size fills it, and the
  render is cached per column count so resizing re-renders.
- The ⋯ menu has a **caption / title** field. It edits the image's alt text
  (`![caption](…)`), which is what the instant print prints and what the box
  frame shows as its title — one name, stored in plain markdown.

## 2026-08-29 — Undo / redo
- Note-level undo: a step is a snapshot of the whole body + caret (blocks
  are rebuilt on restore). 200 steps per open note (`UNDO_DEPTH`), stacks
  reset when switching notes. Redo cleared by any new edit.
- Grouping: consecutive typing (insert/enter/indent) or consecutive deleting
  within 700 ms is one step; switching between the two, pausing, pasting,
  dock format actions, image insert/remove/style/width/caption edits and
  Backspace-over-image each start a new step (caption typing groups).
- Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y and an Edit menu. The stock text editor has
  no undo of its own, so this is the only undo.

## 2026-08-29 — Theme pass 2 (user feedback)
- Muted the loud backgrounds: Handhelder is now real-DMG olive
  (#a9b58a) instead of emulator lime; Little Blue's #0000aa softened to
  #1c2874 with gentler yellow/cyan; Escape from C64 desaturated slightly.
- New **Ghostwriters**: plain white (#ffffff / #fbfbfb panels) with
  #0c0c0c ink — never pure black — and a muted steel blue for tags/links.
- Five COLOURlovers-derived themes (hex values from the palettes, dark
  grounds derived from their deepest colour): Big Fish in Little China
  (Giant Goldfish), Blade Thinker (Thought Provoking), Cheer Up, Karate Kid
  (Cheer Up Emo Kid), The Abyss Five (Ocean Five), Adrift in Dreamscape
  (Adrift in Dreams). Blurbs credit the source palette.

## 2026-08-29 — Little Fish in Big China
Copy of Big Fish in Little China with the tangerine accent muted
(#fa6900 → #c9773f); every other colour identical. User request.

## 2026-08-29 — Shortcuts for everything
- One bind map (`key_binds`) is the single source: menus print the shortcut
  beside each item (libcosmic does this from the map), header/dock tooltips
  append it ("Bold · Ctrl + B"), and Ctrl+Shift+H opens a full-window
  overlay generated from the same map; × or Esc closes it.
- Layout: File — Ctrl+N new, Ctrl+Shift+N folder, Ctrl+Shift+I image,
  Ctrl+F search, Ctrl+Shift+P pin, Ctrl+Shift+D trash. Edit — Ctrl+Z,
  Ctrl+Shift+Z / Ctrl+Y. Format — Ctrl+B/I/E, Ctrl+1/2 headings, Ctrl+L
  bullet, Ctrl+T to-do, Ctrl+K link, Ctrl+Shift+3 tag (# lives on 3),
  Ctrl+R divider. View — Ctrl+Shift+1/2 panes, Ctrl+Shift+0 editor only,
  Ctrl+Shift+C themes, Ctrl+Shift+M markdown syntax, Ctrl+Shift+H this.
  Pane toggles moved from Ctrl+1/2 to Ctrl+Shift+1/2 to free the digits for
  headings. A new Format menu mirrors the dock.

## 2026-08-29 — Save indicator
- The editor badge no longer flips "editing… / saved hh:mm". It is a quiet
  `✓` (autosave is assumed); clicking it shows "✓ saved hh:mm" for four
  seconds. If a save fails the badge becomes "✗ not saved — last saved
  hh:mm" (tooltip has the error) and the edit stays pending with a retry
  every ~5 s until a save succeeds.

## 2026-08-29 — App icon and per-user install
- Icon: 128px SVG, no text (icon renderers can't be trusted to have our
  fonts): phosphor-green terminal frame with a two-dot "jot jot" title notch,
  three text lines, a block cursor, and an 8-point burst breaking out of the
  frame for "boom". Reads at 48px.
- `just install-user` installs without sudo into `~/.local` (binary,
  desktop entry with an absolute `Exec` because `~/.local/bin` is not on this
  machine's PATH, icon under hicolor/scalable/apps, metainfo) and refreshes
  the desktop/icon caches. The template's `install` recipe had the icon
  destination pointing at the directory, not the file — fixed.

## 2026-08-30 — Comic frame and drag-to-move pictures
- New frame style `comic` ("comic book"): a 45° halftone screen (5 px
  cells) over the photo, slightly desaturated (chroma × 0.8) to account for
  the ink — dot size follows the tone at each cell centre, paper shows
  between the dots, and each dot is a darker ink of the colour under it.
  Two heavier versions were built and thrown out: posterise + Sobel + black
  dots (muddy), then the full Photoshop/Toonify pipeline (bilateral smooth,
  auto-levels, cel bands, hysteresis outlines, Ben-Day shadows) — the user
  judged the flattening and outlines "terrible" on real photos, so the
  treatment is just the halftone. The panel is a thick ink border on a
  newsprint gutter; a caption becomes a yellow narration box in the
  top-left corner. Not palette-dependent.
- A picture is moved by dragging it (press, then travel > 6 px; a plain click
  still opens the ⋯ menu). While dragging, the note switches to a "drop
  mode": each body line is drawn as plain text with a top half (drop above)
  and a bottom half (drop below), so the drop line — an accent rule tagged
  "▼ picture drops here" — is drawn exactly where the picture will land, no
  pixel-to-line guessing. Dropping mid-paragraph splits it; the end of the
  note is reachable via a strip under the last line. Esc cancels; the move
  is one undo step. Rejected: estimating the target line from cursor y over
  the live editors (wrapping makes it lie) and a ghost image that follows
  the pointer (window-to-editor coordinates are not available to `view`).
- Script hook: `imgdrag:n:line` shows drop mode with the line before body
  line `line`; `imgmove:n:line` performs the move.

## 2026-08-30 — Flat layout: hairlines, not gaps; foldable sub-tags
- The user sent a reference screenshot: panes are flat and butt against each
  other, separated by one-pixel lines, no gutters, no boxes. The btop-style
  frames (Option A) are retired for the three columns: `retro::pane` is a
  flat pane with the title (still VT323 in the accent) as a header row and
  no border; `retro::vrule` / `retro::hrule` are the hairlines. The sidebar
  sits on `bg`, list and editor on `panel`, so the tonal step reads like
  the reference's darker sidebar. `frame_sized` stays for the inline "box frame"
  image style. `GAP` is gone.
- Sub-tags fold: a parent tag gets a ▸/▾ chevron; clicking it hides or
  shows everything beneath it. Folded tags persist in cosmic-config
  (`collapsed_tags`, full paths). Default is expanded. Indent is 14 px per
  level instead of the old `└─` prefix. Script step `fold:<tag>`.

## 2026-08-30 — Dock sizes, clean previews, text size, editor fonts
- Dock size: Small / Medium / Large / WOW! (`retro::DockSize`), chosen in
  the Appearance drawer, persisted as `dock_size`. The dock is now a
  `flex_row`, so the big sizes wrap onto extra lines instead of running
  off the pane.
- Titles and previews only ever contain typed text: `strip_inline_markup`
  swallows an image's `{frame=… w=…}` attributes with the image, the title
  is the first line that still says something afterwards, and the preview
  starts after that line (not after "the first non-blank line", which was
  wrong when a note opened with a picture). `SCHEMA_VERSION` bumped to 2 so
  the derived index rebuilds and stale previews disappear.
- Editor text size: − / + at the foot of the sidebar, 1 px a step,
  10–48 px, persisted as `editor_font_size` (0 = default 15). Applies to
  every text block and the drag-mode lines.
- Editor fonts: eight bundled faces (OFL/UFL, `resources/fonts`) plus the
  system monospace — IBM Plex Mono, Fira Mono, Ubuntu Mono, Anonymous Pro,
  Space Mono, Courier Prime, B612 Mono, VT323. Static Regular/Bold/Italic
  files were chosen over variable fonts because cosmic-text picks faces by
  weight and would render a variable font's bold as regular. Picked in the
  Appearance drawer, persisted as `editor_font`. ~3.8 MB in the binary.
- Bug found on the way: `iced::font::load` issued from `init()` is
  silently dropped when the compositor does not exist yet (winit's
  `Action::LoadFont` is `if let Some(compositor)`), so the VT323 title font
  had never actually rendered under the xshot harness. Fonts are now also
  loaded on `window::Event::Opened`; already-loaded slices are skipped.
- Script steps: `font:<key>`, `fontsize:+n|-n`, `docksize:<key>`.

## 2026-08-30 — Appearance drawer: Colour / Font / Size, beside the note
- The drawer is now three foldable sections (▸/▾ headers, Colour open by
  default): Colour (themes), Font (the nine editor faces), Size. Size has a
  card per text pane — Sidebar, Notes list, Editor — each with a live
  sample in that pane's font and − / + (1 px a step; sidebar and list
  9–30 px, editor 10–48 px), then the dock size row. Persisted as
  `sidebar_font_size`, `list_font_size`, `editor_font_size` (0 = default;
  13 / 13 / 15). The − / + footer that lived at the foot of the sidebar is
  gone — the user judged text size an occasional setting, not a control.
- The views pane's fixed height now follows the sidebar size so "Trash"
  never clips.
- `core.window.context_is_overlay = false`: libcosmic lays the drawer out
  beside the content instead of over the note being written. On a narrow
  window the editor column gives way to the drawer; on a wide one both fit.
- Script steps: `size:<sidebar|list|editor>:±n`, `section:<colour|font|size>`.

## 2026-08-30 — Rename a tag everywhere
- Right-click a tag in the sidebar → a small popover with "rename…"; that
  swaps to an inline input pre-filled with the full path (`travels/japan`).
  Enter commits, Esc / click-away cancels. `note::rename_tag` rewrites
  `#old` and every `#old/…` in a body (same scanner as `extract_tags`, so
  code fences, inline code and `#oldx` are left alone; the user's casing on
  the unrenamed remainder is kept). `Store::rename_tag` applies it to every
  file (trash included), re-indexes each changed file, and renames folder
  entries under the old path. The open note is flushed first and reloaded
  afterwards if it carried the tag; the current view and the folded set are
  remapped. Unit-tested at both layers.
- Script steps: `tagmenu:<tag>`, `renametag:<old>:<new>`.

## 2026-08-30 — Arrow navigation, real rules, clickable task boxes, format review
- ↑ / ↓ that nothing consumed (the editor is not focused — e.g. right after
  clicking a note) open the note above / below, like a mail client, and the
  list snaps so the selection stays in view. Implemented on the event
  subscription's `Status::Ignored`, so the editor's own caret movement is
  untouched.
- `---` / `***` / `___` lines are blocks of their own (`Segment::Rule`,
  `Block::Rule`), drawn as a full-width 2 px line in the muted colour.
  Typing one (Enter after `---`) or pressing the rule button re-splits the
  block live with the caret preserved (`Blocks::resplit`); Backspace at the
  start of the text below removes it, like an image. Fenced `---` stays
  text.
- Task boxes: clicking `[ ]` / `[x]` flips it (one undo step). `[x]` is
  drawn in the accent, bold; the finished text is the theme foreground at
  45 % alpha. What the stock iced text editor cannot do — a real tick
  glyph, a strike-through line, hidden markers, heading sizes — is exactly
  the custom rich-editor milestone (build step 3); the highlighter can only
  set colour and font per span.
- Format buttons reviewed: bold / italic / code / link now unwrap when the
  selection is already wrapped; heading / bullet / to-do apply to every
  line a selection touches (the first line decides add vs remove) and
  recognise `* ` / `+ ` / `[X]` prefixes; the rule button always puts the
  rule on a line of its own.
- Script steps: `nav:±n`, `togglebox:<line>:<col>`.

## 2026-08-30 — `[]` makes a task; pick your own "done" mark
- Typing `]` right after `[` at the start of a line (indent and a `- `
  allowed) expands to `- [ ] ` (`Blocks::expand_task_shorthand`).
- A task box is `[ ]` open, or `[` + one mark + `]` done. The mark lives
  in the file: `- [✓] milk`, `- [🦆] milk`. Everything that reads boxes —
  the highlighter, previews, the click toggle, the dock's to-do toggle —
  goes through `note::task_box`, which accepts any single mark (≤ 12
  bytes, no spaces), so `[x]` from other apps still works. Chosen in the
  Appearance drawer's new Tasks section (x, ✓, ✔, –, •, ★, 🦆, 🔥, 💀, 🍕,
  🐈, 🚀, 👍, 🍺) with a live sample; persisted as `task_marker`
  (empty = x). The hint says plainly that other markdown apps only count
  `x` as done — the user chose visible ticks over strict GFM.
- Script step: `marker:<mark>`.

## 2026-08-30 — The measure: a centred text column
- Like Craft and other reading-first editors (the user sent references), the note text
  lives in a column that stops growing at a chosen width and centres with
  margins; a narrower pane just wraps the text to fit. Implemented as a
  `max_width` container around the block column inside the editor's
  scrollable, so images, rules and drop-mode lines follow the same measure.
- Four widths, rated in fish in the Size section: 🐟 560 px, 🐟🐟 720 px
  (default), 🐟🐟🐟 920 px, 🐟🐟🐟🐟 no limit. Persisted as `text_width`.
- Script step: `measure:<key>`.

## 2026-08-30 — Designer font pairings
- Research (Typewolf, Pagecloud, Inspotype, Fontfabric, Readium's libre
  font list) agrees on the recipe: a display or serif for headings over a
  calm sans for UI, or one superfamily throughout; Atkinson Hyperlegible
  and Lexend are the accessibility picks; Lato + a serif, PT Sans + PT
  Serif, and Plex are the canonical free pairs. Only families with static
  Regular/Bold/Italic files were bundled (cosmic-text ignores variable
  axes): IBM Plex Serif, Lato, PT Sans, PT Serif, Atkinson Hyperlegible,
  Ubuntu, Spectral, DM Serif Display, Special Elite. Skipped for being
  variable-only: Playfair Display, Libre Baskerville, Merriweather, Source
  Sans/Serif, Literata, Lexend, Cormorant. Fonts are now ~13 MB of the
  binary.
- Eight pairings (`retro::PAIRINGS`), each setting three faces — pane
  titles, sidebar + list, note: JotJotBoom (default: VT323 / system mono /
  system mono), Plex, Editorial (Spectral + Lato), Magazine (DM Serif
  Display + Lato), ParaType (PT Serif + PT Sans), Hyperlegible, Ubuntu,
  Typewriter (Special Elite + Courier Prime). Persisted as `title_font`,
  `ui_font`, `editor_font`; "Restore default fonts" re-applies the first.
  Titles are set bold except for faces that only ship a Regular. The
  single "Editor font only" list stays underneath for mixing.
- Script step: `pairing:<key>`.

## 2026-08-30 — Rich editor: phase-0 spike passed
- On branch `rich-editor`. A throwaway widget (`src/editor/spike.rs`,
  `JJB_SPIKE=1`) proved every unverified assumption in the plan: per-span
  sizes, seam-free collapsed markers (transparent, 0.5 px), overlays from
  layout metadata, strikes from cosmic-text's own decoration spans, drawn
  through `fill_raw`. Findings and exact API notes are in
  `RICH-EDITOR-PLAN.md`. `cosmic-text = "0.19"` is now a direct dependency
  (same version iced uses). Next: phase 1, the real widget behind a
  `rich_editor` config flag.

## 2026-08-30 — Rich editor phase 1: the widget, behind a flag
- `Content` became an enum over iced's content and the new `RichContent`
  so the two editors coexist; new blocks pick the variant from the
  `rich_editor` flag (config, Appearance → Font toggle, `rich:on|off`).
  Default off until phase 2 renders more than the stock editor did.
- The widget copies iced's `text_editor` behaviour (click kinds, key
  table, focus operation, caret blink) but draws the shared buffer with
  `fill_raw`, which is what lets phase 2 add sizes, collapsed markers and
  overlays. Details in `RICH-EDITOR-PLAN.md`.

## 2026-08-30 — Rich editor phase 2: the formatted look
- Behind the same `rich_editor` flag the note now renders formatted:
  sized headings, hidden markers everywhere but the caret's line, drawn
  bullets and task boxes (`x` shown as a ✓, other marks as themselves),
  quote bars, code backgrounds, strike-throughs. Files are unchanged. The
  two hiding modes (collapsed vs transparent-at-width) and the overlay
  order are recorded in `RICH-EDITOR-PLAN.md`.

## 2026-08-30 — Rich editor phase 3: Ctrl+click and hover
- Ctrl+click on a `[[wiki link]]` opens that note (by title), on a `#tag`
  selects that tag; plain click keeps placing the caret so editing a link
  never fights with following it. Hover shows a hand over task boxes, and
  over links/tags with Ctrl held (plus an underline). Task clicks count
  from the list marker onwards, matching where the box is drawn.

## 2026-08-30 — Quote in the dock
- `> ` lines (the bar with italic text) get a dock button ❝, a Format-menu
  entry and Ctrl+Shift+Q, toggling like the other line prefixes.

## 2026-08-30 — Rich editor: raw markdown on double-click only
- Single click places the caret and keeps the line rendered; the line's
  markdown is revealed by double-click, by typing a marker character
  (`* _ ` ~ [ ] > #`), or by a Backspace/Delete that would eat a hidden
  marker (deleting plain text keeps the line rendered), and hides again
  when the caret leaves. Plain letters keep the line rendered. After `[]`
  expands to a task box, or a box is toggled, the line renders at once
  (`RichContent::render_now`), so a new to-do shows its box before its text.

## 2026-08-30 — Rich editor phase 4: only editor, merged
- IME preedit wired like iced's editor. The stock `text_editor` path,
  the highlighter, the flag and the spike are deleted; `Content` is the
  cosmic-text content. v0.2 = first release where the note is rendered
  formatted. Fallback if something goes badly wrong: tag
  `v0.1-pre-rich-editor` still builds the old editor.

## 2026-08-30 — Image picker: thumbnails in a grid
- The in-app picker defaults to a grid of 104×78 thumbnails (decoded off
  the UI thread to 160 px, cached per path for the session) with folders
  as chips above; a button flips to the old list. Script step
  `pickdir:<path>`.

## 2026-08-30 — 8-bit folder icons
- Emoji were rejected as folder icons ("I don't like the emoji style").
  `src/pixel.rs` holds 21 16×16 one-colour pixel glyphs (8×8 first, redrawn at 16×16 the same day: "a little too low res") (coffee, book,
  camera, home, briefcase, music, heart, star, plane, food, bulb, code,
  money, gift, leaf, gear, flag, pin, bug, game pad, beer), drawn in the
  theme's tag colour: as crisp SVG rects in the sidebar and the picker, as
  quads in the editor over the tag's hash. Right-click a tag → "icon…"
  opens the grid; "none" restores the `#`. Assignments are per full tag
  path (a leaf-only match also applies) and persist as `tag=icon` entries
  in `tag_icons`. Coffee words default to the coffee glyph, so the ☕ egg
  became the first icon. Script step `tagicon:<tag>:<key|none>`.

## 2026-08-30 — Coffee
- Three easter eggs, all opt-in by curiosity: a tag whose leaf is a coffee
  word (`coffee`, `espresso`, `flatwhite`, …) wears a ☕ instead of its `#`
  in the sidebar and in rendered notes (`coffee::is_coffee_tag`); typing
  `coffee` into the search box unlocks the hidden "Long Black" theme
  (crema on espresso, `coffee_unlocked` in config, listed after the
  sixteen); Ctrl+Shift+Enter raises a full-window neon sign — a braille-dot
  cup (`coffee::FRAMES`, generated from a pixel grid) whose steam drifts
  through four frames every 170 ms with the odd tube dimming, pink cup,
  cyan steam, VT323 "COFFEE" — any key or click puts it out.

## 2026-08-30 — Settings gear in the sidebar, Alt+,
- The drawer (titled Options) is opened from a ⚙ in the top-right of the
  views pane — tooltip "Settings" — and with Alt+, (Ctrl+Shift+C retired);
  the ◐ left the dock, which is now format actions only.

## 2026-08-30 — Editor header, H1 by default, drop line for new images
- The editor pane no longer repeats the note title in its header (the
  first line is the title); the ✓ save badge stays on the header's right.
- A new note starts as `# ` with the caret after it, so its first line is
  a heading. A body that is only heading markers counts as blank, so an
  untouched new note is still deleted on close.
- Dragging an image in from a file manager shows the same "picture drops
  here" line as moving one: the DnD destination reports pointer motion in
  window coordinates; each text widget records its drawn bounds, the app
  maps y to a line through the widget's own layout (`line_at_y`), and the
  widget draws the rule at that line. On drop the image is inserted at the
  caret and then moved to the target line. Images and rules between text
  blocks get the indicator from the block view, as in drag mode.

## 2026-08-30 — The icon: dot dot hash, in the theme's colours
- Rejected the phosphor-frame icon (title notch, three lines, cursor and a
  burst all merge below 32 px). Six minimal directions were sheeted at
  128→16 px; the user chose "dot dot boom", then a sticker cut (rejected:
  too wide, overlapped dock neighbours), then "dot dot hash" — two jots
  and a `#`, the sign the app runs on — with the jots on the hash's lower
  bar.
- Geometry follows the macOS app-icon grid the reference app uses: tile
  at 824/1024 of the canvas with clear margin, a continuous-curve
  squircle, no outline; the marks sit on the golden ratio (boom/hash
  height = jot × φ², jot→jot × φ = jot→hash). `src/icon.rs` bakes the
  geometry and fills the colours from a palette: the tile is the theme's
  background (lifted 10 % toward the text colour at the top), the marks
  are the note-writing colour — the icon is the note pane in miniature.
  (First cut used the accent as the tile; the user wanted bg + text.)
- The launcher icon is chosen in Appearance → Icon: any theme's colours,
  or "follow the colour theme" (default). The app writes the SVG to
  `~/.local/share/icons/hicolor/scalable/apps/<app id>.svg` and refreshes
  the cache, so the dock and app library follow; the running window's own
  icon updates on next launch. `resources/…/icon.svg` ships the Phosphor
  version for fresh installs. Script step `icon:<theme|follow>`.

## 2026-08-30 — Example notes and the brochure
- `examples/notes/`: eight sample notes (letter, journals, packing list,
  recipe, coffee log, index) with fourteen Unsplash photographs, credited in
  `examples/README.md`; `just install-examples` copies them into the notes
  folder without overwriting. They double as the fixtures for screenshots.
- `docs/brochure.html`: a single-file page (images embedded) telling the
  app's story and features, shot from the examples in the phosphor theme.
  Also published as an artifact for sharing.

## 2026-08-30 — Folder icons: Boxicons instead of pixel art
- The hand-drawn 8×8 / 16×16 glyphs are replaced by Boxicons Solid
  (MIT, github.com/box-icons/boxicons), sixty of them, bundled as SVG path
  data pulled from Iconify (`api.iconify.design/bxs.json`). `src/glyph.rs`
  keeps the old keys (`coffee`, `book`, … `beer`) so saved assignments
  still resolve, and adds cart, car, bell, calendar, envelope, phone,
  moon, sun, cloud, film, pencil, key, lock, brain, cat, dog, palette,
  wrench, trophy, rocket, wine, pizza, bank, medal, truck, bag, movie,
  bookmark, folder, user, pram, paint, tree, ship, train, bed, cake, drink.
  Drawn through the renderer's SVG path with a per-(icon, colour) handle
  cache so nothing is re-rasterised per frame.

## 2026-08-30 — Two folder-icon styles
- Iconoir (MIT, iconoir.com) joins Boxicons as a second style for the same
  sixty meanings; Options → Icon → "Folder icons" switches sidebar, picker
  and in-note icons together (`icon_set` in config). Iconoir has no cat,
  dog or movie camera, so those three fall back to the Boxicons drawing.
  Script step `iconset:<boxicons|iconoir>`.

## 2026-08-30 — Five more folder-icon styles
- Solar (bold), Myna UI (solid), Majesticons (solid), Pixelarticons and
  Duoicons join Boxicons and Iconoir; each set is used in one consistent
  style rather than mixing line and solid variants. Names were matched to
  our sixty meanings by keyword, with a few hand overrides (Myna UI cog for
  gear, boat for ship, telephone for phone; Pixelarticons tools for wrench)
  and bad matches dropped (Solar has no car, Majesticons no food, Pixelarticons
  no medal). Whatever a set lacks borrows the Boxicons drawing — Duoicons is
  only 91 icons, so about half of its folders do.
- Solar is CC BY 4.0, the rest MIT; the Options card shows the licence and
  the README credits all seven.

## 2026-08-30 — Open Sans
- Added as a nineteenth editor face (OFL, Regular/Bold/Italic/BoldItalic
  static instances fetched from Google Fonts' TTF endpoint; full Latin,
  Cyrillic and Greek coverage, ~100 KB each).

## 2026-08-30 — typ.io Open Sans pairings
- Nine pairings drawn from typ.io/fonts/open_sans, each with Open Sans in
  at least one role: Open Sans alone, Montserrat, Playfair, Lora (Montserrat
  / Open Sans / Lora), Bitter (Bitter / Source Sans 3 / Open Sans), Oswald,
  Raleway, Roboto and Old Standard (Abril Fatface / Open Sans / Old Standard
  TT). Brandon Grotesque, Manus and Arial from the same page are not free
  and were left out. Ten new faces bundled (all OFL; Roboto is OFL since its
  2023 re-release); Abril Fatface and Oswald ship without italics, Abril
  without a bold, so titles in it use its one weight. Fonts are now ~19 MB.

## 2026-08-30 — Link cards and attached files (Craft style)
- A web address alone on a line, or `[title](url)`, is a **web card**:
  picture, title, description, `⌁ domain/path`. The line in the file is the
  markdown link, nothing more. The preview comes from the page's Open Graph
  / Twitter / `<title>` tags (scanned, no HTML parser; `ureq`, 1.5 MB page
  cap, 8 MB picture cap) on a blocking thread, and is cached as
  `assets/.links/<hash>.txt` + `.png` — derived data, safe to delete.
- Once a bare address knows its title it is rewritten as `[title](url)` so
  the note itself carries the title and the cache is only a speed-up. That
  edit sets the note dirty but adds no undo step.
- Only whole-line links become cards; a link inside a sentence stays an
  inline link. Cards appear when the line is finished (Enter / paste), so
  typing an address is not interrupted.
- Any other file dropped on the note, or chosen via File → Attach file
  (Ctrl+Shift+A, the image picker listing every file), is copied into
  `assets/` like a picture and written as `[name.ext](assets/name.ext)`: a
  **file card** with a kind badge (PDF, ZIP…), a kind label and the size.
  Click opens it in the system viewer. No PDF page thumbnails yet — that
  needs a renderer and was deferred.
- ⋯ on a card: open, copy link, refresh preview, remove. Backspace at the
  start of the next line removes a card like it does a picture. Cards move
  with the drag-to-line mechanism already used by images
  (`Blocks::move_image` now moves any non-text block).
- Options → Links → "Fetch link previews" (config `link_previews_off`,
  default on): switched off, cards show only the address and the cache is
  still honoured. The hint says fetching tells the site you saved its link.
- Harness: `JJB_LINK_FIXTURE=/path/page.html` serves that file for every
  address (an `og:image` there may be a path next to it); `attach:<path>`
  script step.

## 2026-09-02 — The minimal themes (ten colour cards)
- Ten new themes from the user's colour-card screenshots: Tomato #CE2939,
  Steel Blue #35637C, Goji Berry (Pantone 18-1659, sampled #B22E3D),
  Coquelicot #EC4908, Shamrock Green (Pantone 15-6432, sampled #71A56A),
  Seoul Yellow #F6B65A, Flame of Burnt Brandy #F29538, Blue Fjord
  (Pantone 7454, #648BA8), Azure #0099FF, Mikado #FFC40C, and (added the
  same evening) Apricot #FFAB40. Labels are the
  card names, not action-movie puns; blurbs echo the cards' captions.
- They share one charcoal base (`retro::minimal()`: bg #121214, panel
  #171719, fg #D7D7D5). The card colour appears **only** as: the selection
  highlight (28 % blend over bg), tag and link colour, the editor caret,
  H1/H2, and small marks that already rode the accent (list bullets, done
  ticks, dock accents). Everything else stays neutral — per the user's spec
  "highlights, tags, cursor, H1 and H2, that's it".
- `Palette` grew three slots to make that possible without touching the
  sixteen classics: `caret` (classics: fg), `h3` (H3–H6 heading colour;
  classics: accent, minimal: fg — so only H1/H2 carry colour), and `title`
  (pane-header colour; classics: accent, minimal: the dim grey).
  `span_attrs` now takes the heading level.
- The picker's Colour section lists the classics first, then a "Minimal"
  sub-header with the ten; `Theme::is_minimal()` does the grouping and
  `Theme::ALL` holds every variant so config keys round-trip.

## 2026-09-02 — The colour buffet
- A second, experimental colour picker ("Colour buffet", its own foldable
  Options section) that mixes any of the eleven minimal highlights with any
  of six dark plates from the user's black cards: Night #0E0E10, Rich Black
  #171717 (the near-identical #171718 card folded into it), Jet Black
  #1A1A1A, Graphite #1F2124 (unnamed on its card), Ink #212529, Onyx
  #353839. Round chips, click to apply live; the ring marks the selection.
- The pairing paints via `retro::minimal_with(accent, bg)`: the neutrals
  (panel, hairlines, mute, dim) are now blends of the base toward the
  foreground, so any dark works as a plate — `minimal()` is the same
  function pinned to charcoal #121214, which moved the fixed minimal
  neutrals by ~1/255 (imperceptible).
- Buffet state is three config entries (`buffet_on`, `buffet_highlight` =
  a minimal theme key, `buffet_dark`); clicking any chip switches the
  buffet on, choosing a theme in the Colour section switches it off, and
  the swatch list drops its selected ring while the buffet is serving. The
  launcher icon follows the buffet when it follows the colour theme.
- Harness: `buffet:<highlight>,<dark>` script step.

## 2026-09-02 — The buffet's light side
- The Colour buffet now has clear **Dark mode** and **Light mode**
  subsections under the shared Highlight row. Dark mode is the six plates
  as before. Light mode picks two things, per the user: a **Paper** plate —
  Pearl White #FBFCF8, Cream #FAFAF2, Porcelain #F6F6F6, Moon #EDEDE9,
  Quartz #E1E6EA, Bone #D4D4CE, joined the same evening by Ivory White
  #FFFCEF, Coastal Ivory #F6F1E8, Milk #FFFFF5 (unnamed on its card) and
  Off White #FAF9F6, the row ordered bright to grey; later five
  screen-comfort whites went in ahead of them at the user's ask, in their
  given order — Light Gray #F5F5F5, Antiflash White #F2F3F4, Nearly White
  #FAFAFA, Subtle Off-White #F8F7F7, Warm White #FFFDF7 — for fifteen papers — and a writing **Ink** — Midnight #060B11,
  Ink #212529, Navy #023246, Night #495057, Steel Slate #57666D — all from
  the user's light-mode cards (the AWSMCOLOR strip supplied Porcelain and
  Navy; its #287094 was skipped as too close to the Steel Blue highlight).
- Clicking a plate serves its side (`buffet_mode` config, plus
  `buffet_light` / `buffet_ink`); the selection rings only show on the
  active side, and the status line reads "Serving X on Y" or
  "Serving X on Y, writing in Z".
- `minimal_with` now takes the ink too and blends every neutral between
  plate and ink, so one function paints both sides; selected-row text uses
  a brighter-than-ink white only when the plate is dark (luma < 0.5).
  `retro::buffet_dark` / `retro::buffet_light` are the two entry points.
- Harness: `buffet:h,dark` (dark side) or `buffet:h,paper,ink` (light).

## 2026-09-02 — The highlighter, and the sweep's fixes
- Middle-mouse drag is a **highlighter**: sweep over words like a marker
  pen and on release the selection becomes `==marked==` in the file (the
  Obsidian-compatible mark syntax), drawn as an accent band (32 %, rounded)
  behind the glyphs with the `==` ghosted like other markers. Also on the
  dock/Format menu ("Highlight", `░`) and Ctrl+H, which toggle it on the
  selection like Bold does. Previews strip doubled `==` but keep a lone `=`.
- Bug sweep (user report + /code-review): bullet dots were flush against
  their text in proportional faces — the invisible `- ` footprint is now
  1.5× wide on bullet lines only (task-box prefixes keep line metrics);
  numbered markers (`1. `) no longer collapse into anonymous dots — new
  `Kind::NumMarker` keeps the digits visible in the accent; themed-frame
  image cache keys and the Icon picker's "current" tile now know about the
  buffet (`palette_id()`); SetBuffetHighlight rejects non-minimal keys
  instead of half-applying; a follow-mode icon re-installs on external
  config changes and installs off-thread (no chip-click stutter); buffet
  config writes log failures; the serving line is fully translatable
  (`buffet-serving-dark`/`-light`); `minimal_accent()` is exhaustive so a
  forgotten new-theme arm fails the build, not the runtime.
- The user's "Bold doesn't work" could not be reproduced: key, menu, dock
  and rendering all verified in-harness (debug + installed release, their
  exact config, injected real Ctrl+B). `RUST_LOG=jotjotboom=debug` now
  logs every format apply to diagnose it live; awaiting specifics.

## 2026-09-03 — Roadmap: cross-desktop polish for 1.0
Agreed direction: JotJotBoom keeps its own identity everywhere (no GTK/Qt
rewrites); the 1.0 goal on GNOME/KDE is "perfect citizen", not disguise.
Queued for when the colour work settles, in order:
1. Detect dark/light and accent via the `org.freedesktop.appearance`
   portal when the cosmic-config daemon is absent (also improves the
   follow-the-desktop theme on COSMIC).
2. Offer the xdg-desktop-portal file chooser when it responds; the in-app
   picker stays as the fallback.
3. Flatpak packaging (appears properly in GNOME Software / KDE Discover;
   AUR alongside).
4. Paper-cut sweep under Mutter and KWin: CSD shadow/corners, cursor
   theme, menu popup behaviour — needs a VM or nested session to test,
   and a nested-compositor variant of the xshot harness.

## 2026-09-03 — Roadmap: macOS and Windows (post-1.0)
The stack travels (iced/winit, wgpu on Metal/DX12, cosmic-text shapes its
own fonts, files-on-disk + dirs + keyring are portable); libcosmic's
winit path off-Wayland is the risk, being lightly travelled upstream.
Sequencing agreed: Linux 1.0 first, then a CI spike — GitHub Actions
macOS/Windows runners proving it builds — to turn unknowns into a list;
if healthy, Mac/Windows is a 1.x milestone. Known work already:
- cfg-gate the Linux-isms (icon.rs writes hicolor + gtk-update-icon-cache;
  bundles carry the icon instead).
- Cmd vs Ctrl: keybinds resolve per-platform; menu stays in-window (iced
  has no Mac global menu).
- Packaging via cargo-packager (.app/.dmg, Windows installer). Ongoing
  tolls: Apple notarization ($99/yr) or scary warnings; Windows unsigned
  installers trip SmartScreen (cert optional, survivable without).
- CI proves builds; feel needs real hardware or beta testers per OS.
Sync (steps 7–8) gains value at the same moment — multi-OS users are the
ones who want their notes everywhere.

## 2026-09-03 — Formatting became a real toggle (the writer's day)
- Dogfooded the editor as a writer (harness `sel:` step plays the mouse:
  places carets and selections by byte position) and found Ctrl+B was a
  one-way street. Now Bold/Italic/Highlight/Code/[[Link]] toggle like
  every editor: a selection wraps and stays selected (press again to
  undo); selecting just the words of a wrapped run — the markers hide, so
  that is all one *can* select — eats the surrounding markers; with no
  selection the word under the caret wraps/unwraps, the caret staying on
  its character; on a closing marker the caret steps out, so Ctrl+B …
  type … Ctrl+B means "start bold, stop bold"; in open air the old
  plant-the-pair-caret-inside remains. Star formats count `*` runs so
  bold and italic nest (`***word***`) instead of eating each other.
- Root cause of the "selection dies after every format" half of the bug:
  `focus_editor()` used an id-targeted focus operation, and with the
  rendered widget's id drifting from `blocks`' stored id the operation
  *unfocused* the live editor, queueing a ClearSelection right after
  every dock/menu/keyboard format. Focus now travels through the shared
  `RichContent` (`request_focus`/`take_focus_request`) — no ids, no
  side effects on other widgets.
- Bullet gutter widened to 2.2× the literal `- ` (was 1.5×): the drawn
  dot now gets a conventional gap before its text in proportional faces.
- Harness honesty: step args keep trailing spaces (`type:word ` types the
  space), so scripted flows match real typing.

## 2026-09-03 — Tables, with Keynote formulas
- Tables are GitHub pipe tables in the file — files stay the truth and
  render on GitHub/Obsidian — and a live grid in the editor (a new block
  type beside images/rules/cards; `src/table.rs` is the pure core, fully
  unit-tested). Insert via Format → Table / dock `⊞`; typing a pipe table
  by hand converts to a grid the moment the `| --- |` separator row is
  finished (needs_resplit learned the separator line).
- Cells: click to edit in place (Enter commits and moves down — growing
  the table on the bottom row, Keynote style; Tab moves right; Escape
  drops the draft). While editing, a slim toolbar offers +/− row and
  column. Backspace at the start of the next block deletes the table like
  other cards.
- Formulas, the user's headline ask: a cell starting with `=` computes —
  `+ - * /`, parens, cell refs (`B2`), ranges in functions, `SUM AVG MIN
  MAX COUNT` (AVERAGE/MEAN accepted). The grid shows the value (accent2;
  `#ERR` for bad input and circular refs — cycle detection built in),
  editing shows the formula. The file keeps the formula text, so nothing
  is lost outside JotJotBoom.
- Column and row edges are drag handles (hairlines with a wide grab zone);
  sizes are presentation, stored in a `<!-- jjb:table cols=… rows=… -->`
  comment after the table that other renderers hide. Auto columns share
  the width; a dragged column becomes fixed. Note-list previews render
  table rows as `a · b · c` and skip separator/size lines.
- Harness: `cell:row,col,text` (grows the table as needed) and
  `editcell:row,col` steps.

## 2026-09-03 — Formula pointing (the Excel gesture)
- While a cell's draft is a formula that wants a reference next (it ends
  in `=`, an operator, `(`, a separator or `:`), the other cells become
  targets: a click writes their ref (`B2`) into the draft, a sweep writes
  a range (`B2:B4`), and the swept cells light up in the selection tint
  with a crosshair cursor. Release puts the caret back at the end of the
  formula input. Outside those moments a click still commits-and-moves.
- `ref_expected()` decides the mode from the draft's last character — the
  same rule Excel uses; the pick state (anchor, current, the draft as the
  pick began) lives in `TablePick` so sweeping rewrites one ref cleanly.
- Harness: `fpick:r,c[,r2,c2]`, `fpickover:r,c`, `pickdone`, `draft:text`
  steps ("pick" was already taken by the image picker).
- Post-ship fix from the user's real hands: the pointing gate read the
  draft each render, and the ref just written ends in a digit — so one
  frame after the anchor press every cell lost its sweep handler and
  drag-to-range never worked live. The gate now stays open while a pick
  is in flight. Lesson recorded: the script harness can inject messages
  in one step and skip the re-render between them — sweep tests must use
  separate steps (`fpickover`) so state-dependent view wiring is what is
  actually exercised.

## 2026-09-03 — Arrows stay in the table
↑/↓ while a cell is open walk the table's cells (committing each move)
instead of falling through to the notes-list navigation — the single-line
cell input ignores vertical arrows, so they were reaching the ↑/↓
note-hopper and switching documents mid-table. Leaving the table (Escape,
or Enter walking off the edge) hands keyboard focus back to the editor so
arrows always mean "move in what I'm writing", never "change document".

## 2026-09-03 — Sideways in the table
←/→ roam the grid while the open cell is untouched; the first keystroke
into the draft hands them back to the text caret (the Sheets rule — every
arrival in a new cell resets to roaming, so cruising the whole table with
arrows just works). Shift+Tab now tabs left, and sideways moves at an
edge stay put — arrows never dump focus out of the table. Verified with
real injected keystrokes end to end (right → type → left-left in text →
Enter commits below), after the first test tripped over the design
itself by typing before arrowing.

## 2026-09-03 — Money cells
Formats without a format picker: money is text, and it spreads. A cell
like `$42.5` (also € £ ¥, commas fine) still counts as a number, displays
normalised (`$42.50`, thousands grouped, two decimals), and any formula
that touches money shows money — so typing one `$` makes the totals come
out right, Keynote style. A symbol straight after the `=` (`=$SUM(…)`)
forces the format when inference isn't wanted; the table toolbar's `$ ¤`
button cycles the open cell plain → $ → € → £ → ¥ → plain by rewriting
its text. Everything is visible in the file — no per-cell metadata.

## 2026-09-03 — Table UI quieted, and the fill handle
- The toolbar hints moved into hover tooltips (labels stay: + row,
  + column, − row, − column, $ ¤, and an `fx` tag holding the formula
  hint). Clicking back into the note's text commits the open cell and
  puts the toolbar away; switching notes commits too. Options gained a
  Tables section with a "Show the table toolbar" toggle
  (`table_toolbar_off`) for those who want the grid bare.
- The Numbers fill handle: the open cell wears a little accent dot on its
  bottom-right corner; drag it down or across and on release the source
  cell replicates over the swept run — formulas translated relative to
  each target (`=SUM(C2*C3)` one row down becomes `=SUM(C3*C4)`;
  `table::translate_formula`, unit-tested, `#REF` past the edge, values
  copy verbatim, money symbols survive). The sweep is clamped to one
  axis (whichever moved furthest) and lights up in the selection tint
  like formula pointing. Harness: `fill:r,c,r2,c2`.

## 2026-09-03 — Select cells, press Delete
- Press-drag across cells rubber-bands a selection (selection tint, like
  formula pointing); Delete or Backspace empties every cell in it, Escape
  or any click drops it. A press that never leaves its cell is still the
  normal click-to-edit. Harness: `tsel:r,c,r2,c2`, verified with a real
  injected Delete.
- Found underneath: `request_focus` never unfocused sibling editors, so
  several text blocks could hold widget focus at once and a keystroke
  could be performed by every one of them (two editors both ate a
  Delete). Contents now carry `request_unfocus` too: `focus_editor()`
  gives exactly one editor the keys, and any table interaction (cell
  open, selection) blurs them all.

## 2026-09-03 — The window remembers its size
Resizes land as `window::Event::Resized` → remembered (maximised sizes
skipped — that state belongs to the compositor) → written to config by a
1 s tick that only exists while the remembered size is unwritten, so idle
runs cost nothing (the first attempt piggybacked on AutosaveTick, which
only ticks while a note is dirty — resizing dirties nothing, so it never
fired). `main()` reads `window_width`/`window_height` before the run and
passes them as the initial `Settings::size`, floored at the 480×320
minimum. Verified round trip: a run persists its size; a planted 812×624
config opens an 812×624 window.

## 2026-09-03 — Line weights, pizza-sized
Options → Size grew two controls on a Small / Medium / Large / Family
scale (`retro::LineSize`; Medium is today's look): the `---` divider
(1/2/4/8 px) and the border around link/file cards (1/2/4/6 px), plus a
"Draw a border around cards" toggle whose off state hides the weight row
and renders cards frameless. Config: `rule_size`, `card_line`,
`card_border_off`. The image mid-drag lift keeps its fixed 2 px.

## 2026-09-03 — Eight more highlights
From the user's colour cards: Cyan #00A6CB, Hollyhock #AA89BD, Canyon
Clay (Pantone 18-1431, sampled #CF8578), Thistle #D8BFD8, Fuchsia
#FF0080, Bluestone (Pantone 18-4217, #587284), Slate Blue #466E88
(`slateblue2` — the CSS-named `steelblue` key was already taken by the
day-one minimal), and Jade #9CD5C2. The ninth card, Shamrock Green
15-6432, was already in the buffet and was skipped. Nineteen highlights
now; they double as minimal themes in the Colour picker as ever.

## 2026-09-03 — Sidebar spacers join the pizza scale
The tag-list spacer lines take the same Small/Medium/Large/Family weights
(config `tag_line_size`, same 1/2/4/8 px ladder as the divider), with
their own row in Options → Size between the divider and the cards.

## 2026-09-03 — One-shot installer
`./install.sh` in the repo root: detects the package manager (dnf,
pacman, apt, zypper) and fetches the C toolchain + libxkbcommon if
missing, installs rustup per-user when there is no cargo, builds
release, and performs the per-user install (`~/.local`: binary, icon,
desktop entry with absolute Exec, metainfo) without needing `just`.
Re-running after `git pull` updates. README leads with it.

## 2026-09-05 — Sync v1: PocketBase, opaque envelopes, conflict copies
- Backend as locked in the handover: self-hosted PocketBase. `server/`
  carries the migration (a `notes` collection: owner, note id, revision,
  device, modified, `blob`) and a hook that owns the revision counter —
  an update must quote the revision it was based on or is refused (409).
- The blob is an `Envelope`: the whole file text plus `trashed`/`deleted`
  flags, JSON today, ciphertext later. Trash state and tombstones live
  inside it so the server learns nothing new. `.folders` and `assets/`
  do not sync yet (next step: a `files` collection for assets).
- Local sync state (`sync_state`: per note record id, revision, synced
  hash, trashed; `sync_meta`: cursor, account) lives in index.db under
  the oplog exemption. Losing it is safe: identical text is re-adopted
  silently, differing text becomes a conflict copy. Pending pushes are
  found by comparing the indexed hash with the synced hash — so edits
  made outside the app sync too, not just saves through the editor.
- Cycle = refresh token (a dead token otherwise reads as an empty
  account), pull since cursor, push; runs in `spawn_blocking`, the store
  applies results on the app thread. Triggers: 3 s after a save, every
  60 s, on sign-in, Options → Sync → Sync now.
- Conflicts (both sides changed since the last agreed revision): the
  server's copy keeps the note's id and file; the local text becomes a
  new note "Title (conflict, <hostname>)" and is pushed as its own note.
  Edit vs delete: the edit wins and the note comes back. Same text on
  both sides: adopted quietly.
- The user's call (the plane scenario, 2026-09-05): the open note
  auto-updates to the server version when a conflict or plain change
  lands; the editor never switches to the copy, so the user keeps
  writing on the synced line and does not breed further conflicts.
- Token in the keyring (`sync-token`); server address and email in
  cosmic-config. No keyring = sign-in lasts for the session only.

## 2026-09-18 — Bug sweep: the writer's paper cuts
Dogfooded through the harness (scripted writer flows, file inspection,
captures) plus a read of the user's own notes. Fixed, in order found:
- Enter on a list line now carries the list on — `- `, `* `, `1.` (counts
  up), `- [ ] ` (a ticked box comes back open), indent kept — and Enter
  on an empty item ends the list by clearing its marker, the convention
  everywhere. `note::list_continuation` is the pure part;
  `blocks::continue_list` drives the buffer; the app only intercepts
  `Edit::Enter`, so pastes and the format actions are untouched.
- `[]` → `- [ ] ` used to leave two spaces once the writer typed the
  habitual one; the keystroke after the expansion swallows a single space
  at exactly that spot (`task_space`), anything else clears the hint.
- Opening a note parked the caret at byte 0, before the hidden `# `, so
  the first keystroke turned "Alpha" into "x# Alpha". The caret now
  starts at the end of the title line; the view stays at the top.
- `Blocks::remove_block` refused tables, so Backspace at the start of the
  block after a table (which the app already routed there) did nothing.
  Removing any card also threw the caret to the end of the merged text;
  it now sits at the seam.
- Ctrl+click on `[[Title]]` with no such note was a dead click. It now
  creates the note (`# Title` and a fresh paragraph) and opens it, like
  every wiki; `create_note()` is shared with Ctrl+N. The trash view is
  excluded.
- The "linked from" pane was 58 px tall since the flat-pane switch — its
  own title bar and padding used that up, so the chips drew under the
  clip. 88 px.
- **Caret scroll-into-view**, the plan's known gap: typing or arrowing
  past the top or bottom edge left the caret out of sight. Mechanism:
  the app flags the focused content after an edit or caret motion
  (`request_reveal`); the widget, on its next event (after layout, so the
  buffer is shaped), reports the caret line's rectangle via `on_caret`;
  the app runs a widget `Operation` against the editor scrollable (which
  now has an id) that reads the viewport and translation and scrolls
  just enough, with a two-line margin. Opening a note reveals the title,
  so the view starts at the top. Clicks and drags don't trigger it.
- List previews were cut at 90 characters mid-word with no ellipsis;
  `note::clip_words` ends on a word.
- Harness: `key:name[:times]` step for backspace / delete / enter / tab /
  arrows / home / end, needed to drive the deletion edge cases.
Seen and left alone: the user's own note asks about bullet alignment
(dated before the 2.2× gutter change); text wrapped beside a floated
image breaks a heading mid-word when the column is under ~100 px.

## 2026-09-20 — Sync v2: pictures, attached files and `.folders`
- A second collection, `files` (`server/pocketbase/pb_migrations/
  1758000000_files.js`), under the same bargain as `notes`: the server
  sees a `key` (blake3 of the path — never the file name), revision,
  timestamp, device. Path, content hash and size sit in an opaque `meta`;
  the bytes are a protected upload always named "blob" (downloads need a
  short-lived file token). Encryption later wraps `meta` and the bytes
  and changes nothing else. Limit 256 MB per file; bigger ones are
  reported and stay local.
- What travels: `.folders` and everything visible under `assets/`.
  Dot-named things stay home — `assets/.links/` (derived cache) and
  `assets/.incoming/` (downloads land there, are checked against the
  hash in `meta`, then renamed into place). Symlinks are skipped.
- The revision hook now guards both collections and reads
  `base_revision` as a number or a string (multipart sends strings — the
  old `typeof === "number"` test would have waved every upload through).
- The bytes never cross the app thread: `sync::files::run` scans, hashes,
  uploads and downloads from the blocking thread, after the notes half
  of the same cycle. The store only gets bookkeeping. A cycle moves
  files for about 20 s, smallest first, then asks for another, so a
  first sync of a big folder never holds the notes up.
- Local state: `sync_files` in index.db (record id, revision, agreed
  hash, plus size/mtime/hash of the local file so unchanged files are
  not re-read). Created outside the schema version so adding it cost no
  rebuild. Losing it is safe: identical bytes are re-agreed silently.
- Never clobber, for files: if a *different* file of the same name
  comes down and ours was not simply the old agreed version (two
  cameras' IMG_0001, or both sides edited), ours steps aside as
  `name-2.ext` — the import naming — and every local note showing it is
  rewritten to the new name before anything from the server is applied
  (`note::repoint_asset`, `Applied::Repointed`; the open note is flushed
  first and reloaded). So each note keeps showing the picture it was
  written against, on every device. An untouched local copy is simply
  replaced by the newer one.
- Deleting an asset does not travel (v1): nothing in the app deletes
  assets yet, and a folder that briefly lost its `assets/` must not
  empty everyone else's. A file deleted by hand stays on the server and
  on other devices. Revisit together with orphan clean-up.
- `.folders` is merged three ways against the last agreed text
  (`sync_meta.folders_base`): a folder survives if both sides have it or
  either side added it; one side removing it removes it. Sorted output,
  so devices converge on identical bytes. The merge happens in the
  store (it owns the in-memory list); the merged list goes up on the
  next cycle.
- A server without the new migration gives a plain error naming the fix;
  the notes half still syncs. Older clients ignore the collection.
- UI: Options → Sync gains a files line ("n came down, n sent up") and a
  waiting count. Pictures that failed to load are retried when a cycle
  brings files down.
- Tested: `files_travel_between_two_devices` (needs `JJB_PB_URL`), and
  two real app instances against a local PocketBase — note, picture
  (byte-identical) and folder arrived.

## 2026-09-26 — Dock icons and text zoom

- The dock's format buttons draw icons instead of letters and glyphs
  (`src/dockicon.rs`, generated by `tools/dock_icons.py` from the
  Iconify collections). They are drawn in the same icon set the tags
  wear (Options → Icon), so the sidebar and the dock always match; a
  set that lacks a drawing borrows Boxicons, the tag rule. Heading 1
  and 2 are the set's heading glyph with a stroked numeral where the
  set has no native heading-1/2 (Myna UI and Pixelarticons do). `+`
  and the picture button use the accent colour as before; format
  icons use the text colour. `DockSize::icon()` replaces `glyph()`;
  the paddings were tightened so a medium dock is about as wide as
  the text version was.
- Ctrl + plus / minus grows and shrinks the editor text by 1 px per
  press, Ctrl + 0 puts it back to the default (15 px); the same
  `SizeStep` the Options → Size row uses, so the value persists in
  `editor_font_size`. Every spelling of plus is bound (Ctrl + `+`,
  Ctrl + Shift + `+`, Ctrl + `=`) because `+` is Shift + `=` on most
  keyboards. Listed in the View menu and the shortcuts overlay.
  `shortcut_for` now breaks length ties alphabetically so an action
  with several bindings always shows the same one.


## 2026-09-26 — Windows first: a Rust core and one shell per platform

- Windows is the primary platform from here. The two real audiences are
  Windows and GNOME, not COSMIC; one primary during the feature rush.
  The app becomes a Rust core plus a Tauri 2 shell: WebView2 with Fluent
  UI React v9 chrome on Windows, the same Tauri app on WebKitGTK for
  GNOME (second), Mac third and later. The editor there is CodeMirror 6
  driven by the existing markdown scanner compiled to WASM, so both
  editors paint the same spans from the same code. The libcosmic app is
  frozen as the reference build on the Linux box until the Tauri build
  is the better GNOME app; then it retires (recorded here when it does).
- The repo is a Cargo workspace: `crates/jjb-core` (note, store, sync,
  markdown scanner, images, links, tables, platform trait), `crates/
  jjb-md-wasm` (the scanner for the browser), `apps/jjb-cosmic` (the app
  as it was; package name stays `jotjotboom` because the i18n domain and
  the launcher files derive from it). Rule: no libcosmic, iced or zbus in
  the core. The one OS dependency it keeps is `secret-service`, gated to
  Linux, because the Tauri app on GNOME needs that keyring too.
  `just check-windows` proves the core builds for
  `x86_64-pc-windows-msvc` through cargo-xwin; `cargo test` runs a parity
  test that feeds one fixture through the WASM build (via node) and the
  native scanner and compares the spans.
- What stayed in the shell: the cosmic-config entry (`config.rs`; the
  Tauri app keeps its own settings file with the same keys), the iced
  colour and font table for spans (`markdown::style_for`), the drag-and-
  drop payload type, the palette → image inks conversion. The core's
  `images` takes plain RGB `Inks` and its `markdown` returns plain
  spans with serde derives, so the scanner is toolkit-free.
- The platform trait (`jjb_core::platform`) covers the default notes
  dir, the data dir for `index.db`, the keyring, and opening a URL.
  Single instance is deliberately not on it: it is the shell's job
  (libcosmic's D-Bus activation here, Tauri's single-instance plugin
  there) because the shell receives the forwarded arguments and raises
  the window. The Windows implementation is a stub for the keyring
  (Phase C wires the Credential Manager through the `keyring` crate);
  folders and open-URL already work there through `dirs` and `open`.
- The list-marker helpers (`task_box`, `list_marker`, `numbered_marker`)
  moved from `note` into `markdown`, re-exported from `note`, so the
  scanner has no dependency on the note module (which needs uuid and
  blake3, neither of which the WASM build wants).
- `.gitattributes` forces LF for everything text: notes are byte-exact
  files, the sync hash is over the bytes, and Windows Git would
  otherwise rewrite them.
- Safe point before all this: tag `v0.2-pre-windows`. The handover for
  the Windows session is `WINDOWS-HANDOVER.md` in the repo root; it is
  the only thing that session can read.

## 2026-09-26 — The app is called Attic

- Renamed from JotJotBoom to **Attic** ("store stuff in the attic"), before
  the Windows shell exists, so there is one shell to rename and the
  handover starts with the final name. Binary and package `attic`, app
  id `io.github.reddemo0110.Attic` (the Flathub form, owner then app;
  the old id used the app name twice), D-Bus names follow it, keyring
  attribute `application=attic`, sync `User-Agent: Attic/<version>`,
  default notes folder `~/Documents/Attic`.
- Deliberately unchanged: the `jjb` prefixes. `jjb-core`, `jjb-md-wasm`,
  `apps/jjb-cosmic`, the `<!-- jjb:table -->` comment inside note
  files, the `jjb-file:` sync key prefix and the `JJB_*` environment
  variables are invisible to users and two of them are part of the
  on-disk and sync contract; changing them would rewrite every note
  with a table and re-upload every asset.
- A machine that still has `~/Documents/JotJotBoom` and no
  `~/Documents/Attic` keeps using the old folder
  (`platform::LEGACY_NOTES_FOLDER`); nothing is moved behind the user's
  back. Settings under the old app id are not read; on this box they
  were copied by hand.
- Historical documents keep the old name: the original project
  handover, `RICH-EDITOR-PLAN.md`, and earlier entries here.
- Still to do: rename the GitHub repository to `attic` (the URLs in
  Cargo.toml and the metainfo already point there; GitHub redirects
  the old name), and rethink the launcher icon, whose two dots and hash
  spelled "jot jot boom".

## 2026-11-04 — First-class on both desktops: detection, live GNOME settings, COSMIC launcher
- Detection: `XDG_CURRENT_DESKTOP` first, then `XDG_SESSION_DESKTOP` and
  `DESKTOP_SESSION`; with none set (D-Bus activation, a launcher plugin,
  `env -i`) the session bus is asked who owns
  `com.system76.CosmicSettingsDaemon` (checked first: COSMIC also owns
  some `org.gnome.*` names for compatibility) or `org.gnome.Shell`. The
  result is logged at start-up (`session desktop=…`).
- GNOME settings are now followed live, not read once: `desktop::watch()`
  listens to `org.freedesktop.portal.Settings.SettingChanged` for the
  `org.gnome.desktop.interface` / `…wm.preferences` namespaces and re-reads
  everything with one `ReadAll` (GNOME fires the signal twice per change;
  `apply` ignores a no-op). The listener runs on its own thread with a
  current-thread tokio runtime so neither the iced executor nor the
  blocking portal reader is a concern. Verified on this GNOME session: a
  font change in Settings re-renders the menu bar within milliseconds.
  Dark/light, contrast and accent were already live through libcosmic's
  own `xdg-portal` subscription, so roadmap item 1 from 2026-09-03 is done.
- COSMIC gets the GNOME search provider's twin: a pop-launcher plugin
  (`attic --launcher-plugin`, manifest `resources/launcher-plugin.ron`)
  behind the prefix `note ` / `attic `, isolated so it does not spam the
  app list on every keystroke; just the prefix lists recent notes. Chosen
  over a prefix-less plugin because the COSMIC launcher is also the app
  switcher and its own plugins all take a prefix. Logs go to stderr there
  because stdout is the protocol. Installed by both installers as
  `pop-launcher/plugins/attic/{plugin.ron, attic-launcher}`; harmless on
  GNOME.
- Not done, still queued: Flatpak; the Mutter/KWin paper-cut sweep.

- Paper cut found on GNOME the same day (2026-11-05): a black square in
  the window's top-left corner, outside the rounded header. Cause: the
  app's `style()` painted the palette background over the whole surface,
  while libcosmic draws the window as a rounded container and expects the
  surface behind it to be transparent (its default style is, except when
  maximised). On COSMIC the compositor clips the corners through its own
  protocol, so it never showed there. Fix: transparent background whenever
  the corners are rounded; the palette colour stays for maximised / tiled
  windows, where the corners are square anyway.
- Second paper cut the same evening: a hard grey 1 px line around the
  window where GNOME's windows have a faint outline. libcosmic draws that
  border in `bg_divider` (text colour at 20 %, composited opaque) and the
  border is hard-wired in its window layout, so the app softens the
  divider in the active system theme instead (white 10 % on dark, black
  12 % on light, Adwaita's values) through `cosmic::command::set_theme`,
  re-applied whenever a redraw finds libcosmic has installed a fresh
  theme (start-up, dark/light, accent). The other half of GNOME's look,
  the soft drop shadow, is drawn by the client under Mutter (no
  server-side decorations are offered: `wayland-info` lists no
  xdg-decoration) and libcosmic / iced_winit have no shadow margin, so
  that one is out of reach without a different toolkit; recorded as the
  limit of the libcosmic shell on GNOME.

## 2026-11-07 — The menu bar follows GNOME's accent colour
- Reported on GNOME with the accent set to slate: the menu labels (File,
  Edit, …) and header icons stayed COSMIC blue. libcosmic's `text_button`
  component draws its text in the accent, and on GNOME the accent never
  left COSMIC's default: libcosmic's portal subscription
  (`theme/portal.rs`) reads the colour scheme and contrast and follows
  their changes, but never asks for `accent-color`, even though
  `Desktop::Accent` and its handler exist. Its `portal_accent` field is
  `pub(super)`, so the app cannot feed it either.
- Fix in the app, not a libcosmic fork: `desktop::read_portal` reads
  `org.freedesktop.appearance` alongside the GNOME namespaces (and keeps
  the accent alone where the GNOME namespaces are absent, so KDE's accent
  comes along too); `watch()` follows that namespace's `SettingChanged`;
  the outline fix-up grew into "fit the system theme to the desktop":
  `softened_theme()` rebuilds the theme with `Theme::with_accent` first,
  then softens the divider through serde as before, and `outline_stale()`
  reports either lack. Same bounded retry counter, keyed on the theme's
  own (dark, accent).
- `with_accent` keeps the theme's accent *lightness* and takes hue and
  chroma from the desktop, the way libcosmic applies the portal accent on
  its own. Kept that behaviour rather than the raw GNOME colour: COSMIC's
  dark theme wants a light accent for text on its dark chrome, and the
  stale check compares against the same adjusted colour so it settles.
  Slate on dark comes out as (171, 192, 213), near libadwaita's own
  standalone slate; the portal's raw slate is (111, 131, 150).
- No accent set (portal reports out-of-range components) means leave
  libcosmic's default alone. The `gsettings` fallback does not map
  GNOME's accent names to colours; the portal is on every GNOME box.

## 2026-11-07 — A plain image style
- User: a style that "doesn't do any styling, just places the full image
  which is the full width of the editor's text width". New `FrameStyle::Plain`
  (`frame=plain`, first in the menu): no pixel treatment, no card at all,
  the `widget::image` alone, which already fills the text column when the
  reference has no `w=`. Choosing it in the ⋯ menu also clears the width
  and float so the picture spans the column; the grip and width presets
  still work afterwards for someone who wants it narrower.
- `box` stays the default so every existing bare `![alt](path)` renders as
  it did; plain is written out as `{frame=plain}`.
- Plain keeps up to 1600 px on the long edge (`display_max`) instead of
  the 720 px the treatments run on: with nothing to compute per pixel and
  a column wider than 720 logical px on a 2× display, the old cap would
  have shown a soft upscale.
- Same evening: the user asked for plain to be what a dropped picture
  gets. `import_image_at` (the one path for drag-and-drop, the dock's
  picture button and the in-app picker) now inserts `{frame=plain}`
  instead of the bare line. The format's default is still `box`, so this
  is written out explicitly and older notes are untouched.
- Also that evening: the `◢` resize grip only shows while the pointer is
  over the picture (`image_hover`, from the card's `mouse_area` enter /
  exit) or a resize is in flight (`live_width` for that block, since the
  pointer wanders off the card mid-drag). The `⋯` chip stays put: it is
  the way into the menu and should be discoverable. The hit area is gone
  with the glyph, which is fine because nobody can reach the corner
  without first hovering the card.

## 2026-11-07 — The menu bar's dropdowns, and a way to see them
- User: the menus "really don't look ok". Seen once captured: every
  dropdown 150 px wide with the shortcuts ellipsised ("Ctrl + …ift + P"),
  and each divider a 30 px grey slab. Cause: `menu::bar` was built with
  libcosmic's defaults, `ItemWidth::Uniform(150)` and
  `ItemHeight::Uniform(30)`; the uniform height stretches the thin divider
  widget to a full row. Same on COSMIC, so not a GNOME paper cut.
- Now `ItemHeight::Dynamic(40)`, 4 px spacing (what cosmic-edit and the
  libcosmic menu example use) and `ItemWidth::Static(260)` with per-menu
  widths: Edit 200, View 330 (its longest row is "Keyboard shortcuts
  Ctrl + Shift + H"). Static over Uniform so Edit does not have to be as
  wide as View.
- Seeing it took a harness: the script hook cannot open a libcosmic menu
  (its state is internal), and injecting pointer input on the session's
  Xwayland is both unsafe (it lands on the real desktop) and, under
  rootless Xwayland, the capture can pick up other clients' windows (it
  did: a password manager; the picture was deleted on sight). `xshot.py
  --nested WxH` now runs the app in a private headless mutter on a private
  session bus; clicks go through `org.gnome.Mutter.RemoteDesktop` (XTEST
  from Xwayland is ignored there), absolute motion anchored to a
  `ScreenCast` stream of the virtual monitor that nobody consumes. Gotchas
  recorded in CLAUDE.md: the app's X11 window gets focus and pointer only
  from a click, so the first click is spent; the window maps late, so
  `--wait 9`.

## 2026-11-07 — Dropbox as the sync: a chosen notes folder, watched
- The user is moving to Dropbox on every Linux machine and asked whether
  pointing the app at a synced folder would do as cloud sync. It does,
  and it is what "files on disk are the source of truth" was for: notes,
  `assets/`, `.trash/` and `.folders` all live in the one folder, and the
  index is per machine outside it. Two things were missing.
- **Options → Notes folder.** The folder was a config value with no UI.
  Now a section with the current path, a hint, "Choose folder…" (the
  in-app picker in folder mode: Home / Documents / Dropbox shortcuts,
  folders only, "Use this folder" beside the path) and "Use the default"
  when it is not. `switch_notes_dir` flushes and drops the open note,
  stores "" for the default so the path is not frozen per machine, and
  reopens the store on the same `index.db`: the reopen's re-scan indexes
  the new folder and the old folder's rows fall away. An empty folder
  gets a welcome note, as a first run does.
- **A watcher.** `watch.rs` (`notify`, recursive, 500 ms settle; our own
  `.name.tmp` files filtered) feeds `NotesChanged`; `external_change`
  reloads `.folders` if it was touched, re-indexes, and bails when
  nothing changed (our own saves: the index already has the hash). For
  the open note it follows the server sync's rules: gone on disk and
  clean here → closed; gone but dirty → the edit wins on the next flush;
  changed on disk and clean → reloaded in place; changed on both sides →
  the typing here stays open, the disk version is filed as "… (conflict,
  another device)" (`Store::keep_disk_copy`) and ours is flushed over the
  file. Dropbox's own "conflicted copy" files simply show up as notes.
- `Store::reindex` dropped a note that was renamed elsewhere: the new
  path was indexed under the same id, then the stale path's id was
  removed. Now a stale path whose id already points elsewhere is left
  alone. Found by the test for the rename case.
- Verified live with the harness: a file dropped into the folder appears
  in the list, and the open note picks up an edit made to its file.
- Not done: the PocketBase sync and a synced folder on the same notes
  are not stopped; they should not be combined. Dropbox's Linux client
  syncs dotfiles, so `.folders` and `.trash/` travel.

## 2026-11-08 — Dropped pictures went missing: the drop was a move
- User: dragging a picture in shows the drop line and then nothing. Could
  not be reproduced with the script hook (it bypasses DnD), nor at first in
  the nested-mutter harness: a drag from a small GTK 4 window (the new
  `tools/dnd-probe.py` + `dnd-source.py`) worked. Two things had to be made
  faithful. (1) smithay-clipboard only creates the seat's data device when
  the seat gains a *keyboard* capability; the sandbox seat had only the
  virtual pointer, so the app never received `wl_data_device.enter`. On a
  real desktop both exist. (2) The source must offer what Nautilus offers,
  copy / move / link / ask, and delete the file after a move.
- With that, `WAYLAND_DEBUG=1` showed the app answering
  `set_actions(copy|move, preferred move)`: libcosmic's `DndDestination`
  defaults to `preferred_action: Move`, mutter picked Move, the source
  reported "delete = True" and removed the original, and the app's copy
  into `assets/` failed ("importing image err=copying …"), silently. On
  the user's machine Nautilus is the source, so each dropped picture was
  deleted from where it came from. The drop line the user saw is what the
  editor shows while a drop is pending; with nothing inserted there was no
  re-layout to replace it.
- Fix: the destination advertises and prefers Copy only; the source keeps
  its file and the copy succeeds (probe: "delete = False", the image line
  is in the note). An import failure now also shows in the editor's status
  line (`import-error`) instead of only in the log.
- Not fixed in libcosmic itself: the default of preferring Move for a
  `dnd_destination_for_data` is a trap for any app that copies.


## 2026-11-08 — The chrome takes the note theme's side
- User, on a white theme with GNOME set to dark: the header, menus and
  window buttons stayed dark. The chrome had followed the desktop's
  dark/light alone. Now it follows the theme being painted: the app works
  out the side from the palette's background luminance (`chrome_dark`),
  `None` for the "follows your desktop" theme, and hands it to
  `desktop::set_chrome_dark` on every update.
- `desktop::softened_theme` grew from "outline + accent" into fitting the
  whole chrome theme: the base is libcosmic's light or dark *system* theme
  (the user's own on COSMIC, the default on GNOME), the accent and the
  soft outline go on top off COSMIC, and the result is installed as a
  `ThemeType::Custom` theme, remembered by pointer. Custom because
  libcosmic's own system-theme handlers only replace System themes, so
  they no longer fight ours; the desktop's dark/light and accent changes
  reach the chrome through our portal watcher instead (`color-scheme`
  joins `accent-color` in `Settings`). A custom theme that is not ours is
  left alone. On COSMIC with the app back on the desktop-following theme,
  the theme is handed back to libcosmic (`system_preference()`).
- Rejected: using the note palette's colours for the chrome. The rule
  stays "stock chrome"; only its side changes, so the menus still look
  like the desktop's.
- Two traps met on the way. `Theme::with_accent` rebuilds the theme from
  the COSMIC config's builder, and with no config (GNOME) the light one
  comes back *dark*; the accent is now applied with the plain
  `ThemeBuilder::light()/dark()` the defaults are made from. And the
  "follows your desktop" note palette read `core.system_theme()`,
  libcosmic's own idea of the system theme (COSMIC's cyan accent), not
  the chrome theme installed; it reads `theme::active()` now, so the note
  surface and the chrome agree.
- Harness: `index.db` is shared by every instance under `XDG_DATA_HOME`,
  and the folder watcher re-indexes on change, so a scratch run and the
  real app re-indexed each other's notes away (an empty list in one
  capture). `xshot.py` and `dnd-probe.py` give each run its own
  `XDG_DATA_HOME`.

## 2026-11-11 — Spelling and grammar, as switches

- The ask: sometimes you do not want constant reminders that you spelled
  something wrong or forgot a comma; sometimes you just want to write. So
  the marks are opt-in, from two toggles in the dock (S and G, after Add
  image, lit with the selected-row colour while on), red under spelling,
  blue under grammar. Nothing else: no popups, no auto-correct. Both off
  by default and remembered (`proof_spelling`, `proof_grammar`).
- Engine: Harper (`harper-core`, Apache-2.0, pure Rust, offline). One
  crate gives both spelling and grammar, it reads markdown itself (so
  `#tags`, `[[links]]`, code spans and URLs pass without masking), and it
  has an Australian dialect (so "colour", "organised" pass). Rejected:
  Hunspell-style dictionaries (`spellbook`) — spelling only, any language,
  but grammar would have been a second project; LanguageTool — a Java
  server or a cloud call, and notes stay on the machine. English only is
  the price; a Hunspell path can be added if non-English notes turn up.
- Where: `jjb_core::proof` (behind `full`), one line at a time
  (`check_line`), with Harper's spans turned into byte ranges, so the
  Windows app gets the same checker. The rule set is Harper's curated one
  minus `UseTitleCase` (a note's headings are sentences); more exceptions
  go in `RULES_OFF` as they annoy.
- Cost: the engine takes ~300 ms to build, so it is built once, off the UI
  thread (`load_proof()` when a toggle turns on, or at start when one was
  left on), and the editor only asks once `proof::ready()` — the settings
  carry `Want::NONE` until then, so no line is ever cached as "clean"
  before it was read. A line costs ~0.3–1.5 ms; the editor caches the
  answer per (line text, wanted kinds) next to its styling cache and
  re-asks only for lines that changed, so typing costs one line.
- Drawing: an issue's byte range is matched to the glyphs it covers on
  each layout run (`Overlays::proof`) and drawn as a 2 px band under the
  baseline — a plain line, in keeping with the retro surface. The word
  under the caret is skipped while it is being typed (the issue's range
  touches the caret index on the caret's line).
- Not yet: suggestions. The editor has no right-click menu today; a
  menu offering Harper's replacements, "add to dictionary" (a file in the
  notes folder, so Dropbox carries it) and "ignore" is the next step.
- Harness: `proof:s|g|sg|off` loads the engine synchronously and sets the
  toggles, for screenshots. The plain `xshot.py` run (session Xwayland)
  found no window today; `--nested 1600x1000 --wait 9` did.

## 2026-11-11 — Paper: the buffet highlight with no colour

- The ask: "sometimes I feel like no colour at all, just white in the
  highlights" — the third paper colour (Nearly White, #F4F4F4-ish) as an
  option in the Highlight row, for dark mode.
- The highlight was a minimal `Theme`; it is now `retro::Highlight`
  (`Theme(Theme)` or `Paper`), stored as the key `paper`. Rejected: a new
  minimal theme with a white accent — it would also have appeared in the
  theme list as a white-on-charcoal theme nobody asked for, and would be
  invisible on a paper plate.
- On a light plate Paper resolves to the chosen ink instead
  (`Highlight::accent(light_mode, ink)`): "no colour" means the writing's
  own tone on either side, and nothing disappears into the page.
- Harness: `buffet:` steps call `persist_buffet()`, so a screenshot run
  must set `XDG_CONFIG_HOME` to a scratch dir or it rewrites the real
  buffet (the `notesdir` rule, same reason). Its arguments are
  comma-separated: `buffet:paper,richblack`.

## 2026-11-11 — Immersive writing

- The ask: a mode where "everything goes away except the writing" — the
  folders, the list, the dock (hidden like the Mac's, with a thin line to
  say it is there), and the desktop itself, the way a slideshow takes
  the whole screen.
- Three things at once (`Message::ToggleImmersive`): the window goes
  fullscreen (`iced::window::set_mode(id, Mode::Fullscreen)`; on Wayland
  that is `xdg_toplevel.set_fullscreen`, and GNOME hides its top bar and
  dock for a fullscreen window), libcosmic's header bar is switched off
  (`core.window.show_headerbar = false` — the menu bar lives in it), and
  the side panes close. `immersive: Option<(nav, list)>` remembers what
  to put back; it is never persisted, so the app always starts normal.
  `UpdateConfig` writes the pane visibility into that memory instead of
  the live state while immersive, or a config write from anywhere
  (the buffet, a toggle) would bring the panes back mid-session.
- The dock: a `stack` over the text with a full-width `mouse_area` at
  the foot — a 160 × 2 px line in the mute colour while hidden,
  `on_enter` → `DockPeek(true)` swaps in the real dock, `on_exit` puts
  it away. Over the text, not under it, so nothing shifts when it shows.
  Rejected: swapping the dock for the line in the column — the text
  area would jump by the dock's height every time.
- Out: Esc (when nothing else wants it: tables and drags take it first)
  or F11 again. The View menu carries it as "Immersive writing". F11 is
  the one named-key binding; the `bind` helper only takes characters.
- Not verified: the GNOME top bar vanishing. The nested harness has no
  panel; the fullscreen request itself is the standard one.

