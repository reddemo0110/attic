// SPDX-License-Identifier: GPL-3.0-only

//! The note body as editable blocks: runs of text (each its own text editor)
//! with images sitting between them exactly where their `![…]` line is.

use crate::editor::Content;
use crate::images::{self, ImageRef, Segment};
use crate::links::LinkRef;
use cosmic::widget::{self, text_editor};

pub enum Block {
    Text {
        content: Content,
        id: widget::Id,
    },
    Image(ImageRef),
    /// A horizontal rule, drawn full-width; the markdown is kept verbatim.
    Rule(String),
    /// A link card: a web address or an attached file on its own line.
    Link(LinkRef),
    /// A live table (pipe table in the file).
    Table(crate::table::Table),
}

pub struct Blocks {
    pub items: Vec<Block>,
    /// Index of the text block that has (or last had) keyboard focus.
    pub focused: usize,
    /// A selection running across blocks, if there is one. Set through
    /// `set_span`; any rebuild of the blocks drops it.
    pub span: Option<Span>,
}

/// A selection that crosses block boundaries: text, the pictures, cards,
/// rules and tables between, and more text. Each text block is its own
/// editor and can only hold a selection inside itself, so the span is
/// kept here and painted into every block it covers. Both ends sit in
/// text blocks (text and non-text blocks alternate, so there always is
/// one); the head's position is that block's caret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub anchor: usize,
    pub anchor_pos: text_editor::Position,
    pub head: usize,
}

/// One end of a span: a text block and a place in it.
pub type SpanEnd = (usize, text_editor::Position);

const START: text_editor::Position = text_editor::Position { line: 0, column: 0 };

/// The last position in a text block.
pub fn end_pos(content: &Content) -> text_editor::Position {
    let line = content.line_count().saturating_sub(1);
    let column = content.line(line).map_or(0, |l| l.text.len());
    text_editor::Position { line, column }
}

/// Byte offset of `pos` in `text` (lines split on `\n`), clamped to the
/// text and to a character boundary.
fn offset_of(text: &str, pos: text_editor::Position) -> usize {
    let mut offset = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == pos.line {
            let mut col = pos.column.min(line.len());
            while !line.is_char_boundary(col) {
                col -= 1;
            }
            return offset + col;
        }
        offset += line.len() + 1;
    }
    text.len()
}

/// Whether pasted text carries a line that must stand alone to become a
/// block again: a picture or a table.
pub fn has_block_lines(text: &str) -> bool {
    text.split('\n')
        .any(|l| images::parse_line(l).is_some() || crate::table::separator_line(l))
}

impl Default for Blocks {
    fn default() -> Self {
        Self::from_body("")
    }
}

impl Blocks {
    pub fn from_body(body: &str) -> Self {
        let items = images::split(body)
            .into_iter()
            .map(|seg| match seg {
                Segment::Text(t) => Block::Text {
                    content: Content::with_text(&t),
                    id: widget::Id::unique(),
                },
                Segment::Image(r) => Block::Image(r),
                Segment::Rule(t) => Block::Rule(t),
                Segment::Link(l) => Block::Link(l),
                Segment::Table(t) => Block::Table(t),
            })
            .collect();
        Self {
            items,
            focused: 0,
            span: None,
        }
    }

    /// Reassemble the markdown body.
    pub fn body(&self) -> String {
        let segments: Vec<Segment> = self
            .items
            .iter()
            .map(|b| match b {
                Block::Text { content, .. } => Segment::Text(content_text(content)),
                Block::Image(r) => Segment::Image(r.clone()),
                Block::Rule(t) => Segment::Rule(t.clone()),
                Block::Link(l) => Segment::Link(l.clone()),
                Block::Table(t) => Segment::Table(t.clone()),
            })
            .collect();
        images::join(&segments)
    }

    pub fn segments(&self) -> Vec<Segment> {
        self.items
            .iter()
            .map(|b| match b {
                Block::Text { content, .. } => Segment::Text(content_text(content)),
                Block::Image(r) => Segment::Image(r.clone()),
                Block::Rule(t) => Segment::Rule(t.clone()),
                Block::Link(l) => Segment::Link(l.clone()),
                Block::Table(t) => Segment::Table(t.clone()),
            })
            .collect()
    }

    /// Image references with their block index.
    pub fn images(&self) -> Vec<(usize, &ImageRef)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match b {
                Block::Image(r) => Some((i, r)),
                _ => None,
            })
            .collect()
    }

    /// Link cards with their block index.
    pub fn links(&self) -> Vec<(usize, &LinkRef)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match b {
                Block::Link(l) => Some((i, l)),
                _ => None,
            })
            .collect()
    }

    pub fn table_mut(&mut self, block: usize) -> Option<&mut crate::table::Table> {
        match self.items.get_mut(block) {
            Some(Block::Table(t)) => Some(t),
            _ => None,
        }
    }

    pub fn image_mut(&mut self, block: usize) -> Option<&mut ImageRef> {
        match self.items.get_mut(block) {
            Some(Block::Image(r)) => Some(r),
            _ => None,
        }
    }

    pub fn text_mut(&mut self, block: usize) -> Option<&mut Content> {
        match self.items.get_mut(block) {
            Some(Block::Text { content, .. }) => Some(content),
            _ => None,
        }
    }

    pub fn text(&self, block: usize) -> Option<&Content> {
        match self.items.get(block) {
            Some(Block::Text { content, .. }) => Some(content),
            _ => None,
        }
    }

    pub fn focused_text(&mut self) -> Option<&mut Content> {
        self.text_mut(self.focused)
    }

    pub fn id(&self, block: usize) -> Option<widget::Id> {
        match self.items.get(block) {
            Some(Block::Text { id, .. }) => Some(id.clone()),
            _ => None,
        }
    }

    pub fn focused_id(&self) -> Option<widget::Id> {
        self.id(self.focused)
    }

    /// Previous / next text block relative to `block`.
    pub fn text_before(&self, block: usize) -> Option<usize> {
        (0..block)
            .rev()
            .find(|&i| matches!(self.items[i], Block::Text { .. }))
    }

    pub fn text_after(&self, block: usize) -> Option<usize> {
        (block + 1..self.items.len()).find(|&i| matches!(self.items[i], Block::Text { .. }))
    }

    pub fn last_text(&self) -> usize {
        (0..self.items.len())
            .rev()
            .find(|&i| matches!(self.items[i], Block::Text { .. }))
            .unwrap_or(0)
    }

    /// Rebuild from a body, keeping focus on the text block at `focus` with
    /// the cursor at `cursor` when possible.
    pub fn rebuild(&mut self, body: &str, focus: usize, cursor: Option<text_editor::Cursor>) {
        let fresh = Blocks::from_body(body);
        self.items = fresh.items;
        self.span = None;
        self.focused = if matches!(self.items.get(focus), Some(Block::Text { .. })) {
            focus
        } else {
            self.text_before(focus.min(self.items.len())).unwrap_or(0)
        };
        if let (Some(c), Some(content)) = (cursor, self.focused_text()) {
            content.move_to(c);
        }
    }

    /// Insert an image after the cursor line of the focused block. Returns
    /// the index of the text block that follows it.
    pub fn insert_image(&mut self, r: ImageRef) -> usize {
        self.insert_segment(Segment::Image(r))
    }

    /// Insert a link card the same way.
    pub fn insert_link(&mut self, l: LinkRef) -> usize {
        self.insert_segment(Segment::Link(l))
    }

    fn insert_segment(&mut self, seg: Segment) -> usize {
        let focused = self.focused;
        let Some(content) = self.text(focused) else {
            return focused;
        };
        let text = content_text(content);
        let cursor = content.cursor().position.line;
        let lines: Vec<&str> = text.split('\n').collect();
        let split_at = if lines.get(cursor).is_some_and(|l| l.trim().is_empty()) {
            cursor
        } else {
            cursor + 1
        };
        let before = lines[..split_at.min(lines.len())].join("\n");
        let after = lines[split_at.min(lines.len())..].join("\n");
        let mut segs = self.segments();
        segs.splice(
            focused..=focused,
            [Segment::Text(before), seg, Segment::Text(after)],
        );
        let body = images::join(&segs);
        self.rebuild(
            &body,
            focused + 2,
            Some(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 0 },
                selection: None,
            }),
        );
        focused + 2
    }

    /// Remove the image block at `block`, merging the text around it.
    pub fn remove_image(&mut self, block: usize) {
        self.remove_block(block);
    }

    /// Remove a non-text block (image, rule or link), merging the text around it.
    pub fn remove_block(&mut self, block: usize) {
        if !matches!(
            self.items.get(block),
            Some(Block::Image(_) | Block::Rule(_) | Block::Link(_) | Block::Table(_))
        ) {
            return;
        }
        let mut segs = self.segments();
        segs.remove(block);
        // Merge the two text segments that now touch; the caret goes to the
        // seam (the start of the text that followed the removed block).
        let mut cursor = None;
        if block > 0
            && block < segs.len()
            && let (Segment::Text(a), Segment::Text(b)) = (&segs[block - 1], &segs[block])
        {
            let (merged, line) = if a.is_empty() {
                (b.clone(), Some(0))
            } else if b.is_empty() {
                (a.clone(), None) // nothing followed: the end of `a`
            } else {
                (format!("{a}\n{b}"), Some(a.split('\n').count()))
            };
            cursor = line.map(|line| text_editor::Cursor {
                position: text_editor::Position { line, column: 0 },
                selection: None,
            });
            segs.splice(block - 1..=block, [Segment::Text(merged)]);
        }
        let body = images::join(&segs);
        let focus = block.saturating_sub(1);
        self.rebuild(&body, focus, cursor);
        let motion = if self.focused < block {
            text_editor::Motion::DocumentEnd
        } else {
            text_editor::Motion::DocumentStart
        };
        if let (None, Some(content)) = (cursor, self.focused_text()) {
            content.perform(text_editor::Action::Move(motion));
        }
    }

    // ----- selection across blocks -----

    /// Select from `anchor` to `head`, painting each text block's share of
    /// it. Ends in the same block make an ordinary selection there and no
    /// span.
    pub fn set_span(&mut self, anchor: SpanEnd, head: SpanEnd) {
        if self.text(anchor.0).is_none() || self.text(head.0).is_none() {
            return;
        }
        let (lo, hi) = (anchor.0.min(head.0), anchor.0.max(head.0));
        let down = head.0 > anchor.0;
        for (i, block) in self.items.iter_mut().enumerate() {
            let Block::Text { content, .. } = block else {
                continue;
            };
            let end = end_pos(content);
            // (where the selection starts from, where the caret sits)
            let (from, caret) = if anchor.0 == head.0 && i == anchor.0 {
                (Some(anchor.1), head.1)
            } else if anchor.0 == head.0 {
                (None, content.cursor().position)
            } else if i == anchor.0 {
                (Some(anchor.1), if down { end } else { START })
            } else if i == head.0 {
                (Some(if down { START } else { end }), head.1)
            } else if i > lo && i < hi {
                (Some(START), end)
            } else {
                (None, content.cursor().position)
            };
            content.move_to(text_editor::Cursor {
                position: caret,
                selection: from.filter(|f| *f != caret),
            });
        }
        self.span = (anchor.0 != head.0).then_some(Span {
            anchor: anchor.0,
            anchor_pos: anchor.1,
            head: head.0,
        });
    }

    /// Drop the span and the selections it painted; `keep` is a block
    /// whose own selection is about to be set by the caller.
    pub fn clear_span(&mut self, keep: Option<usize>) {
        if self.span.take().is_none() {
            return;
        }
        for (i, block) in self.items.iter_mut().enumerate() {
            if let Block::Text { content, .. } = block
                && Some(i) != keep
            {
                content.perform(text_editor::Action::ClearSelection);
            }
        }
    }

    /// The span's two ends in document order.
    pub fn span_ends(&self) -> Option<(SpanEnd, SpanEnd)> {
        let s = self.span?;
        self.text(s.anchor)?;
        let anchor = (s.anchor, s.anchor_pos);
        let head = (s.head, self.text(s.head)?.cursor().position);
        Some(if anchor.0 < head.0 {
            (anchor, head)
        } else {
            (head, anchor)
        })
    }

    /// The markdown the span covers, pictures and all, as it would read in
    /// the file. Ends with a newline when the last thing selected is a
    /// block, so its line is whole wherever it is pasted.
    pub fn span_markdown(&self) -> Option<String> {
        let ((lo, lo_pos), (hi, hi_pos)) = self.span_ends()?;
        let mut parts: Vec<String> = Vec::new();
        let mut ends_on_block = false;
        for i in lo..=hi {
            match &self.items[i] {
                Block::Text { content, .. } => {
                    let t = content_text(content);
                    let from = if i == lo { offset_of(&t, lo_pos) } else { 0 };
                    let to = if i == hi { offset_of(&t, hi_pos) } else { t.len() };
                    // An empty text block is only the gap between two
                    // blocks; it owns no line.
                    if to > from {
                        parts.push(t[from..to].to_owned());
                        ends_on_block = false;
                    }
                }
                Block::Image(r) => {
                    parts.push(r.to_markdown());
                    ends_on_block = true;
                }
                Block::Rule(t) => {
                    parts.push(t.clone());
                    ends_on_block = true;
                }
                Block::Link(l) => {
                    parts.push(l.to_markdown());
                    ends_on_block = true;
                }
                Block::Table(t) => {
                    parts.push(t.to_markdown());
                    ends_on_block = true;
                }
            }
        }
        let mut out = parts.join("\n");
        if ends_on_block {
            out.push('\n');
        }
        Some(out)
    }

    /// Delete everything the span covers; the text before and after it
    /// meet on one line with the caret at the seam. Returns whether there
    /// was a span.
    pub fn delete_span(&mut self) -> bool {
        let Some(((lo, lo_pos), (hi, hi_pos))) = self.span_ends() else {
            return false;
        };
        let mut segs = self.segments();
        let (Segment::Text(a), Segment::Text(b)) = (&segs[lo], &segs[hi]) else {
            return false;
        };
        let merged = format!("{}{}", &a[..offset_of(a, lo_pos)], &b[offset_of(b, hi_pos)..]);
        segs.splice(lo..=hi, [Segment::Text(merged)]);
        let body = images::join(&segs);
        self.rebuild(
            &body,
            lo,
            Some(text_editor::Cursor {
                position: lo_pos,
                selection: None,
            }),
        );
        true
    }

    /// Body line at which each block starts. Empty text blocks own no line
    /// (`images::join` skips them), so they share the next block's offset.
    /// The final entry is the total line count.
    pub fn line_offsets(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.items.len() + 1);
        let mut line = 0;
        for b in &self.items {
            out.push(line);
            line += match b {
                Block::Text { content, .. } => {
                    let t = content_text(content);
                    if t.is_empty() {
                        0
                    } else {
                        t.split('\n').count()
                    }
                }
                Block::Image(_) | Block::Rule(_) | Block::Link(_) => 1,
                // Rows + separator + maybe the size comment.
                Block::Table(t) => t.to_markdown().lines().count(),
            };
        }
        out.push(line);
        out
    }

    /// Re-split after typing: a `---` line inside a text block becomes a
    /// rule block of its own (images likewise). Keeps the caret where it
    /// was, in whichever block now owns that line. Returns whether the
    /// block structure changed.
    pub fn resplit(&mut self) -> bool {
        let body = self.body();
        let fresh = Blocks::from_body(&body);
        let same_shape = fresh.items.len() == self.items.len()
            && fresh
                .items
                .iter()
                .zip(&self.items)
                .all(|(a, b)| std::mem::discriminant(a) == std::mem::discriminant(b));
        if same_shape {
            return false;
        }
        let offsets = self.line_offsets();
        let (line, column) = self
            .text(self.focused)
            .map(|c| {
                let cur = c.cursor().position;
                (offsets[self.focused] + cur.line, cur.column)
            })
            .unwrap_or((0, 0));
        self.items = fresh.items;
        self.span = None;
        let offsets = self.line_offsets();
        // The text block that owns the caret's line (a rule owns its own
        // line, so a caret "on" it lands in the text after it).
        let owner = (0..self.items.len())
            .filter(|&i| matches!(self.items[i], Block::Text { .. }))
            .find(|&i| line >= offsets[i] && line < offsets[i + 1].max(offsets[i] + 1))
            .or_else(|| self.text_after(0).or(Some(0)))
            .unwrap_or(0);
        self.focused = if matches!(self.items.get(owner), Some(Block::Text { .. })) {
            owner
        } else {
            self.last_text()
        };
        let local = line.saturating_sub(offsets[self.focused]);
        if let Some(c) = self.focused_text() {
            // Never point past the end of the line we land on (cosmic-text
            // panics on an out-of-range caret).
            let local = local.min(c.line_count().saturating_sub(1));
            let len = c.line(local).map_or(0, |l| l.text.chars().count());
            c.move_to(text_editor::Cursor {
                position: text_editor::Position {
                    line: local,
                    column: column.min(len),
                },
                selection: None,
            });
        }
        true
    }

    /// Whether the focused block holds a line that should be its own block.
    pub fn needs_resplit(&self) -> bool {
        self.text(self.focused).is_some_and(|c| {
            content_text(c).split('\n').any(|l| {
                images::is_rule_line(l)
                    || images::parse_line(l).is_some()
                    || crate::links::parse_line(l).is_some()
                    // A finished `| --- |` row turns pipe lines into a table.
                    || crate::table::separator_line(l)
            })
        })
    }

    /// Move the image at `block` so that its line sits just before body line
    /// `target` (`target == line count` puts it last). Returns the image's
    /// new block index, or `None` when nothing changed.
    pub fn move_image(&mut self, block: usize, target: usize) -> Option<usize> {
        if matches!(self.items.get(block), Some(Block::Text { .. }) | None) {
            return None;
        }
        let offsets = self.line_offsets();
        let from = offsets[block];
        let total = *offsets.last()?;
        let target = target.min(total);
        if target == from || target == from + 1 {
            return None;
        }
        let body = self.body();
        let mut lines: Vec<&str> = body.lines().collect();
        let md = lines.remove(from);
        let at = if target > from { target - 1 } else { target };
        lines.insert(at, md);
        let mut new_body = lines.join("\n");
        new_body.push('\n');
        // Focus the text that now follows the moved block (the non-text
        // block that owns line `at`).
        let fresh = Blocks::from_body(&new_body);
        let fresh_offsets = fresh.line_offsets();
        let new_block = (0..fresh.items.len())
            .find(|&i| !matches!(fresh.items[i], Block::Text { .. }) && fresh_offsets[i] == at)?;
        let focus = fresh
            .text_after(new_block)
            .unwrap_or_else(|| fresh.last_text());
        self.rebuild(&new_body, focus, None);
        Some(new_block)
    }
}

/// If the cursor sits on a task box (`- [ ] ` / `- [x] `), flip it, marking
/// a finished task with `marker`. Returns whether anything changed.
pub fn toggle_task_at_cursor(content: &mut Content, marker: &str) -> bool {
    use crate::note::{list_marker, task_box};
    use text_editor::{Action, Cursor, Edit, Motion, Position};
    let cursor = content.cursor();
    let Some(line) = content.line(cursor.position.line) else {
        return false;
    };
    let text = line.text.to_string();
    let indent = text.len() - text.trim_start().len();
    let Some(lm) = list_marker(&text[indent..]) else {
        return false;
    };
    let box_start = indent + lm;
    let Some((_, done)) = task_box(&text[box_start..]) else {
        return false;
    };
    let close = text[box_start..].find(']').unwrap_or(0) + 1;
    let box_chars = text[box_start..box_start + close].chars().count();
    let start_col = text[..box_start].chars().count();
    // A click anywhere from the list marker through the box counts (the
    // rich editor draws the box where the marker is).
    let col = cursor.position.column;
    let marker_col = text[..indent].chars().count();
    if col < marker_col || col > start_col + box_chars {
        return false;
    }
    let flipped = if done {
        "[ ]".to_owned()
    } else {
        format!("[{marker}]")
    };
    content.move_to(Cursor {
        position: Position {
            line: cursor.position.line,
            column: start_col,
        },
        selection: None,
    });
    for _ in 0..box_chars {
        content.perform(Action::Select(Motion::Right));
    }
    content.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(flipped))));
    content.perform(Action::Move(Motion::End));
    true
}

/// Typing `[]` at the start of a line (after an optional `- `) turns it
/// into a task: `- [ ] `. Call right after a `]` was inserted.
pub fn expand_task_shorthand(content: &mut Content) -> bool {
    use crate::note::list_marker;
    use text_editor::{Action, Cursor, Edit, Motion, Position};
    let cursor = content.cursor();
    let Some(line) = content.line(cursor.position.line) else {
        return false;
    };
    let text = line.text.to_string();
    let before: String = text.chars().take(cursor.position.column).collect();
    let indent_len = before.len() - before.trim_start().len();
    let body = &before[indent_len..];
    let body = list_marker(body).map_or(body, |n| &body[n..]);
    if body != "[]" {
        return false;
    }
    let indent = &before[..indent_len];
    content.move_to(Cursor {
        position: Position {
            line: cursor.position.line,
            column: 0,
        },
        selection: None,
    });
    for _ in 0..before.chars().count() {
        content.perform(Action::Select(Motion::Right));
    }
    content.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(format!(
        "{indent}- [ ] "
    )))));
    true
}

/// Enter on a list line carries the list on: the new line starts with the
/// same marker (`1.` counts up, a ticked box comes back empty), and Enter
/// on an item with nothing after its marker ends the list by clearing the
/// marker instead. Returns false when Enter should behave as usual (not a
/// list line, a selection, or the caret inside the marker).
pub fn continue_list(content: &mut Content) -> bool {
    use crate::note::list_continuation;
    use text_editor::{Action, Cursor, Edit, Motion, Position};
    let cursor = content.cursor();
    if cursor.selection.is_some() {
        return false;
    }
    let Some(line) = content.line(cursor.position.line) else {
        return false;
    };
    let text = line.text.to_string();
    let Some((marker_len, next)) = list_continuation(&text) else {
        return false;
    };
    let column = cursor.position.column.min(text.len());
    if column < marker_len {
        return false;
    }
    if text[marker_len..].trim().is_empty() {
        // An empty item: Enter ends the list here.
        content.move_to(Cursor {
            position: Position {
                line: cursor.position.line,
                column: 0,
            },
            selection: None,
        });
        content.perform(Action::Select(Motion::End));
        content.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(String::new()))));
        return true;
    }
    content.perform(Action::Edit(Edit::Enter));
    for c in next.chars() {
        content.perform(Action::Edit(Edit::Insert(c)));
    }
    true
}

/// `Content::text()` appends a trailing newline; drop it so joins stay exact.
fn content_text(content: &Content) -> String {
    let mut t = content.text();
    if t.ends_with('\n') {
        t.pop();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: usize, column: usize) -> text_editor::Position {
        text_editor::Position { line, column }
    }

    #[test]
    fn span_copies_text_and_pictures_and_deletes_them() {
        let body = "alpha beta\n![p](assets/p.png)\ngamma delta\n---\nomega\n";
        let mut b = Blocks::from_body(body);
        // Text, Image, Text, Rule, Text.
        assert_eq!(b.items.len(), 5);
        // From "beta" down to just after "gamma".
        b.set_span((0, pos(0, 6)), (2, pos(0, 5)));
        assert!(b.span.is_some());
        assert_eq!(
            b.span_markdown().as_deref(),
            Some("beta\n![p](assets/p.png)\ngamma")
        );
        // Each block holds its share: the anchor to its end, the head
        // from its start.
        assert_eq!(b.text(0).unwrap().selection().as_deref(), Some("beta"));
        assert_eq!(b.text(2).unwrap().selection().as_deref(), Some("gamma"));
        assert_eq!(b.text(4).unwrap().selection(), None);
        // Selected upward, it reads the same.
        b.set_span((2, pos(0, 5)), (0, pos(0, 6)));
        assert_eq!(
            b.span_markdown().as_deref(),
            Some("beta\n![p](assets/p.png)\ngamma")
        );
        assert!(b.delete_span());
        assert_eq!(b.body(), "alpha  delta\n---\nomega\n");
        assert_eq!(b.span, None);
        assert_eq!(b.focused, 0);
        assert_eq!(b.text(0).unwrap().cursor().position, pos(0, 6));
    }

    #[test]
    fn span_ending_on_a_block_keeps_its_line_whole() {
        let mut b = Blocks::from_body("one\n![p](assets/p.png)\ntwo\n");
        // From the end of "one" to the start of "two": only the picture.
        b.set_span((0, pos(0, 3)), (2, pos(0, 0)));
        assert_eq!(b.span_markdown().as_deref(), Some("![p](assets/p.png)\n"));
        assert!(has_block_lines(&b.span_markdown().unwrap()));
        // The whole note.
        let end = end_pos(b.text(2).unwrap());
        b.set_span((0, pos(0, 0)), (2, end));
        assert_eq!(
            b.span_markdown().as_deref(),
            Some("one\n![p](assets/p.png)\ntwo")
        );
        // Ends in one block are an ordinary selection, not a span.
        b.set_span((2, pos(0, 0)), (2, pos(0, 2)));
        assert_eq!(b.span, None);
        assert_eq!(b.text(2).unwrap().selection().as_deref(), Some("tw"));
        assert_eq!(b.text(0).unwrap().selection(), None);
        // Clearing puts every block's selection away.
        b.set_span((0, pos(0, 0)), (2, pos(0, 1)));
        b.clear_span(None);
        assert_eq!(b.text(0).unwrap().selection(), None);
        assert_eq!(b.text(2).unwrap().selection(), None);
    }

    #[test]
    fn body_round_trips_and_images_insert_remove() {
        let mut b = Blocks::from_body("hello\nworld\n");
        assert_eq!(b.body(), "hello\nworld\n");
        assert_eq!(b.items.len(), 1);
        // cursor on line 0 ("hello") → image goes after it
        let r = images::parse_line("![p](assets/p.png)").unwrap();
        let next = b.insert_image(r);
        assert_eq!(next, 2);
        assert_eq!(b.body(), "hello\n![p](assets/p.png)\nworld\n");
        assert_eq!(b.images().len(), 1);
        b.remove_image(1);
        assert_eq!(b.body(), "hello\nworld\n");
    }

    #[test]
    fn rules_are_blocks_and_task_boxes_toggle() {
        use text_editor::{Action, Cursor, Edit, Motion, Position};
        let mut b = Blocks::from_body("one\n---\n- [ ] milk\n- [x] eggs\n");
        assert!(matches!(b.items.get(1), Some(Block::Rule(r)) if r == "---"));
        assert_eq!(b.line_offsets(), vec![0, 1, 2, 4]);
        assert_eq!(b.body(), "one\n---\n- [ ] milk\n- [x] eggs\n");
        let at = |line, column| Cursor {
            position: Position { line, column },
            selection: None,
        };
        let c = b.text_mut(2).unwrap();
        // Click on the box flips it; a click in the text does nothing.
        c.move_to(at(0, 3));
        assert!(toggle_task_at_cursor(c, "🦆"));
        c.move_to(at(1, 8));
        assert!(!toggle_task_at_cursor(c, "x"));
        c.move_to(at(1, 2));
        assert!(toggle_task_at_cursor(c, "x"));
        assert_eq!(b.body(), "one\n---\n- [🦆] milk\n- [ ] eggs\n");
        // A duck unticks again, and `[]` typed on a fresh line expands.
        let c = b.text_mut(2).unwrap();
        c.move_to(at(0, 4));
        assert!(toggle_task_at_cursor(c, "✓"));
        assert_eq!(b.body(), "one\n---\n- [ ] milk\n- [ ] eggs\n");
        let c = b.text_mut(2).unwrap();
        c.perform(Action::Move(Motion::DocumentEnd));
        for ch in "\n  []".chars() {
            c.perform(Action::Edit(if ch == '\n' {
                Edit::Enter
            } else {
                Edit::Insert(ch)
            }));
        }
        assert!(expand_task_shorthand(c));
        for ch in "bread".chars() {
            c.perform(Action::Edit(Edit::Insert(ch)));
        }
        assert_eq!(
            b.body(),
            "one\n---\n- [ ] milk\n- [ ] eggs\n  - [ ] bread\n"
        );
        assert!(!expand_task_shorthand(b.text_mut(2).unwrap()));
        b.remove_block(1);
        assert_eq!(b.body(), "one\n- [ ] milk\n- [ ] eggs\n  - [ ] bread\n");
        // The caret sits at the seam, not at the end of the merged text.
        let c = b.focused_text().unwrap();
        assert_eq!((c.cursor().position.line, c.cursor().position.column), (1, 0));
    }

    #[test]
    fn removing_a_table_or_a_last_block_places_the_caret_sensibly() {
        let mut b = Blocks::from_body("intro\n| a | b |\n| --- | --- |\n| 1 | 2 |\nafter\n");
        assert!(matches!(b.items.get(1), Some(Block::Table(_))));
        b.remove_block(1);
        assert_eq!(b.body(), "intro\nafter\n");
        let c = b.focused_text().unwrap();
        assert_eq!((c.cursor().position.line, c.cursor().position.column), (1, 0));
        let mut b = Blocks::from_body("intro\n---\n");
        b.remove_block(1);
        assert_eq!(b.body(), "intro\n");
        let c = b.focused_text().unwrap();
        assert_eq!((c.cursor().position.line, c.cursor().position.column), (0, 5));
    }

    #[test]
    fn enter_carries_lists_on_and_an_empty_item_ends_them() {
        use text_editor::{Action, Edit, Motion};
        let mut b = Blocks::from_body("- milk\n");
        let c = b.focused_text().unwrap();
        c.perform(Action::Move(Motion::DocumentEnd));
        assert!(continue_list(c));
        for ch in "eggs".chars() {
            c.perform(Action::Edit(Edit::Insert(ch)));
        }
        assert_eq!(b.body(), "- milk\n- eggs\n");
        // Enter, Enter: the second one sees an empty item and ends the list.
        assert!(continue_list(b.focused_text().unwrap()));
        assert_eq!(b.body(), "- milk\n- eggs\n- \n");
        assert!(continue_list(b.focused_text().unwrap()));
        let c = b.focused_text().unwrap();
        assert_eq!(c.line_count(), 3);
        assert_eq!(c.line(2).unwrap().text.to_string(), "");
        // Numbers count up; a ticked box comes back open; indent survives.
        let mut b = Blocks::from_body("  3. c\n- [x] done\n");
        let c = b.focused_text().unwrap();
        c.perform(Action::Move(Motion::End));
        assert!(continue_list(c));
        assert_eq!(b.body(), "  3. c\n  4. \n- [x] done\n");
        let c = b.focused_text().unwrap();
        c.perform(Action::Move(Motion::DocumentEnd));
        assert!(continue_list(c));
        assert_eq!(b.body(), "  3. c\n  4. \n- [x] done\n- [ ] \n");
        // Not a list line, or the caret inside the marker: plain Enter.
        let mut b = Blocks::from_body("plain\n- item\n");
        let c = b.focused_text().unwrap();
        c.perform(Action::Move(Motion::End));
        assert!(!continue_list(c));
        c.perform(Action::Move(Motion::Down));
        c.perform(Action::Move(Motion::Home));
        assert!(!continue_list(c));
        // Mid-item Enter splits the text and carries the marker.
        c.perform(Action::Move(Motion::End));
        c.perform(Action::Move(Motion::Left));
        c.perform(Action::Move(Motion::Left));
        assert!(continue_list(c));
        assert_eq!(b.body(), "plain\n- it\n- em\n");
    }

    #[test]
    fn typing_a_rule_splits_live_and_keeps_the_caret() {
        use text_editor::{Action, Edit, Motion};
        let mut b = Blocks::from_body("alpha\n");
        let c = b.focused_text().unwrap();
        c.perform(Action::Move(Motion::DocumentEnd));
        for ch in "\n---\nbeta".chars() {
            c.perform(Action::Edit(if ch == '\n' {
                Edit::Enter
            } else {
                Edit::Insert(ch)
            }));
        }
        assert!(b.needs_resplit());
        assert!(b.resplit());
        assert_eq!(b.body(), "alpha\n---\nbeta\n");
        assert!(matches!(b.items.get(1), Some(Block::Rule(_))));
        assert_eq!(b.focused, 2);
        let cur = b.text(2).unwrap().cursor().position;
        assert_eq!((cur.line, cur.column), (0, 4));
        assert!(!b.resplit());
        // Caret on the rule line itself (typed `---`, no Enter yet) lands in
        // the empty block after it, clamped to its length.
        let mut b = Blocks::from_body("alpha\n");
        let c = b.focused_text().unwrap();
        c.perform(Action::Move(Motion::DocumentEnd));
        for ch in "\n---".chars() {
            c.perform(Action::Edit(if ch == '\n' {
                Edit::Enter
            } else {
                Edit::Insert(ch)
            }));
        }
        assert!(b.resplit());
        assert_eq!(b.body(), "alpha\n---\n");
        assert_eq!(b.focused, 2);
        let cur = b.text(2).unwrap().cursor().position;
        assert_eq!((cur.line, cur.column), (0, 0));
    }

    #[test]
    fn move_image_between_lines() {
        let body = "one\ntwo\n![p](assets/p.png)\nthree\n\nfour\n";
        let mut b = Blocks::from_body(body);
        assert_eq!(b.line_offsets(), vec![0, 2, 3, 6]);
        // Same place (before or after its own line) is a no-op.
        assert_eq!(b.move_image(1, 2), None);
        assert_eq!(b.move_image(1, 3), None);
        assert_eq!(b.body(), body);
        // Up to the top.
        assert_eq!(b.move_image(1, 0), Some(1));
        assert_eq!(b.body(), "![p](assets/p.png)\none\ntwo\nthree\n\nfour\n");
        assert_eq!(b.line_offsets(), vec![0, 0, 1, 6]);
        // Into the middle of a paragraph splits it.
        assert_eq!(b.move_image(1, 4), Some(1));
        assert_eq!(b.body(), "one\ntwo\nthree\n![p](assets/p.png)\n\nfour\n");
        assert_eq!(b.focused, 2);
        // To the very end.
        assert_eq!(b.move_image(1, 6), Some(1));
        assert_eq!(b.body(), "one\ntwo\nthree\n\nfour\n![p](assets/p.png)\n");
        assert_eq!(b.focused, 2);
        assert_eq!(b.line_offsets(), vec![0, 5, 6, 6]);
        // Not an image block.
        assert_eq!(b.move_image(0, 3), None);
    }

    #[test]
    fn typing_after_an_inserted_image_goes_below_it() {
        use text_editor::{Action, Edit, Motion};
        let mut b = Blocks::from_body("Weekend\n\nDrove out early.\n");
        b.focused_text()
            .unwrap()
            .perform(Action::Move(Motion::DocumentEnd));
        let r = images::parse_line("![s](assets/s.png)").unwrap();
        let next = b.insert_image(r);
        assert_eq!(b.focused, next);
        for c in "Tomorrow".chars() {
            b.focused_text()
                .unwrap()
                .perform(Action::Edit(Edit::Insert(c)));
        }
        assert_eq!(
            b.body(),
            "Weekend\n\nDrove out early.\n![s](assets/s.png)\nTomorrow\n"
        );
    }
}
