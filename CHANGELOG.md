# Changelog

What changed for people using Attic, newest first. Every commit adds its
notes under **Unreleased**; a release moves them under a version and date.
Format after [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added
- **Ten light colour themes**, at the top of Options → **Colour Themes**
  (the section was called Colour). Each is white paper and dark ink
  after a designer's colour card: Deep Water, Retro TV Glow, Ocean
  Breeze, Oxidized Iron, Sunray, Marina, Aegean Sky, Beanie, Spritz and
  Poolside. Colour is used sparingly, the way the cards use it: the
  bright one for headings, bullets and the caret, a second for tags and
  links, the card's pale one for the side panels. A bright colour too
  faint to read on white is deepened just enough to read; Ocean Breeze
  and Sunray use their yellows as the text highlighter instead.
- **Capture from the web**: a new cloud icon in the dock opens a panel
  where you paste one or more web addresses and press **Capture**. Each
  address becomes a new note holding the page's text, cleaned up the way
  a browser's reader view does it: headings, paragraphs, lists, quotes,
  links and code, without the menus, adverts and footers. The note
  starts with the page title, a card linking back to the source, and the
  date. An article split over several pages ("Page 1 of 5", a "Next"
  link) is followed and joined into one note, up to 30 pages. Every
  address gets a ticket in the panel that shows how it is getting on:
  waiting, fetching page 2 of 5, done (with a link to open the note), or
  why it failed. Captures run in the background, so you can keep writing,
  and a note started while you are in a folder is filed there. Text
  only for now: pictures are left out.
- **Capture from sites you log in to.** When a page sits behind a login,
  its ticket offers **Log in with Firefox**: a Firefox window opens on
  that site, you log in as you always do, and when you close the window
  the capture runs again and gets the page. Attic remembers the site, so
  later captures from it need no second login until the site logs you
  out. This uses the Firefox already on your computer with a profile of
  Attic's own; your everyday Firefox, its tabs, history and logins are
  not touched, and your password never passes through Attic. A finished
  ticket also has "Not the whole text? Log in and capture again" for
  sites that show a teaser to strangers; the fuller text replaces the
  teaser in the same note. Pages that only fill in their text with
  JavaScript are read through Firefox as well, automatically.
- **Duplicate a note**: right-click a note in the list and choose
  *duplicate*. The copy opens straight away with " copy" after its
  title, pictures and all, and sits in the same folders as the original
  because it carries the same tags. Change its tag to move it somewhere
  else.
- **Twenty new buffet colours, in their own New section** at the foot of
  the colour buffet, under the colours that were already there, so the
  newcomers are easy to spot. Twelve highlights: Barbados Cherry, Barbera,
  Dark Ruby, Ferrari Red, Volcano Flame, Cherry Pop, Dragon Fruit Pink,
  Salmon Orange, Treetop, Adagio Mint, Ocean Spray and Glasier. Six dark
  plates: Adagio Navy, Beetle Wing, Ocean Depths, Japanese Indigo, Adagio
  Slate and Kaffeebraun. One paper, the butter-yellow Gelb, and one ink,
  Kaffeebraun. They mix with everything above them. The twelve highlights
  also join the end of the Minimal theme list.
- **Spelling and grammar marks**, switched on from the dock: two letters
  after Add image, **S** and **G**, each lit while it is on. With S on,
  misspelt words get a red line underneath; with G on, grammar slips get a
  blue one. Both off by default, so you can just write and check later. The
  word you are typing is never marked. Australian English; `#tags`,
  `[[links]]`, code and web addresses are left alone. Works offline.
- **Immersive writing** (View menu, or **F11**): the window takes the whole
  screen, GNOME's top bar and dock included, and everything but the note
  goes away: no folders, no notes list, no menu bar. The dock hides under
  a short hairline at the foot of the page; rest the pointer on it and the
  dock rises, move away and it goes. **Esc** or F11 brings everything back
  the way it was.
- **Paper, a highlight with no colour**, last in the colour buffet's
  Highlight row: on a dark plate the headings, tags and links come out in
  Nearly White, the third paper colour; on a light plate they take your
  chosen ink. For the days you want just white on black.
- **Notes folder** in Options: choose where the notes live, with Home,
  Documents and Dropbox shortcuts in the picker. Point every machine at the
  same synced folder and the notes follow; Attic watches the folder and
  shows another machine's changes while it is open. A note changed on both
  sides keeps what you are typing and files the other version as a
  conflict copy.
- **Plain picture style**: the bare image across the text column, no frame
  or treatment. It is first in a picture's menu, and it is what a dropped
  or picked picture gets now.
- **GNOME as a first-class desktop**: the app follows GNOME's fonts, icon
  theme, title-bar buttons and accent colour, live as you change them in
  Settings, and knows which desktop it is on even when started from a
  launcher. Notes appear in GNOME's Activities search.
- **COSMIC launcher plugin**: type `note ` in the launcher to search notes.

### Changed
- **On GNOME, Attic's title bar and menus now look like GNOME's own apps.**
  The minimize, maximize and close buttons are GNOME's round grey buttons
  instead of flat icons tinted in the accent colour, and they follow
  GNOME's title-bar button setting as before. The pin, trash, new-note and
  sidebar buttons are GNOME's flat buttons: a soft grey when you point at
  them or switch them on, never blue. The File, Edit, Format and View
  menus open as GNOME-style popovers with rounder corners, a little
  padding round the items, GNOME's menu colour and its tighter row
  spacing. On COSMIC nothing changes.
- **Installing no longer leaves gigabytes behind.** `install.sh` now
  removes Rust's build files (the `target/` folder) once Attic is
  installed; they had grown to 32 GB on one laptop. The next install
  builds from scratch, which takes a few minutes. Set
  `ATTIC_KEEP_BUILD=1` to keep them, for example while working on Attic.
- The window chrome (header bar, menus, buttons, options drawer) follows
  the note theme's side: pick a light theme and the chrome goes light too,
  whatever the desktop is set to. The "follows your desktop" theme leaves
  it to the desktop, as before.
- With the "follows your desktop" theme, the note surface takes its
  accent from the same theme as the chrome, so on GNOME it wears your
  GNOME accent rather than COSMIC's cyan.
- The menu bar's dropdowns are wide enough for their shortcuts, with thin
  dividers and proper spacing (they were cramped, with shortcuts cut short
  and dividers drawn as thick bars).
- A picture's resize grip only shows while the pointer is over it.
- The menu bar text follows the desktop's accent colour instead of
  staying COSMIC blue.

### Fixed
- **Selecting across pictures.** A selection used to stop at the first
  picture, link card, divider or table, because the text on either side
  of it was a separate editor. Now a drag carries on through them into
  the text beyond, **Ctrl+A** takes the whole note, and **Shift** with
  the up or down arrow steps over a picture too. Everything selected is
  highlighted, pictures included. Copy or cut it and paste it into
  another note and the pictures come along; typing or Delete replaces
  the lot.
- **Pasting a picture into the middle of a line** now puts it on a line
  of its own, so it shows as a picture and not as its markdown.
- **Dropping a picture from the file manager deleted the original.** The
  drop was treated as a move, so the file manager removed the source before
  Attic had copied it, and nothing appeared in the note. Drops are copies
  now, and a failed import shows a message in the editor's status line.
- A note renamed on another machine (arriving through a synced folder) no
  longer disappears from the list.
- On GNOME: a black square in the window's top-left corner, and a hard grey
  outline where GNOME windows have a faint one.

### For developers
- `tools/xshot.py --nested WxH` runs the app in a private headless mutter
  where `--click` and `--key` are safe; menus and popovers can be captured.
- `tools/dnd-probe.py` reproduces a file-manager drag into the app and
  prints the Wayland protocol exchange.

## 0.1.0 — 2026-09-26

The app renamed from JotJotBoom to Attic; a Rust core, a wasm markdown
scanner and the libcosmic shell split into a workspace. Everything before
this is in `DECISIONS.md`.
