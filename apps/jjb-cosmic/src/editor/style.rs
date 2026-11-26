// SPDX-License-Identifier: GPL-3.0-only

//! Span kind → cosmic-text attributes for the rich editor.
//!
//! Two looks per line: the *active* line (the caret's) shows its markdown
//! markers dimmed, like the stock editor; every other line hides them.
//! Hidden comes in two flavours: *collapsed* (transparent and ~0 px wide —
//! `**`, `# `, backticks) and *transparent* (invisible but full width —
//! `- `, `[x]`, `> `), which keeps the layout so the overlay pass can draw
//! a bullet, a task box or a quote bar in exactly that space.

use crate::markdown::{self, Kind, Settings};
use cosmic::iced::Font;
use cosmic::iced::advanced::graphics::text as gtext;
use cosmic_text::{Attrs, Family, Metrics, Weight};

/// Overlay metadata carried by glyphs (`Attrs::metadata`).
pub const META_CODE: usize = 1;
pub const META_CODE_BLOCK: usize = 2;
pub const META_TASK_OPEN: usize = 3;
pub const META_TASK_DONE: usize = 4;
pub const META_BULLET: usize = 5;
pub const META_QUOTE: usize = 6;
/// The `- ` before a task box: invisible, part of the box's footprint.
pub const META_TASK_PREFIX: usize = 7;
/// Clickable spans: a `[[wiki link]]` target and a `#tag`.
pub const META_LINK: usize = 8;
pub const META_TAG: usize = 9;
/// `==marked==` text: a highlighter band is drawn behind these glyphs.
pub const META_MARK: usize = 10;
/// The `#` of a tag wearing a folder icon: invisible at width, the icon
/// is drawn over it. Metadata = base + index into `glyph::Icon::ALL`.
pub const META_TAGICON_BASE: usize = 100;

/// Heading level from the line's leading hashes (0 = not a heading).
pub fn heading_level(line: &str) -> usize {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes > 0 && hashes <= 6 && t[hashes..].starts_with(' ') {
        hashes
    } else {
        0
    }
}

/// Font size multiplier for a heading level.
pub fn heading_scale(level: usize) -> f32 {
    match level {
        1 => 1.6,
        2 => 1.35,
        3 => 1.15,
        _ => 1.0,
    }
}

/// The per-line base attributes: body text, or heading text when the line
/// is a heading (so its markers and bold runs share the bigger metrics).
pub fn line_base(
    font: Font,
    size: f32,
    line_height: f32,
    level: usize,
    s: &Settings,
) -> Attrs<'static> {
    let base = gtext::to_attributes(font).color(gtext::to_color(s.palette.fg));
    if level > 0 {
        let k = heading_scale(level);
        base.metrics(Metrics::new(
            size * k,
            (line_height * k).max(size * k * 1.2),
        ))
    } else {
        base.metrics(Metrics::new(size, line_height))
    }
}

/// Attributes for one span. `active` is whether the caret is on this line,
/// `level` the line's heading level (0 = not a heading).
pub fn span_attrs(
    kind: Kind,
    base: &Attrs<'static>,
    line_height: f32,
    active: bool,
    level: usize,
    s: &Settings,
) -> Attrs<'static> {
    let p = &s.palette;
    let color = |c: cosmic::iced::Color| gtext::to_color(c);
    let collapsed = || {
        base.clone()
            .color(cosmic_text::Color::rgba(0, 0, 0, 0))
            .metrics(Metrics::new(0.5, line_height))
    };
    let transparent = |meta: usize| {
        base.clone()
            .color(cosmic_text::Color::rgba(0, 0, 0, 0))
            .metadata(meta)
    };
    let h = markdown::style_for(kind, s);
    let styled = || {
        let mut a = match h.font {
            Some(f) => {
                // Keep the line's metrics; take the highlighter's family/weight/style.
                let fa = gtext::to_attributes(f);
                base.clone()
                    .family(fa.family)
                    .weight(fa.weight)
                    .style(fa.style)
            }
            None => base.clone(),
        };
        if let Some(c) = h.color {
            a = a.color(color(c));
        }
        a
    };
    match kind {
        Kind::Marker | Kind::LinkUrl => {
            if active {
                styled()
            } else {
                collapsed()
            }
        }
        Kind::ListMarker => {
            if active {
                styled()
            } else {
                // Spread the literal `- ` out so the drawn dot gets a
                // conventional gutter before the text — proportional faces
                // shape the hyphen-space pair far too narrow for that.
                // Spacing (in ems), not a bigger font: taller glyphs would
                // push the whole line's baseline down, away from the dot.
                transparent(META_BULLET).letter_spacing(0.36)
            }
        }
        Kind::TaskBox => {
            if active {
                styled()
            } else {
                transparent(META_TASK_OPEN)
            }
        }
        Kind::TaskDone => {
            if active {
                styled()
            } else {
                transparent(META_TASK_DONE)
            }
        }
        Kind::QuoteMarker => {
            if active {
                styled()
            } else {
                transparent(META_QUOTE)
            }
        }
        // Numbered markers keep their digits on screen, in the accent.
        Kind::NumMarker => styled(),
        Kind::Quote => styled(),
        Kind::Code => styled().family(Family::Monospace).metadata(META_CODE),
        Kind::CodeBlock => styled().family(Family::Monospace).metadata(META_CODE_BLOCK),
        Kind::Done => styled()
            .strikethrough()
            .strikethrough_color(color(p.fg.scale_alpha(0.55))),
        Kind::Strike => styled()
            .strikethrough()
            .strikethrough_color(color(p.fg.scale_alpha(0.8))),
        // H3 down take the palette's `h3` colour — the accent on the
        // classics, plain text on the minimal themes.
        Kind::Heading if level >= 3 => styled().weight(Weight::BOLD).color(color(p.h3)),
        Kind::Heading if level == 2 => styled().weight(Weight::BOLD).color(color(p.h2)),
        Kind::Heading => styled().weight(Weight::BOLD),
        Kind::Link => styled().metadata(META_LINK),
        Kind::Tag => styled().metadata(META_TAG),
        Kind::Mark => styled().metadata(META_MARK),
        Kind::Bold | Kind::Italic | Kind::BoldItalic => styled(),
    }
}
