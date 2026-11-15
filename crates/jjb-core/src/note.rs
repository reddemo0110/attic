// SPDX-License-Identifier: GPL-3.0-only

//! The note document format and the pure text functions that operate on it.
//!
//! A note on disk is a markdown file with a small YAML-ish frontmatter block
//! carrying what the filesystem cannot: a stable id, creation time, pinned
//! state. Everything below the frontmatter is the body, and the body is the
//! source of truth — title, tags and links are all derived from it.

use chrono::{DateTime, SecondsFormat, Utc};
use std::path::PathBuf;

pub const UNTITLED: &str = "Untitled";
const PREVIEW_CHARS: usize = 140;
const MAX_FILENAME_CHARS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    /// Derived from the first non-blank line of `body`.
    pub title: String,
    /// Markdown body, without frontmatter.
    pub body: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub pinned: bool,
    pub trashed: bool,
    /// Frontmatter lines we don't understand, preserved verbatim on save.
    pub extra_frontmatter: Vec<String>,
    /// Absolute path of the backing file.
    pub path: PathBuf,
}

/// The lightweight row shown in the note list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub modified: DateTime<Utc>,
    pub pinned: bool,
    pub trashed: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub id: Option<String>,
    pub created: Option<DateTime<Utc>>,
    pub pinned: bool,
    pub extra: Vec<String>,
}

/// Split a document into its frontmatter and body.
///
/// Frontmatter is optional. When present it must start on the very first
/// line with `---` and end with a `---` line.
pub fn parse_document(text: &str) -> (Frontmatter, &str) {
    let mut fm = Frontmatter::default();
    let Some(rest) = text.strip_prefix("---") else {
        return (fm, text);
    };
    // The opening fence must be alone on its line.
    let rest = match rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
    {
        Some(r) => r,
        None => return (fm, text),
    };
    let mut consumed = text.len() - rest.len();
    let mut body_start = None;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        consumed += line.len();
        if trimmed == "---" {
            body_start = Some(consumed);
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "id" if !value.is_empty() => fm.id = Some(value.to_owned()),
                "created" => {
                    fm.created = DateTime::parse_from_rfc3339(value)
                        .ok()
                        .map(|d| d.with_timezone(&Utc));
                }
                "pinned" => fm.pinned = matches!(value, "true" | "yes" | "1"),
                _ => fm.extra.push(trimmed.to_owned()),
            }
        } else if !trimmed.trim().is_empty() {
            fm.extra.push(trimmed.to_owned());
        }
    }
    match body_start {
        Some(start) => (fm, &text[start..]),
        // Unterminated frontmatter: treat the whole thing as body.
        None => (Frontmatter::default(), text),
    }
}

/// Render a note back to its on-disk form.
pub fn serialize_document(note: &Note) -> String {
    let mut out = String::with_capacity(note.body.len() + 128);
    out.push_str("---\n");
    out.push_str("id: ");
    out.push_str(&note.id);
    out.push('\n');
    out.push_str("created: ");
    out.push_str(&note.created.to_rfc3339_opts(SecondsFormat::Secs, true));
    out.push('\n');
    if note.pinned {
        out.push_str("pinned: true\n");
    }
    for line in &note.extra_frontmatter {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(&note.body);
    if !note.body.is_empty() && !note.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The title is the first non-blank line, with heading markers and light
/// inline formatting stripped. Falls back to [`UNTITLED`].
pub fn derive_title(body: &str) -> String {
    // The first line that still says something once markup (and any
    // image, with its `{…}` attributes) is stripped.
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_tag_only_line(l))
        .map(|l| strip_inline_markup(l.trim_start_matches('#').trim()))
        .map(|l| {
            l.trim()
                .chars()
                .take(200)
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| UNTITLED.to_owned())
}

/// The body of a duplicate: the same note with " copy" after its title,
/// so the two can be told apart in the list and get different filenames.
/// A note with no title line, or one whose title is a picture, is copied
/// as it is.
pub fn duplicate_body(body: &str) -> String {
    let mut done = false;
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        let is_title = !done
            && !t.is_empty()
            && !is_tag_only_line(t)
            && !t.trim_start_matches('#').trim().is_empty();
        if is_title {
            done = true;
            if crate::images::parse_line(line).is_none() {
                out.push(format!("{} copy", line.trim_end()));
                continue;
            }
        }
        out.push(line.to_owned());
    }
    let mut copy = out.join("\n");
    if body.ends_with('\n') {
        copy.push('\n');
    }
    copy
}

/// A line made only of `#tags` (e.g. the line a folder pre-fills) is
/// metadata, not a title.
pub fn is_tag_only_line(line: &str) -> bool {
    let mut tokens = line.split_whitespace().peekable();
    tokens.peek().is_some()
        && tokens.all(|t| t.starts_with('#') && t.len() > 1 && !t.starts_with("# "))
}

/// True when the body carries no real content — empty, whitespace, or tag-only lines.
pub fn is_blank(body: &str) -> bool {
    body.lines()
        .map(str::trim)
        .all(|l| l.trim_start_matches('#').trim().is_empty() || is_tag_only_line(l))
}

/// Normalise a user-typed folder/tag name into tag form: lowercase, spaces to
/// dashes, no leading `#`, only tag characters. Returns None if nothing usable is left.
pub fn normalize_tag(name: &str) -> Option<String> {
    let cleaned: String = name
        .trim()
        .trim_start_matches('#')
        .to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|&c| is_tag_char(c))
        .collect();
    let cleaned = cleaned
        .trim_matches(|c| matches!(c, '/' | '-' | '_'))
        .to_owned();
    (cleaned.chars().any(char::is_alphabetic) && !cleaned.contains("//")).then_some(cleaned)
}

/// Markdown-stripped preview of the body after the title line.
pub fn preview(body: &str) -> String {
    let lines = body.lines().map(str::trim).filter(|l| !l.is_empty());
    let mut out = String::new();
    // The first line with visible text is the title (see `derive_title`);
    // the preview starts after it.
    let mut title_seen = false;
    for line in lines {
        if line.starts_with("```")
            || line.starts_with("---")
            || line.starts_with("***")
            || is_tag_only_line(line)
            || crate::table::separator_line(line)
            || crate::table::size_comment(line)
        {
            continue;
        }
        // A table row reads as its cells, space-separated.
        let line = if crate::table::table_line(line) {
            line.trim_matches('|')
                .split('|')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        } else {
            line.to_owned()
        };
        let line = strip_block_markup(&line);
        let line = strip_inline_markup(line.as_str());
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !title_seen {
            title_seen = true;
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
        if out.chars().count() > PREVIEW_CHARS {
            break;
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > PREVIEW_CHARS {
        let mut cut: String = collapsed.chars().take(PREVIEW_CHARS).collect();
        cut = cut.trim_end().to_owned();
        cut.push('…');
        cut
    } else {
        collapsed
    }
}

/// The first `max` characters of `s`, cut back to a word boundary and
/// finished with an ellipsis when anything was dropped.
pub fn clip_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max).collect();
    let whole_word = s.chars().nth(max).is_some_and(char::is_whitespace);
    let cut = if whole_word {
        head.len()
    } else {
        head.rfind(char::is_whitespace)
            .filter(|&i| i > max / 2)
            .unwrap_or(head.len())
    };
    let mut out = head[..cut].trim_end().to_owned();
    out.push('…');
    out
}

fn strip_block_markup(line: &str) -> String {
    let mut s = line;
    loop {
        let before = s;
        s = s.trim_start_matches('#').trim_start();
        s = s.trim_start_matches('>').trim_start();
        if let Some(r) = s
            .strip_prefix("- ")
            .or_else(|| s.strip_prefix("* "))
            .or_else(|| s.strip_prefix("+ "))
        {
            s = r.trim_start();
        }
        if let Some((len, _)) = task_box(s) {
            s = &s[len..];
        }
        // Ordered list "12. "
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        if digits > 0
            && let Some(r) = s[digits..]
                .strip_prefix(". ")
                .or_else(|| s[digits..].strip_prefix(") "))
        {
            s = r;
        }
        if s == before {
            break;
        }
    }
    s.to_owned()
}

/// Remove inline emphasis/code/link syntax, keeping the visible text.
fn strip_inline_markup(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '!' if chars.get(i + 1) == Some(&'[') => {
                // Image: drop entirely, along with its `{frame=… w=…}` attributes.
                if let Some(end) = find_link_end(&chars, i + 1) {
                    i = end + 1;
                    if chars.get(i) == Some(&'{')
                        && let Some(close) = chars[i..].iter().position(|&x| x == '}')
                    {
                        i += close + 1;
                    }
                    continue;
                }
                out.push(c);
            }
            '[' if chars.get(i + 1) == Some(&'[') => {
                // Wiki link: keep the target/alias text.
                if let Some(close) = find_seq(&chars, i + 2, &[']', ']']) {
                    let inner: String = chars[i + 2..close].iter().collect();
                    let shown = inner.split('|').next_back().unwrap_or("").trim();
                    out.push_str(shown);
                    i = close + 2;
                    continue;
                }
                out.push(c);
            }
            '[' => {
                // Markdown link: keep the label.
                if let Some(close) = chars[i + 1..]
                    .iter()
                    .position(|&x| x == ']')
                    .map(|p| p + i + 1)
                    && chars.get(close + 1) == Some(&'(')
                    && let Some(end) = chars[close + 2..]
                        .iter()
                        .position(|&x| x == ')')
                        .map(|p| p + close + 2)
                {
                    let label: String = chars[i + 1..close].iter().collect();
                    out.push_str(&label);
                    i = end + 1;
                    continue;
                }
                out.push(c);
            }
            '*' | '_' | '`' | '~' => {}
            // `==mark==` highlights: drop the doubled markers, keep lone `=`.
            '=' if chars.get(i + 1) == Some(&'=') => {
                i += 2;
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn find_seq(chars: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..chars.len().saturating_sub(seq.len() - 1)).find(|&i| chars[i..i + seq.len()] == *seq)
}

/// For `[label](target)` starting at `chars[start] == '['`, return the index of the closing `)`.
fn find_link_end(chars: &[char], start: usize) -> Option<usize> {
    let close = chars[start..].iter().position(|&x| x == ']')? + start;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    chars[close + 2..]
        .iter()
        .position(|&x| x == ')')
        .map(|p| p + close + 2)
}

/// Extract `#tag` and `#nested/tag` tags from the body.
///
/// Rules (matching common `#tag` conventions): a tag starts with `#` at the
/// start of a line or after whitespace / an opening bracket, continues with
/// letters, digits, `_`, `-`, `/`, must contain at least one letter, and is
/// ignored inside fenced code blocks and inline code spans. `# Heading`
/// (hash followed by space) is not a tag. Tags are lowercased.
pub fn extract_tags(body: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for (_, _, tag) in tag_spans(body) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// Every tag occurrence in `body`: byte range of the tag text (after the
/// `#`, trimmed of stray `/ - _`) and its normalised name.
fn tag_spans(body: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut in_code = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i].1;
            if c == '`' {
                in_code = !in_code;
                i += 1;
                continue;
            }
            if in_code || c != '#' {
                i += 1;
                continue;
            }
            let boundary_ok = i == 0
                || chars[i - 1].1.is_whitespace()
                || matches!(chars[i - 1].1, '(' | '[' | '{' | ',' | ';');
            if !boundary_ok {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && is_tag_char(chars[j].1) {
                j += 1;
            }
            // Trim stray separators off both ends, keeping byte positions.
            let stray = |c: char| matches!(c, '/' | '-' | '_');
            let mut from = i + 1;
            let mut to = j;
            while from < to && stray(chars[from].1) {
                from += 1;
            }
            while to > from && stray(chars[to - 1].1) {
                to -= 1;
            }
            if from < to {
                let raw: String = chars[from..to].iter().map(|(_, c)| *c).collect();
                let tag = raw.to_lowercase();
                if tag.chars().any(char::is_alphabetic) && !tag.contains("//") {
                    let start = line_start + chars[from].0;
                    let end = line_start + chars[to - 1].0 + chars[to - 1].1.len_utf8();
                    out.push((start, end, tag));
                }
            }
            i = j.max(i + 1);
        }
    }
    out
}

/// Point every `](old)` link or picture target in `body` at `new` — an
/// asset changed its name. Only whole targets match (`assets/pic.jpg` leaves
/// `assets/pic.jpg.bak` alone). `None` when nothing matched.
pub fn repoint_asset(body: &str, old: &str, new: &str) -> Option<String> {
    let needle = format!("]({old}");
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    let mut changed = false;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        let whole = after.starts_with([')', ' ', '\t']);
        out.push_str(&rest[..at]);
        if whole {
            out.push_str("](");
            out.push_str(new);
            changed = true;
        } else {
            out.push_str(&needle);
        }
        rest = after;
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// Rewrite every `#old` and `#old/…` in `body` as `#new` / `#new/…`.
/// `old` and `new` are normalised tags. `None` when nothing matched.
pub fn rename_tag(body: &str, old: &str, new: &str) -> Option<String> {
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    let mut changed = false;
    for (start, end, tag) in tag_spans(body) {
        let rest = if tag == old {
            Some("")
        } else if tag.len() > old.len() && tag.starts_with(old) && tag[old.len()..].starts_with('/')
        {
            Some(&tag[old.len()..])
        } else {
            None
        };
        let Some(rest) = rest else { continue };
        // Keep the user's own casing for the part that isn't renamed.
        let original = &body[start..end];
        let kept = original
            .char_indices()
            .nth(old.chars().count())
            .map_or("", |(i, _)| &original[i..]);
        let _ = rest;
        out.push_str(&body[last..start]);
        out.push_str(new);
        out.push_str(kept);
        last = end;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&body[last..]);
    Some(out)
}

pub use crate::markdown::{list_marker, numbered_marker, task_box};

/// What Enter should do on a list line: the byte length of the indent +
/// marker (+ task box) that opens `line`, and the prefix the next line
/// should start with (`1. ` becomes `2. `, a ticked box comes back open).
/// `None` when the line is not a list item.
pub fn list_continuation(line: &str) -> Option<(usize, String)> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let body = &line[indent_len..];
    if let Some(n) = list_marker(body) {
        let marker = &body[..n];
        if let Some((box_len, _)) = task_box(&body[n..]) {
            // `- [x]` at the very end of the line has no trailing space yet.
            let len = n + box_len;
            return Some((indent_len + len, format!("{indent}{marker}[ ] ")));
        }
        return Some((indent_len + n, format!("{indent}{marker}")));
    }
    let (n, number) = numbered_marker(body)?;
    let punct = &body[n - 2..n - 1];
    Some((
        indent_len + n,
        format!("{indent}{}{punct} ", number.saturating_add(1)),
    ))
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// Extract `[[wiki link]]` targets (alias syntax `[[target|shown]]` yields `target`).
pub fn extract_links(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        if !inner.contains('\n') {
            let target = inner.split('|').next().unwrap_or("").trim();
            if !target.is_empty()
                && !links
                    .iter()
                    .any(|l: &String| l.eq_ignore_ascii_case(target))
            {
                links.push(target.to_owned());
            }
        }
        rest = &after[end + 2..];
    }
    links
}

/// Turn a title into a filesystem-safe filename stem.
pub fn slug_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == ' ' || c == '-');
    let capped: String = trimmed.chars().take(MAX_FILENAME_CHARS).collect();
    let capped = capped.trim_end().to_owned();
    if capped.is_empty() {
        UNTITLED.to_owned()
    } else {
        capped
    }
}

/// Content hash of a full document, hex-encoded.
pub fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_is_the_same_note_with_copy_after_its_title() {
        assert_eq!(
            duplicate_body("# Shopping\n\n#home\n- milk\n"),
            "# Shopping copy\n\n#home\n- milk\n"
        );
        // The title is the first line that says something: not a blank
        // line, not the tag line a folder pre-fills.
        assert_eq!(
            duplicate_body("\n#work\nPlan for Monday\nmore"),
            "\n#work\nPlan for Monday copy\nmore"
        );
        assert_eq!(derive_title(&duplicate_body("# Shopping\n")), "Shopping copy");
        // Nothing to rename: copied as it is.
        assert_eq!(duplicate_body("# \n\n#home\n"), "# \n\n#home\n");
        assert_eq!(
            duplicate_body("![p](assets/p.png)\ntext\n"),
            "![p](assets/p.png)\ntext\n"
        );
    }

    #[test]
    fn repointing_an_asset_touches_whole_targets_only() {
        let body = "![a](assets/pic.jpg){w=240}\n[pic](assets/pic.jpg)\n![b](assets/pic.jpg.bak)\n![c](assets/pic.jpg \"t\")\n";
        assert_eq!(
            repoint_asset(body, "assets/pic.jpg", "assets/pic-2.jpg").unwrap(),
            "![a](assets/pic-2.jpg){w=240}\n[pic](assets/pic-2.jpg)\n![b](assets/pic.jpg.bak)\n![c](assets/pic-2.jpg \"t\")\n"
        );
        assert!(repoint_asset("no pictures", "assets/pic.jpg", "assets/x.jpg").is_none());
    }

    #[test]
    fn clip_words_ends_on_a_word() {
        assert_eq!(clip_words("short one", 20), "short one");
        assert_eq!(
            clip_words("the quick brown fox jumps", 15),
            "the quick brown…"
        );
        assert_eq!(clip_words("the quick brown fox jumps", 14), "the quick…");
        assert_eq!(clip_words("supercalifragilistic word", 10), "supercalif…");
    }

    #[test]
    fn list_continuation_knows_bullets_numbers_and_boxes() {
        assert_eq!(list_continuation("- milk"), Some((2, "- ".into())));
        assert_eq!(list_continuation("* milk"), Some((2, "* ".into())));
        assert_eq!(list_continuation("  - milk"), Some((4, "  - ".into())));
        assert_eq!(list_continuation("- [ ] milk"), Some((6, "- [ ] ".into())));
        assert_eq!(list_continuation("- [x] milk"), Some((6, "- [ ] ".into())));
        assert_eq!(list_continuation("- [🦆] milk"), Some((9, "- [ ] ".into())));
        assert_eq!(list_continuation("- [ ]"), Some((5, "- [ ] ".into())));
        assert_eq!(list_continuation("3. eggs"), Some((3, "4. ".into())));
        assert_eq!(list_continuation("  12) eggs"), Some((6, "  13) ".into())));
        assert_eq!(list_continuation("eggs"), None);
        assert_eq!(list_continuation("-milk"), None);
        assert_eq!(list_continuation("1.5 litres"), None);
        assert_eq!(list_continuation("---"), None);
    }

    #[test]
    fn roundtrip_document() {
        let note = Note {
            id: "abc".into(),
            title: "Hello".into(),
            body: "# Hello\n\nWorld".into(),
            created: DateTime::parse_from_rfc3339("2026-08-29T01:02:03Z")
                .unwrap()
                .into(),
            modified: Utc::now(),
            pinned: true,
            trashed: false,
            extra_frontmatter: vec!["colour: red".into()],
            path: PathBuf::new(),
        };
        let text = serialize_document(&note);
        let (fm, body) = parse_document(&text);
        assert_eq!(fm.id.as_deref(), Some("abc"));
        assert_eq!(fm.created, Some(note.created));
        assert!(fm.pinned);
        assert_eq!(fm.extra, vec!["colour: red".to_string()]);
        assert_eq!(body, "# Hello\n\nWorld\n");
    }

    #[test]
    fn no_frontmatter_is_all_body() {
        let (fm, body) = parse_document("# Just a note\n");
        assert_eq!(fm, Frontmatter::default());
        assert_eq!(body, "# Just a note\n");
    }

    #[test]
    fn unterminated_frontmatter_is_body() {
        let (fm, body) = parse_document("---\nid: x\nno end");
        assert_eq!(fm.id, None);
        assert_eq!(body, "---\nid: x\nno end");
    }

    #[test]
    fn title_derivation() {
        assert_eq!(derive_title("\n\n## **Bold** title  \nbody"), "Bold title");
        assert_eq!(derive_title(""), UNTITLED);
        assert_eq!(derive_title("   \n#\n"), UNTITLED);
        assert_eq!(derive_title("plain first line"), "plain first line");
    }

    #[test]
    fn tag_only_lines_are_not_titles() {
        assert_eq!(derive_title("#work/incab\n\nReal title"), "Real title");
        assert_eq!(derive_title("#work #home"), UNTITLED);
        assert!(is_blank("\n\n#work\n  "));
        assert!(!is_blank("#work\nhello"));
        assert_eq!(preview("Title\n\n#work #home\nbody text"), "body text");
        assert_eq!(
            normalize_tag("  #Work Stuff/Sub "),
            Some("work-stuff/sub".into())
        );
        assert_eq!(normalize_tag("123"), None);
        assert_eq!(normalize_tag("##"), None);
    }

    #[test]
    fn images_and_their_attributes_never_reach_title_or_preview() {
        let body = "![cover](assets/c.png){frame=film w=458}\nKyoto\n\nWalking ![pic](a.png){frame=comic} past the shop.\n![x](y.png)\n";
        assert_eq!(derive_title(body), "Kyoto");
        assert_eq!(preview(body), "Walking past the shop.");
        assert_eq!(derive_title("![only](a.png){frame=tint}\n"), UNTITLED);
    }

    #[test]
    fn preview_strips_markdown() {
        let body = "# Title\n\n- **item** one\n> quote `code` [link](http://x) ![img](y) [[Other|shown]]\n";
        assert_eq!(preview(body), "item one quote code link shown");
    }

    #[test]
    fn tags() {
        let body = "# Heading not a tag\nhello #work/incab and #Work, (#Idea) #123 `#code` #trailing/\n```\n#fenced\n```\nemail#nottag";
        assert_eq!(
            extract_tags(body),
            vec!["work/incab", "work", "idea", "trailing"]
        );
    }

    #[test]
    fn task_boxes_of_any_mark() {
        assert_eq!(task_box("[ ] milk"), Some((4, false)));
        assert_eq!(task_box("[x] milk"), Some((4, true)));
        assert_eq!(task_box("[✓] milk"), Some((3 + '✓'.len_utf8(), true)));
        assert_eq!(task_box("[🦆]"), Some((2 + '🦆'.len_utf8(), true)));
        assert_eq!(task_box("[]"), None);
        assert_eq!(task_box("[x]milk"), None);
        assert_eq!(task_box("[a b] c"), None);
        assert_eq!(preview("T\n- [🦆] ducks\n- [ ] milk\n"), "ducks milk");
    }

    #[test]
    fn rename_tag_rewrites_tag_and_subtags_only() {
        let body = "Trip #Travels and #travels/japan, not #travelsx or `#travels` here\n```\n#travels\n```\n#todo\n";
        let out = rename_tag(body, "travels", "journeys").unwrap();
        assert_eq!(
            out,
            "Trip #journeys and #journeys/japan, not #travelsx or `#travels` here\n```\n#travels\n```\n#todo\n"
        );
        assert_eq!(
            rename_tag(body, "todo", "done").unwrap(),
            body.replace("#todo", "#done")
        );
        assert!(rename_tag(body, "nothing", "x").is_none());
        // Renaming a sub-tag leaves the parent alone.
        assert_eq!(
            rename_tag("#travels #travels/japan", "travels/japan", "travels/kyoto").unwrap(),
            "#travels #travels/kyoto"
        );
        assert_eq!(
            extract_tags(&out),
            vec!["journeys", "journeys/japan", "travelsx", "todo"]
        );
    }

    #[test]
    fn links() {
        assert_eq!(
            extract_links("see [[Alpha]] and [[beta|B]] and [[alpha]] [[]]"),
            vec!["Alpha", "beta"]
        );
    }

    #[test]
    fn slugs() {
        assert_eq!(slug_filename("A/B: C?"), "A-B- C");
        assert_eq!(slug_filename("  ...  "), UNTITLED);
        assert_eq!(slug_filename("ünïcödé ok"), "ünïcödé ok");
    }
}
