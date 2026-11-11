// SPDX-License-Identifier: GPL-3.0-only

//! Editable note text: a cosmic-text editor over a buffer the renderer
//! draws directly. Exposes the small API the app uses (`perform`,
//! `move_to`, `cursor`, `line`, `text`, `selection`).

use crate::markdown;
use jjb_core::proof;
use cosmic::iced::advanced::graphics::text as gtext;
use cosmic::iced::{Font, Point, Rectangle, Size};
use cosmic::widget::text_editor::{Action, Cursor, Edit, Line, LineEnding, Motion, Position};
use cosmic_text::{Attrs, AttrsList, Buffer, BufferRef, Edit as _, Metrics, Shaping};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::{Arc, Weak};

/// The text of one block.
pub type Content = RichContent;

/// The rich editor's text: a cosmic-text editor over a shared buffer so the
/// renderer can draw the very same buffer (`fill_raw`) that is edited.
pub struct RichContent(RefCell<Inner>);

struct Inner {
    editor: cosmic_text::Editor<'static>,
    /// What the buffer was last shaped for (width, font, size, line height).
    shaped_for: Option<(f32, Font, f32, f32)>,
    style: Option<markdown::Settings>,
    /// Whether text changed since attributes were last applied.
    dirty: bool,
    /// The caret's line at the last styling (markers shown there).
    active: Option<usize>,
    /// Per line: hash of (text, in-fence) and whether it was styled active.
    line_keys: Vec<(u64, bool)>,
    /// Per line: the proofreading answer and the (text hash, wanted kinds)
    /// it was computed for. `None` = not asked yet, or asked before the
    /// engine was loaded — ask again.
    proof: Vec<Option<(u64, proof::Want, Vec<proof::Issue>)>>,
    /// Set by the app after an automatic rewrite (`[]` → task box, a box
    /// toggle): the widget should render the caret's line, not reveal it.
    render_hint: bool,
    /// The widget's layout bounds, recorded on every event (drawing skips
    /// editors scrolled out of view, events do not). In the scroll
    /// container's coordinate space, the same one `pointer_y` uses.
    bounds: Rectangle,
    /// The pointer's last known vertical position, in the same coordinate
    /// space as `bounds`; a drag from outside is forwarded as pointer motion.
    pointer_y: Option<f32>,
    /// A drop indicator to draw before this line (`line_count` = after the
    /// last line), while a file is being dragged over the note.
    drop_marker: Option<usize>,
    /// The app asked for keyboard focus (dock click, format action). The
    /// widget takes it on its next event — id-free, unlike a focus
    /// operation, so no other widget gets unfocused along the way.
    focus_requested: bool,
    /// The app moved or edited at the caret: once laid out, the widget
    /// should report where the caret's line sits so the note scrolls to it.
    reveal_requested: bool,
    /// The app asked this editor to let go of keyboard focus (a sibling
    /// took it, or a table took the keys).
    unfocus_requested: bool,
}

/// A task box to draw: its square, the mark inside, and whether it is done.
pub struct TaskBox {
    pub rect: Rectangle,
    pub mark: String,
    pub done: bool,
}

/// Something the pointer can act on.
pub struct Hotspot {
    pub rect: Rectangle,
    pub kind: HotKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotKind {
    /// A `[[wiki link]]` target (alias stripped).
    Link(String),
    /// A `#tag` (with its hash).
    Tag(String),
    /// A drawn task box; clicking toggles it.
    Task,
}

#[derive(Default)]
pub struct Overlays {
    pub hotspots: Vec<Hotspot>,
    /// Where a tag's hash sits and the 8-bit icon drawn over it.
    pub tag_icons: Vec<(Rectangle, crate::glyph::Icon)>,
    pub code_bgs: Vec<Rectangle>,
    pub code_block_rows: Vec<Rectangle>,
    pub boxes: Vec<TaskBox>,
    pub bullets: Vec<Rectangle>,
    pub quote_bars: Vec<Rectangle>,
    pub strikes: Vec<(Rectangle, Option<cosmic_text::Color>)>,
    /// Highlighter bands behind `==marked==` text.
    pub marks: Vec<Rectangle>,
    /// Proofreading underlines: a thin band under each flagged stretch.
    pub proof: Vec<(Rectangle, proof::Kind)>,
}

/// The caret or selection, in buffer coordinates.
pub enum Highlight {
    Caret(Point),
    Range(Vec<Rectangle>),
}

impl RichContent {
    pub fn with_text(text: &str) -> Self {
        let mut buffer = Buffer::new_empty(Metrics::new(15.0, 22.5));
        buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
        let mut guard = gtext::font_system().write().expect("font system");
        buffer.shape_until_scroll(guard.raw(), false);
        let editor = cosmic_text::Editor::new(BufferRef::Arc(Arc::new(buffer)));
        Self(RefCell::new(Inner {
            editor,
            shaped_for: None,
            style: None,
            dirty: true,
            active: None,
            line_keys: Vec::new(),
            proof: Vec::new(),
            render_hint: false,
            bounds: Rectangle::default(),
            pointer_y: None,
            drop_marker: None,
            focus_requested: false,
            reveal_requested: false,
            unfocus_requested: false,
        }))
    }

    /// Ask the widget to show the caret's line rendered at the next layout
    /// (used after `[]` expands to a task box, or a box is toggled).
    pub fn render_now(&mut self) {
        self.0.get_mut().render_hint = true;
    }

    /// Whether a Backspace (`before == true`) or Delete at the caret would
    /// remove a character that is hidden when the line is rendered — a
    /// marker, a task box, a list marker. Deleting those blind is the
    /// classic hidden-markdown trap, so the widget reveals the line first.
    pub fn delete_touches_marker(&self, before: bool) -> bool {
        use markdown::Kind;
        let cursor = self.cursor();
        if cursor.selection.is_some() {
            return false;
        }
        let Some(line) = self.line(cursor.position.line) else {
            return false;
        };
        let text = line.text.to_string();
        let col = cursor.position.column;
        let byte = if before {
            let Some(prev) = col.checked_sub(1) else {
                return false;
            };
            match text.char_indices().nth(prev) {
                Some((b, _)) => b,
                None => return false,
            }
        } else {
            match text.char_indices().nth(col) {
                Some((b, _)) => b,
                None => return false,
            }
        };
        let (spans, _) = markdown::scan_line(&text, false);
        spans.iter().any(|s| {
            s.range.contains(&byte)
                && matches!(
                    s.kind,
                    Kind::Marker
                        | Kind::LinkUrl
                        | Kind::ListMarker
                        | Kind::TaskBox
                        | Kind::TaskDone
                        | Kind::QuoteMarker
                )
        })
    }

    pub fn set_bounds(&self, r: Rectangle) {
        self.0.borrow_mut().bounds = r;
    }

    pub fn bounds(&self) -> Rectangle {
        self.0.borrow().bounds
    }

    pub fn set_pointer_y(&self, y: Option<f32>) {
        self.0.borrow_mut().pointer_y = y;
    }

    pub fn pointer_y(&self) -> Option<f32> {
        self.0.borrow().pointer_y
    }

    pub fn request_focus(&self) {
        self.0.borrow_mut().focus_requested = true;
    }

    pub fn request_reveal(&self) {
        self.0.borrow_mut().reveal_requested = true;
    }

    pub fn take_reveal_request(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().reveal_requested)
    }

    /// Top and height of the caret's line in buffer coordinates (valid
    /// after a layout).
    pub fn caret_line_span(&self) -> (f32, f32) {
        let line = self.cursor().position.line;
        (self.line_top(line), self.line_height_at_cursor())
    }

    pub fn take_focus_request(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().focus_requested)
    }

    pub fn request_unfocus(&self) {
        self.0.borrow_mut().unfocus_requested = true;
    }

    pub fn take_unfocus_request(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().unfocus_requested)
    }

    pub fn set_drop_marker(&self, line: Option<usize>) {
        self.0.borrow_mut().drop_marker = line;
    }

    pub fn drop_marker(&self) -> Option<usize> {
        self.0.borrow().drop_marker
    }

    /// The line under `y` (buffer coordinates) and whether the pointer is
    /// in its lower half — i.e. whether a drop belongs after it.
    pub fn line_at_y(&self, y: f32) -> (usize, bool) {
        self.with_buffer(|b| {
            let mut last = (0, true);
            for run in b.layout_runs() {
                if y < run.line_top {
                    return (run.line_i, false);
                }
                if y < run.line_top + run.line_height {
                    return (run.line_i, y > run.line_top + run.line_height / 2.0);
                }
                last = (run.line_i, true);
            }
            last
        })
    }

    /// Vertical position (buffer coordinates) of the top of `line`, or the
    /// bottom of the last line when `line` is past the end.
    pub fn line_top(&self, line: usize) -> f32 {
        self.with_buffer(|b| {
            let mut bottom = 0.0;
            for run in b.layout_runs() {
                if run.line_i == line {
                    return run.line_top;
                }
                bottom = run.line_top + run.line_height;
            }
            bottom
        })
    }

    pub fn take_render_hint(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().render_hint)
    }

    fn with_buffer<T>(&self, f: impl FnOnce(&Buffer) -> T) -> T {
        self.0.borrow().editor.with_buffer(f)
    }

    /// A weak handle for the renderer; a new one every draw (mutation
    /// through `Arc::make_mut` invalidates old ones).
    pub fn weak_buffer(&self) -> Weak<Buffer> {
        match self.0.borrow().editor.buffer_ref() {
            BufferRef::Arc(a) => Arc::downgrade(a),
            _ => unreachable!("rich content always shares its buffer"),
        }
    }

    pub fn perform(&mut self, action: Action) {
        let inner = self.0.get_mut();
        let mut guard = gtext::font_system().write().expect("font system");
        let fs = guard.raw();
        let editor = &mut inner.editor;
        match action {
            Action::Move(motion) => {
                if let Some((start, end)) = editor.selection_bounds() {
                    editor.set_selection(cosmic_text::Selection::None);
                    match motion {
                        Motion::Home
                        | Motion::End
                        | Motion::DocumentStart
                        | Motion::DocumentEnd => {
                            editor.action(fs, cosmic_text::Action::Motion(to_motion(motion)));
                        }
                        _ => editor.set_cursor(match motion.direction() {
                            cosmic::iced::advanced::text::editor::Direction::Left => start,
                            cosmic::iced::advanced::text::editor::Direction::Right => end,
                        }),
                    }
                } else {
                    editor.action(fs, cosmic_text::Action::Motion(to_motion(motion)));
                }
            }
            Action::Select(motion) => {
                let cursor = editor.cursor();
                if editor.selection_bounds().is_none() {
                    editor.set_selection(cosmic_text::Selection::Normal(cursor));
                }
                editor.action(fs, cosmic_text::Action::Motion(to_motion(motion)));
                if let Some((s, e)) = editor.selection_bounds()
                    && s.line == e.line
                    && s.index == e.index
                {
                    editor.set_selection(cosmic_text::Selection::None);
                }
            }
            Action::SelectWord => {
                let c = editor.cursor();
                editor.set_selection(cosmic_text::Selection::Word(c));
            }
            Action::SelectLine => {
                let c = editor.cursor();
                editor.set_selection(cosmic_text::Selection::Line(c));
            }
            Action::SelectAll => {
                let non_empty = editor.with_buffer(|b| {
                    b.lines.len() > 1 || b.lines.first().is_some_and(|l| !l.text().is_empty())
                });
                if non_empty {
                    let c = editor.cursor();
                    editor.set_selection(cosmic_text::Selection::Normal(cosmic_text::Cursor {
                        line: 0,
                        index: 0,
                        ..c
                    }));
                    editor.action(
                        fs,
                        cosmic_text::Action::Motion(cosmic_text::Motion::BufferEnd),
                    );
                }
            }
            Action::ClearSelection => editor.set_selection(cosmic_text::Selection::None),
            Action::Edit(edit) => {
                match edit {
                    Edit::Insert(c) => editor.action(fs, cosmic_text::Action::Insert(c)),
                    Edit::Paste(text) => editor.insert_string(&text, None),
                    Edit::Indent => editor.action(fs, cosmic_text::Action::Indent),
                    Edit::Unindent => editor.action(fs, cosmic_text::Action::Unindent),
                    Edit::Enter => editor.action(fs, cosmic_text::Action::Enter),
                    Edit::Backspace => editor.action(fs, cosmic_text::Action::Backspace),
                    Edit::Delete => editor.action(fs, cosmic_text::Action::Delete),
                }
                inner.dirty = true;
            }
            Action::Click(p) => editor.action(
                fs,
                cosmic_text::Action::Click {
                    x: p.x as i32,
                    y: p.y as i32,
                },
            ),
            Action::Drag(p) => {
                editor.action(
                    fs,
                    cosmic_text::Action::Drag {
                        x: p.x as i32,
                        y: p.y as i32,
                    },
                );
                if let Some((s, e)) = editor.selection_bounds()
                    && s.line == e.line
                    && s.index == e.index
                {
                    editor.set_selection(cosmic_text::Selection::None);
                }
            }
            // The note's scrollable scrolls; the buffer never does.
            Action::Scroll { .. } => {}
        }
        editor.shape_as_needed(fs, false);
    }

    pub fn move_to(&mut self, cursor: Cursor) {
        let inner = self.0.get_mut();
        inner.editor.set_cursor(cosmic_text::Cursor {
            line: cursor.position.line,
            index: cursor.position.column,
            affinity: cosmic_text::Affinity::Before,
        });
        inner.editor.set_selection(match cursor.selection {
            Some(s) => cosmic_text::Selection::Normal(cosmic_text::Cursor {
                line: s.line,
                index: s.column,
                affinity: cosmic_text::Affinity::Before,
            }),
            None => cosmic_text::Selection::None,
        });
    }

    pub fn cursor(&self) -> Cursor {
        let inner = self.0.borrow();
        let c = inner.editor.cursor();
        let selection = match inner.editor.selection() {
            cosmic_text::Selection::None => None,
            cosmic_text::Selection::Normal(s)
            | cosmic_text::Selection::Line(s)
            | cosmic_text::Selection::Word(s) => Some(Position {
                line: s.line,
                column: s.index,
            }),
        };
        Cursor {
            position: Position {
                line: c.line,
                column: c.index,
            },
            selection,
        }
    }

    pub fn line_count(&self) -> usize {
        self.with_buffer(|b| b.lines.len())
    }

    pub fn line(&self, index: usize) -> Option<Line<'_>> {
        self.with_buffer(|b| {
            b.lines.get(index).map(|l| Line {
                text: Cow::Owned(l.text().to_owned()),
                ending: match l.ending() {
                    cosmic_text::LineEnding::Lf => LineEnding::Lf,
                    cosmic_text::LineEnding::CrLf => LineEnding::CrLf,
                    cosmic_text::LineEnding::Cr => LineEnding::Cr,
                    cosmic_text::LineEnding::LfCr => LineEnding::LfCr,
                    cosmic_text::LineEnding::None => LineEnding::None,
                },
            })
        })
    }

    pub fn text(&self) -> String {
        // Match iced: lines joined by their endings, plus a final newline.
        self.with_buffer(|b| {
            let mut out = String::new();
            for (i, l) in b.lines.iter().enumerate() {
                out.push_str(l.text());
                if i + 1 < b.lines.len() {
                    let e = l.ending();
                    out.push_str(if e == cosmic_text::LineEnding::None {
                        "\n"
                    } else {
                        e.as_str()
                    });
                }
            }
            out.push('\n');
            out
        })
    }

    pub fn selection(&self) -> Option<String> {
        self.0.borrow().editor.copy_selection()
    }

    pub fn is_empty(&self) -> bool {
        self.with_buffer(|b| {
            b.lines.is_empty() || (b.lines.len() == 1 && b.lines[0].text().is_empty())
        })
    }

    /// Bring the buffer in line with the widget: width, metrics, and the
    /// per-span attributes. `active` is the caret's line while the widget
    /// has focus: that line shows its markers, the others hide them. Only
    /// lines whose text or active state changed are re-styled.
    pub fn update(
        &self,
        width: f32,
        font: Font,
        size: f32,
        line_height: f32,
        style: &markdown::Settings,
        active: Option<usize>,
    ) {
        let mut inner = self.0.borrow_mut();
        let key = (width, font, size, line_height);
        let restyle_all = inner.shaped_for != Some(key) || inner.style.as_ref() != Some(style);
        if !restyle_all && !inner.dirty && inner.active == active {
            return;
        }
        let mut guard = gtext::font_system().write().expect("font system");
        let fs = guard.raw();
        let mut keys = std::mem::take(&mut inner.line_keys);
        let mut proofs = std::mem::take(&mut inner.proof);
        let want = style.proof;
        inner.editor.with_buffer_mut(|b| {
            if b.metrics().font_size != size || b.metrics().line_height != line_height {
                b.set_metrics(Metrics::new(size, line_height));
            }
            if b.size().0 != Some(width) {
                b.set_size(Some(width), None);
            }
            keys.resize(b.lines.len(), (0, false));
            proofs.resize(b.lines.len(), None);
            let mut in_fence = false;
            for (i, line) in b.lines.iter_mut().enumerate() {
                let is_active = active == Some(i);
                let (spans, after) = markdown::scan_line(line.text(), in_fence);
                let was_fence = in_fence;
                in_fence = after;
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                use std::hash::{Hash, Hasher};
                line.text().hash(&mut hasher);
                was_fence.hash(&mut hasher);
                let k = (hasher.finish(), is_active);
                // Proofreading, cached by text and by what was wanted. A
                // fenced line is code: never read. Unanswered (engine still
                // loading) stays `None` and is asked again next time.
                let proof_key = (k.0, want);
                if !want.any() {
                    proofs[i] = Some((k.0, want, Vec::new()));
                } else if proofs[i].as_ref().is_none_or(|(h, w, _)| (*h, *w) != proof_key) {
                    let answer = if was_fence {
                        Some(Vec::new())
                    } else {
                        proof::check_line(line.text(), want)
                    };
                    proofs[i] = answer.map(|issues| (k.0, want, issues));
                }
                if !restyle_all && keys[i] == k {
                    continue;
                }
                keys[i] = k;
                let level = super::style::heading_level(line.text());
                let base = super::style::line_base(font, size, line_height, level, style);
                let mut list = AttrsList::new(&base);
                for (j, span) in spans.iter().enumerate() {
                    let mut attrs = super::style::span_attrs(
                        span.kind,
                        &base,
                        line_height,
                        is_active,
                        level,
                        style,
                    );
                    // A tag with a folder icon wears it instead of its hash (off the caret line).
                    if span.kind == markdown::Kind::Tag && !is_active {
                        let tag = line.text()[span.range.clone()].trim_start_matches('#');
                        if let Some(icon) = crate::glyph::for_tag(tag, &style.tag_icons) {
                            let idx = crate::glyph::Icon::ALL
                                .iter()
                                .position(|i| *i == icon)
                                .unwrap_or(0);
                            let hash = attrs
                                .clone()
                                .color(cosmic_text::Color::rgba(0, 0, 0, 0))
                                .metadata(super::style::META_TAGICON_BASE + idx);
                            list.add_span(span.range.start..span.range.start + 1, &hash);
                            list.add_span(span.range.start + 1..span.range.end, &attrs);
                            continue;
                        }
                    }
                    // A task's `- ` gets no bullet: the box is the marker.
                    // It also keeps the line's own metrics — the widened
                    // footprint is only for the drawn bullet dot.
                    if span.kind == markdown::Kind::ListMarker
                        && !is_active
                        && spans.get(j + 1).is_some_and(|n| {
                            matches!(n.kind, markdown::Kind::TaskBox | markdown::Kind::TaskDone)
                        })
                    {
                        attrs = attrs.metadata(super::style::META_TASK_PREFIX);
                        if let Some(m) = base.metrics_opt {
                            attrs = attrs.metrics(m.into());
                        }
                    }
                    list.add_span(span.range.clone(), &attrs);
                }
                let _ = line.set_attrs_list(list);
            }
        });
        inner.line_keys = keys;
        inner.proof = proofs;
        inner.editor.shape_as_needed(fs, false);
        inner.shaped_for = Some(key);
        inner.style = Some(style.clone());
        inner.active = active;
        inner.dirty = false;
    }

    /// Everything the widget paints besides the glyphs, computed from the
    /// laid-out runs: code backgrounds, task boxes, bullets, quote bars,
    /// and strike-throughs.
    pub fn overlays(&self) -> Overlays {
        use super::style::*;
        let mut o = Overlays::default();
        let inner = self.0.borrow();
        let caret = inner.editor.cursor();
        let proofs = &inner.proof;
        inner.editor.with_buffer(|b| {
            let size = b.metrics().font_size;
            for run in b.layout_runs() {
                let line_text = b.lines.get(run.line_i).map(|l| l.text()).unwrap_or("");
                // Proofreading underlines: each issue's byte range → the
                // glyphs it covers on this run. The word under the caret
                // is left alone while it is still being typed.
                if let Some(Some((_, _, issues))) = proofs.get(run.line_i) {
                    for issue in issues {
                        let on_caret = run.line_i == caret.line
                            && issue.range.start <= caret.index
                            && caret.index <= issue.range.end;
                        if on_caret {
                            continue;
                        }
                        let (mut x0, mut x1) = (f32::MAX, f32::MIN);
                        for g in run
                            .glyphs
                            .iter()
                            .filter(|g| g.start < issue.range.end && g.end > issue.range.start)
                        {
                            x0 = x0.min(g.x);
                            x1 = x1.max(g.x + g.w);
                        }
                        if x0 < x1 {
                            o.proof.push((
                                Rectangle {
                                    x: x0,
                                    y: run.line_y + size * 0.14,
                                    width: x1 - x0,
                                    height: (size * 0.09).clamp(1.5, 2.5),
                                },
                                issue.kind,
                            ));
                        }
                    }
                }
                let span_rect = |meta: usize| -> Option<(Rectangle, usize, usize)> {
                    let mut x0 = f32::MAX;
                    let mut x1 = f32::MIN;
                    let (mut s0, mut e0) = (usize::MAX, 0);
                    for g in run.glyphs.iter().filter(|g| g.metadata == meta) {
                        x0 = x0.min(g.x);
                        x1 = x1.max(g.x + g.w);
                        s0 = s0.min(g.start);
                        e0 = e0.max(g.end);
                    }
                    (x0 < x1).then(|| {
                        (
                            Rectangle {
                                x: x0,
                                y: run.line_top,
                                width: x1 - x0,
                                height: run.line_height,
                            },
                            s0,
                            e0,
                        )
                    })
                };
                // One band per unbroken stretch: two marks on a row are two
                // strokes, not one across the words between them.
                let stretches = |meta: usize| -> Vec<Rectangle> {
                    let mut out: Vec<Rectangle> = Vec::new();
                    let mut open = false;
                    for g in run.glyphs.iter() {
                        if g.metadata != meta {
                            open = false;
                        } else if let Some(r) = out.last_mut().filter(|_| open) {
                            let x1 = (r.x + r.width).max(g.x + g.w);
                            r.x = r.x.min(g.x);
                            r.width = x1 - r.x;
                        } else {
                            open = true;
                            out.push(Rectangle {
                                x: g.x,
                                y: run.line_top,
                                width: g.w,
                                height: run.line_height,
                            });
                        }
                    }
                    out
                };
                for r in stretches(META_CODE) {
                    let h = size * 1.25;
                    o.code_bgs.push(Rectangle {
                        x: r.x - 3.0,
                        y: r.y + (r.height - h) / 2.0,
                        width: r.width + 6.0,
                        height: h,
                    });
                }
                for r in stretches(META_MARK) {
                    // A little taller than the glyphs, like a marker stroke.
                    let h = size * 1.35;
                    o.marks.push(Rectangle {
                        x: r.x - 2.0,
                        y: r.y + (r.height - h) / 2.0,
                        width: r.width + 4.0,
                        height: h,
                    });
                }
                if run.glyphs.iter().any(|g| g.metadata == META_CODE_BLOCK) {
                    o.code_block_rows.push(Rectangle {
                        x: 0.0,
                        y: run.line_top,
                        width: run.line_w.max(b.size().0.unwrap_or(run.line_w)),
                        height: run.line_height,
                    });
                }
                for (meta, done) in [(META_TASK_OPEN, false), (META_TASK_DONE, true)] {
                    if let Some((r, s0, e0)) = span_rect(meta) {
                        // `[x] ` → the mark between the brackets.
                        let span_text = line_text.get(s0..e0).unwrap_or("");
                        let inner = span_text
                            .strip_prefix('[')
                            .and_then(|t| t.split(']').next())
                            .unwrap_or("")
                            .trim();
                        let mark = match inner {
                            "" | " " => String::new(),
                            "x" | "X" => "✓".to_owned(),
                            m => m.to_owned(),
                        };
                        // Colour emoji ignore a transparent colour, so the real
                        // glyph stays visible: box it where it is, draw no copy.
                        let emoji = mark.chars().any(|c| c as u32 >= 0x1F000);
                        // The box sits where the bullet would: over the `- `
                        // when there is one, else over the brackets.
                        let side = (size * 1.1).min(r.width.max(1.0));
                        let left = if emoji {
                            r.x + 1.0
                        } else {
                            span_rect(META_TASK_PREFIX).map_or(r.x, |(p, _, _)| p.x) + 1.0
                        };
                        let (w, h) = if emoji {
                            (size * 1.5, side)
                        } else {
                            (side, side)
                        };
                        let rect = Rectangle {
                            x: left,
                            y: r.y + (r.height - h) / 2.0,
                            width: w,
                            height: h,
                        };
                        o.hotspots.push(Hotspot {
                            rect: rect.expand(3.0),
                            kind: HotKind::Task,
                        });
                        o.boxes.push(TaskBox {
                            rect,
                            mark: if emoji { String::new() } else { mark },
                            done,
                        });
                    }
                }
                if let Some((r, _, _)) = span_rect(META_BULLET) {
                    o.bullets.push(r);
                }
                for (idx, icon) in crate::glyph::Icon::ALL.iter().enumerate() {
                    if let Some((r, _, _)) = span_rect(META_TAGICON_BASE + idx) {
                        o.tag_icons.push((r, *icon));
                    }
                }
                // Links and tags: one hotspot per contiguous run of glyphs.
                let mut cur: Option<(usize, f32, f32, usize, usize)> = None;
                let flush = |cur: &mut Option<(usize, f32, f32, usize, usize)>,
                             o: &mut Overlays| {
                    if let Some((meta, x0, x1, s0, e0)) = cur.take() {
                        let text = line_text.get(s0..e0).unwrap_or("").trim();
                        let kind = if meta == META_LINK {
                            HotKind::Link(text.split('|').next().unwrap_or("").trim().to_owned())
                        } else {
                            HotKind::Tag(text.to_owned())
                        };
                        o.hotspots.push(Hotspot {
                            rect: Rectangle {
                                x: x0,
                                y: run.line_top,
                                width: x1 - x0,
                                height: run.line_height,
                            },
                            kind,
                        });
                    }
                };
                for g in run.glyphs {
                    match (&mut cur, g.metadata) {
                        (Some(c), m) if c.0 == m => {
                            c.1 = c.1.min(g.x);
                            c.2 = c.2.max(g.x + g.w);
                            c.3 = c.3.min(g.start);
                            c.4 = c.4.max(g.end);
                        }
                        (_, m) if m == META_LINK || m == META_TAG => {
                            flush(&mut cur, &mut o);
                            cur = Some((m, g.x, g.x + g.w, g.start, g.end));
                        }
                        _ => flush(&mut cur, &mut o),
                    }
                }
                flush(&mut cur, &mut o);
                if let Some((r, _, _)) = span_rect(META_QUOTE) {
                    o.quote_bars.push(Rectangle {
                        x: r.x + 2.0,
                        y: run.line_top + 2.0,
                        width: 3.0,
                        height: run.line_height - 4.0,
                    });
                }
                for d in run.decorations {
                    if !d.data.text_decoration.strikethrough || d.glyph_range.is_empty() {
                        continue;
                    }
                    // Skip leading blanks so the strike starts at the text.
                    let mut range = d.glyph_range.clone();
                    while range.start < range.end
                        && line_text
                            .get(run.glyphs[range.start].start..run.glyphs[range.start].end)
                            .is_some_and(|t| t.trim().is_empty())
                    {
                        range.start += 1;
                    }
                    if range.is_empty() {
                        continue;
                    }
                    let first = &run.glyphs[range.start];
                    let last = &run.glyphs[range.end - 1];
                    let m = d.data.strikethrough_metrics;
                    o.strikes.push((
                        Rectangle {
                            x: first.x,
                            y: run.line_y - m.offset * d.font_size,
                            width: last.x + last.w - first.x,
                            height: (m.thickness * d.font_size).max(1.0),
                        },
                        d.color_opt,
                    ));
                }
            }
        });
        o
    }

    /// Height of all laid-out lines.
    pub fn height(&self) -> f32 {
        self.with_buffer(|b| {
            b.layout_runs()
                .last()
                .map_or(b.metrics().line_height, |r| r.line_top + r.line_height)
        })
    }

    /// Where the caret is, or the rectangles of the selection.
    pub fn highlight(&self) -> Highlight {
        let inner = self.0.borrow();
        let cursor = inner.editor.cursor();
        let bounds = inner.editor.selection_bounds();
        inner.editor.with_buffer(|b| {
            let line_height = b.metrics().line_height;
            match bounds {
                Some((start, end)) => Highlight::Range(
                    b.layout_runs()
                        .filter(|r| r.line_i >= start.line && r.line_i <= end.line)
                        .flat_map(|r| {
                            let top = r.line_top;
                            let h = r.line_height;
                            r.highlight(start, end)
                                .filter(|(_, w)| *w > 0.0)
                                .map(move |(x, w)| Rectangle {
                                    x,
                                    y: top,
                                    width: w,
                                    height: h.max(line_height),
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect(),
                ),
                None => {
                    let point = b
                        .layout_runs()
                        .filter(|r| r.line_i == cursor.line)
                        .find_map(|r| {
                            r.cursor_position(&cursor)
                                .map(|x| Point::new(x, r.line_top))
                        })
                        .unwrap_or_else(|| {
                            // Past the last laid-out run (empty buffer or trailing empty line).
                            let y = b
                                .layout_runs()
                                .filter(|r| r.line_i < cursor.line)
                                .last()
                                .map_or(0.0, |r| r.line_top + r.line_height);
                            Point::new(0.0, y)
                        });
                    Highlight::Caret(point)
                }
            }
        })
    }

    pub fn line_height_at_cursor(&self) -> f32 {
        let inner = self.0.borrow();
        let cursor = inner.editor.cursor();
        inner.editor.with_buffer(|b| {
            b.layout_runs()
                .find(|r| r.line_i == cursor.line)
                .map_or(b.metrics().line_height, |r| r.line_height)
        })
    }

    pub fn size(&self) -> Size {
        self.with_buffer(|b| Size::new(b.size().0.unwrap_or(0.0), self.height()))
    }
}

fn to_motion(motion: Motion) -> cosmic_text::Motion {
    match motion {
        Motion::Left => cosmic_text::Motion::Left,
        Motion::Right => cosmic_text::Motion::Right,
        Motion::Up => cosmic_text::Motion::Up,
        Motion::Down => cosmic_text::Motion::Down,
        Motion::WordLeft => cosmic_text::Motion::LeftWord,
        Motion::WordRight => cosmic_text::Motion::RightWord,
        Motion::Home => cosmic_text::Motion::Home,
        Motion::End => cosmic_text::Motion::End,
        Motion::PageUp => cosmic_text::Motion::PageUp,
        Motion::PageDown => cosmic_text::Motion::PageDown,
        Motion::DocumentStart => cosmic_text::Motion::BufferStart,
        Motion::DocumentEnd => cosmic_text::Motion::BufferEnd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize, column: usize) -> Cursor {
        Cursor {
            position: Position { line, column },
            selection: None,
        }
    }

    #[test]
    fn rich_content_edits_like_iced() {
        let mut c = RichContent::with_text("hello\nworld");
        assert_eq!(c.line_count(), 2);
        assert_eq!(c.text(), "hello\nworld\n");
        c.perform(Action::Move(Motion::DocumentEnd));
        for ch in "!\nnew".chars() {
            c.perform(Action::Edit(if ch == '\n' {
                Edit::Enter
            } else {
                Edit::Insert(ch)
            }));
        }
        assert_eq!(c.text(), "hello\nworld!\nnew\n");
        assert_eq!(c.cursor().position, Position { line: 2, column: 3 });
        c.move_to(at(0, 0));
        for _ in 0..5 {
            c.perform(Action::Select(Motion::Right));
        }
        assert_eq!(c.selection().as_deref(), Some("hello"));
        c.perform(Action::Edit(Edit::Paste(Arc::new("bye".into()))));
        assert_eq!(c.text(), "bye\nworld!\nnew\n");
        assert_eq!(c.line(1).unwrap().text, "world!");
        c.perform(Action::Edit(Edit::Backspace));
        assert_eq!(c.text(), "by\nworld!\nnew\n");
        c.perform(Action::SelectAll);
        assert_eq!(c.selection().as_deref(), Some("by\nworld!\nnew"));
        c.perform(Action::Move(Motion::Left));
        assert_eq!(c.cursor().selection, None);
        assert_eq!(c.cursor().position, Position { line: 0, column: 0 });
    }

    #[test]
    fn rich_content_layout_and_caret() {
        let mut c = RichContent::with_text("one two three four five six seven eight nine ten");
        let settings = markdown::Settings {
            palette: crate::retro::Theme::Phosphor.palette(&cosmic::Theme::default()),
            show_markers: false,
            font: cosmic::font::mono(),
            tag_icons: Default::default(),
            icon_set: Default::default(),
            proof: Default::default(),
        };
        c.update(120.0, cosmic::font::mono(), 15.0, 22.5, &settings, None);
        // Narrow width wraps into several visual lines.
        assert!(c.height() > 22.5 * 2.0);
        c.perform(Action::Move(Motion::DocumentEnd));
        match c.highlight() {
            Highlight::Caret(p) => assert!(p.y > 22.5),
            Highlight::Range(_) => panic!("no selection expected"),
        }
        c.perform(Action::SelectAll);
        match c.highlight() {
            Highlight::Range(r) => assert!(r.len() >= 2),
            Highlight::Caret(_) => panic!("selection expected"),
        }
        assert!(!c.is_empty());
        assert!(RichContent::with_text("").is_empty());
    }

    #[test]
    fn hotspots_for_links_tags_and_boxes() {
        let c = RichContent::with_text("See [[Kyoto!|the trip]] and #travels/japan\n- [x] done");
        let settings = markdown::Settings {
            palette: crate::retro::Theme::Phosphor.palette(&cosmic::Theme::default()),
            show_markers: false,
            font: cosmic::font::mono(),
            tag_icons: Default::default(),
            icon_set: Default::default(),
            proof: Default::default(),
        };
        c.update(600.0, cosmic::font::mono(), 15.0, 22.5, &settings, None);
        let o = c.overlays();
        let kinds: Vec<&HotKind> = o.hotspots.iter().map(|h| &h.kind).collect();
        assert!(kinds.contains(&&HotKind::Link("Kyoto!".into())));
        assert!(kinds.contains(&&HotKind::Tag("#travels/japan".into())));
        assert!(kinds.contains(&&HotKind::Task));
        assert_eq!(o.boxes.len(), 1);
        assert_eq!(o.boxes[0].mark, "✓");
        assert!(o.boxes[0].rect.y > 20.0, "box on the second line");
    }
    #[test]
    fn each_mark_on_a_row_gets_its_own_band() {
        let c = RichContent::with_text("==one== two ==three==");
        let settings = markdown::Settings {
            palette: crate::retro::Theme::Phosphor.palette(&cosmic::Theme::default()),
            show_markers: false,
            font: cosmic::font::mono(),
            tag_icons: Default::default(),
            icon_set: Default::default(),
            proof: Default::default(),
        };
        c.update(600.0, cosmic::font::mono(), 15.0, 22.5, &settings, None);
        let o = c.overlays();
        assert_eq!(o.marks.len(), 2);
        assert!(o.marks[0].x + o.marks[0].width < o.marks[1].x, "a gap over `two`");
    }
}
