// SPDX-License-Identifier: GPL-3.0-only

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use std::path::PathBuf;

#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    /// Where the `.md` files live. Empty string = default (`~/Documents/Attic`).
    pub notes_dir: String,
    /// Stable per-installation id, stamped into the oplog for sync. Generated on first run.
    pub device_id: String,
    /// Retro theme key (see `retro::Theme::key`). Empty = default.
    pub theme: String,
    /// Paint markdown syntax markers visibly (dim) instead of ghosted.
    pub show_markers: bool,
    /// Collapse the views/tags column. (Inverted so the default is shown.)
    pub hide_nav: bool,
    /// Collapse the notes list.
    pub hide_list: bool,
    /// Tags whose sub-tags are folded away in the sidebar (full paths).
    pub collapsed_tags: Vec<String>,
    /// Editor font key (see `retro::EditorFont::key`). Empty = system monospace.
    pub editor_font: String,
    /// Sidebar + notes-list font key. Empty = system monospace.
    pub ui_font: String,
    /// Notes-list font key. Empty = follow `ui_font`.
    pub list_font: String,
    /// Pane-title font key. Empty = VT323.
    pub title_font: String,
    /// Editor text size in px. 0 = default.
    pub editor_font_size: u16,
    /// Sidebar (views/tags) and notes-list text sizes in px. 0 = default.
    pub sidebar_font_size: u16,
    pub list_font_size: u16,
    /// Dock size key: small, medium, large, wow. Empty = medium.
    pub dock_size: String,
    /// What goes inside the brackets of a finished task. Empty = "x".
    pub task_marker: String,
    /// Widest the note text gets before it centres: narrow, medium, wide, full.
    pub text_width: String,
    /// Launcher icon: a theme key, or empty to follow the colour theme.
    pub icon_theme: String,
    /// The hidden "Long Black" theme has been found (search for "coffee").
    pub coffee_unlocked: bool,
    /// Folder icons: `tag=icon` per entry (see `glyph::Icon::key`).
    pub tag_icons: Vec<String>,
    /// Folder icon style: boxicons (solid) or iconoir (outline).
    pub icon_set: String,
    /// Do not fetch title/description/picture for web links. (Inverted so
    /// the default is on.)
    pub link_previews_off: bool,
    /// Sidebar order of the top-level tags, top to bottom; an empty string
    /// is a spacer line. Tags not listed slot in alphabetically.
    pub tag_order: Vec<String>,
    /// Glide time of moving UI, in ms ("0" = snap; old preset keys still
    /// parse). Empty = default.
    pub animation: String,
    /// Landing softness of the glide: ease-out exponent in tenths
    /// ("10" = linear … "60" = floatiest). Empty = default.
    pub animation_ease: String,
    /// Editor body weight: 200, 300, 400 or 500. 0 = default (400).
    pub editor_weight: u16,
    /// Colour buffet: paint with the buffet pairing instead of `theme`.
    pub buffet_on: bool,
    /// Buffet highlight: a minimal theme key. Empty = tomato.
    pub buffet_highlight: String,
    /// Buffet dark plate key (see `retro::Dark::key`). Empty = rich black.
    pub buffet_dark: String,
    /// Buffet side: "light" for the light mode, anything else = dark.
    pub buffet_mode: String,
    /// Buffet paper plate key (see `retro::Light::key`). Empty = pearl white.
    pub buffet_light: String,
    /// Buffet writing colour key (see `retro::Ink::key`). Empty = ink.
    pub buffet_ink: String,
    /// Hide the table toolbar (+/− row/column, money). (Inverted so the
    /// default is shown.)
    pub table_toolbar_off: bool,
    /// Weight of the `---` divider: small, medium, large, family.
    pub rule_size: String,
    /// Border weight of link/file cards, same scale.
    pub card_line: String,
    /// Weight of the sidebar's tag spacer lines, same scale.
    pub tag_line_size: String,
    /// Draw no border around link/file cards. (Inverted so the default is on.)
    pub card_border_off: bool,
    /// Sync server address (a PocketBase instance, see `server/`). Empty = off.
    pub sync_url: String,
    /// Account email on that server. The token lives in the keyring.
    pub sync_email: String,
    /// Last window size in logical pixels; 0 = let the compositor choose.
    pub window_width: u32,
    pub window_height: u32,
    /// Proofreading marks: red under misspellings, blue under grammar
    /// slips (the dock's S and G). Both off by default: sometimes you
    /// just want to write.
    pub proof_spelling: bool,
    pub proof_grammar: bool,
}

impl Config {
    /// Resolve the notes directory, falling back to the platform default
    /// (`~/Documents/Attic`); `JJB_NOTES_DIR` overrides both.
    pub fn notes_dir(&self) -> PathBuf {
        jjb_core::platform::notes_dir(&self.notes_dir)
    }

    /// The derived SQLite index. Safe to delete — rebuilt from the notes dir on start.
    pub fn index_path(app_id: &str) -> PathBuf {
        jjb_core::platform::index_path(app_id)
    }
}
