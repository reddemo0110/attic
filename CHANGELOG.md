# Changelog

What changed for people using Attic, newest first. Every commit adds its
notes under **Unreleased**; a release moves them under a version and date.
Format after [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added
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
