// SPDX-License-Identifier: GPL-3.0-only

//! The core's markdown scanner plus the iced highlighter built on it: how
//! each span kind is painted — bold text bold, tags and links in the
//! secondary accent, and the syntax markers themselves in a "ghost" colour
//! so the text reads as formatted while the cursor can still move over them.

pub use jjb_core::markdown::*;

use crate::retro::Palette;
use cosmic::iced::font::{Style, Weight};
use cosmic::iced::{Color, Font};

#[derive(Clone, PartialEq)]
pub struct Settings {
    pub palette: Palette,
    pub show_markers: bool,
    pub font: Font,
    /// Folder icons by tag; a tag wearing one shows it instead of its `#`.
    pub tag_icons: std::sync::Arc<std::collections::HashMap<String, crate::glyph::Icon>>,
    pub icon_set: crate::glyph::IconSet,
    /// Which proofreading marks to draw (the dock's S and G toggles). The
    /// app only asks once the engine is loaded, so an editor never caches
    /// "nothing wrong" for a line that was never read.
    pub proof: jjb_core::proof::Want,
}

#[derive(Clone, Copy, Debug)]
pub struct Highlight {
    pub color: Option<Color>,
    pub font: Option<Font>,
}

/// Colour and font for a span kind under `settings` — shared by iced's
/// highlighter path and the rich editor.
pub fn style_for(kind: Kind, settings: &Settings) -> Highlight {
    {
        let p = &settings.palette;
        let base = settings.font;
        let bold = Font {
            weight: Weight::Bold,
            ..base
        };
        let italic = Font {
            style: Style::Italic,
            ..base
        };
        let bold_italic = Font {
            weight: Weight::Bold,
            style: Style::Italic,
            ..base
        };
        let ghost = if settings.show_markers {
            p.dim
        } else {
            p.mute.scale_alpha(0.45)
        };
        let (color, font) = match kind {
            Kind::Marker | Kind::QuoteMarker => (ghost, None),
            Kind::Bold => (p.fg, Some(bold)),
            Kind::Italic => (p.fg, Some(italic)),
            Kind::BoldItalic => (p.fg, Some(bold_italic)),
            Kind::Code | Kind::CodeBlock => (p.accent2, None),
            Kind::Heading => (p.accent, Some(bold)),
            Kind::Tag | Kind::Link => (p.accent2, None),
            Kind::LinkUrl => (ghost, None),
            Kind::ListMarker | Kind::NumMarker => (p.accent, None),
            Kind::Quote => (p.dim, Some(italic)),
            // Finished tasks fade into the theme rather than change colour.
            Kind::Done | Kind::Strike => (p.fg.scale_alpha(0.45), None),
            // The band behind it carries the colour; the text stays put.
            Kind::Mark => (p.fg, None),
            Kind::TaskBox => (p.dim, None),
            Kind::TaskDone => (p.accent, Some(bold)),
        };
        Highlight {
            color: Some(color),
            font,
        }
    }
}

