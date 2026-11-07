// SPDX-License-Identifier: GPL-3.0-only

//! Debug-only driver: `JJB_SCRIPT="new;type:Hello #tag;wait:1500;exit"` feeds
//! the app the same messages real input would, so end-to-end behaviour can be
//! exercised (and captured with `tools/xshot.py`) without a human at the
//! keyboard. Steps are separated by `;`; `\n` and `\;` are unescaped in text.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Create a new note (Ctrl+N).
    New,
    /// Insert text into the editor at the cursor.
    Type(String),
    /// Type into the search box.
    Search(String),
    /// Select the n-th note in the current list (0-based).
    Select(usize),
    /// Toggle pin on the current note.
    Pin,
    /// Move the current note to the trash.
    Trash,
    /// Create a folder (tag) and switch to it.
    Folder(String),
    /// Apply a dock format action by key (bold, italic, code, h1, h2, bullet, todo, link, tag, rule).
    Format(String),
    /// Select all text in the editor.
    SelectAll,
    /// Toggle the dock's `+` section.
    Dock,
    /// Undo / redo one step.
    Undo,
    Redo,
    /// Toggle the shortcuts overlay.
    Shortcuts,
    /// Click the save tick.
    SavedInfo,
    /// Open the theme picker drawer.
    Themes,
    /// Toggle editor-only layout.
    Solo,
    /// Import an image file into the note.
    Image(String),
    /// Open the image picker (portal dialog).
    Pick,
    /// Point the picker at a folder: `pickdir:/path`.
    PickDir(String),
    /// Keep notes in this folder from now on: `notesdir:/path`.
    NotesDir(String),
    /// Set the frame style of the n-th image: `imgframe:n:key`.
    ImgFrame(usize, String),
    /// Set alignment of the n-th image: `imgalign:n:left|center|right`.
    ImgAlign(usize, String),
    /// Set width in px of the n-th image (0 = full): `imgwidth:n:px`.
    ImgWidth(usize, u32),
    /// Open the ⋯ menu of the n-th image.
    ImgMenu(usize),
    /// Show the n-th image mid-drag with the drop line before body line `line`: `imgdrag:n:line`.
    ImgDrag(usize, usize),
    /// Start dragging the n-th link card with the drop slot before `line`.
    LinkDrag(usize, usize),
    /// Start dragging the sidebar root entry `n` with the drop slot before
    /// entry `slot` (drop it for tagmove).
    TagDrag(usize, usize),
    TagMove(usize, usize),
    /// Append a spacer line to the tag list.
    AddSpace,
    /// Set the editor body weight (200/300/400/500).
    Weight(u16),
    /// Point the font picker at a pane: tags, notes or editor.
    FontFor(String),
    /// Quit through the File menu's path (flush, close the window).
    Quit,
    /// Sign in to a sync server (`sync:url,email,password`), creating the
    /// account when `,new` is appended.
    Sync(String, String, String, bool),
    /// Run a sync cycle now.
    SyncNow,
    /// Move the n-th image to sit before body line `line`: `imgmove:n:line`.
    ImgMove(usize, usize),
    /// Set the caption of the n-th image: `imgcaption:n:text`.
    ImgCaption(usize, String),
    /// Switch theme by key.
    Theme(String),
    /// Colour buffet, dark side: `buffet:highlight,dark`.
    Buffet(String, String),
    /// Select `sel:line,col,line2,col2` in the focused editor (byte cols);
    /// with two args just places the caret.
    Sel(usize, usize, Option<(usize, usize)>),
    /// The highlighter pen over `sweep:line,col,line2,col2`: what a
    /// middle-button drag between those two points does.
    Sweep(usize, usize, usize, usize),
    /// Set a cell of the first table block: `cell:row,col,text`.
    Cell(usize, usize, String),
    /// Open a cell for editing (and leave it open): `editcell:row,col`.
    EditCell(usize, usize),
    /// Formula pointing: `fpick:r,c` clicks a cell; `fpick:r,c,r2,c2`
    /// sweeps a range (left active for capture); `pickdone` releases.
    Pick2(usize, usize, Option<(usize, usize)>),
    /// Extend the active pick to a cell (its own step, so renders happen
    /// between sweep points like a real drag): `fpickover:r,c`.
    PickOver2(usize, usize),
    PickDone,
    /// Set the open cell's draft text: `draft:=SUM(`.
    Draft(String),
    /// Drag the fill handle from one cell to another: `fill:r,c,r2,c2`.
    Fill(usize, usize, usize, usize),
    /// Rubber-band select cells (left active): `tsel:r,c,r2,c2`.
    TSel(usize, usize, usize, usize),
    /// Colour buffet, light side: `buffet:highlight,paper,ink`.
    BuffetLight(String, String, String),
    /// Fold / unfold the sub-tags of a tag: `fold:travels`.
    Fold(String),
    /// Editor font by key: `font:plex`.
    Font(String),
    /// Designer font pairing by key: `pairing:editorial`.
    Pairing(String),
    /// Step a pane's text size: `size:editor:+3`, `size:sidebar:-1`, `size:list:+2`.
    Size(String, i16),
    /// Dock size by key: `docksize:wow`.
    DockSize(String),
    /// Fold / unfold an Appearance section: `section:colour|font|size`.
    Section(String),
    /// Attach a file as a card: `attach:/path/to/file.pdf`.
    Attach(String),
    /// Open the right-click menu of a tag: `tagmenu:travels`.
    TagMenu(String),
    /// Set the finished-task mark: `marker:🦆`.
    Marker(String),
    /// Set the text column width: `measure:narrow|medium|wide|full`.
    Measure(String),
    /// Follow a wiki link by title, as Ctrl+click would: `follow:Kyoto!`.
    Follow(String),
    /// Launcher icon: a theme key or `follow`: `icon:amber`.
    Icon(String),
    /// Toggle the neon coffee sign.
    Coffee,
    /// Folder icon style: `iconset:boxicons|iconoir`.
    IconSet(String),
    /// Give a tag an 8-bit icon (or `none`): `tagicon:travels:plane`.
    TagIcon(String, String),
    /// Walk the notes list like ↑/↓: `nav:+1` / `nav:-1`.
    Nav(i32),
    /// Toggle the task box at line:column of the focused block: `togglebox:2:3`.
    ToggleBox(usize, usize),
    /// Rename a tag everywhere: `renametag:old:new`.
    RenameTag(String, String),
    /// Press a named key in the focused editor, optionally several times:
    /// `key:backspace`, `key:left:3` (backspace, delete, enter, tab,
    /// left, right, up, down, home, end).
    Key(String, usize),
    /// Pause for the given milliseconds (lets autosave run).
    Wait(u64),
    /// Flush and quit.
    Exit,
}

pub fn parse(script: &str) -> Vec<Step> {
    split_steps(script)
        .into_iter()
        .filter_map(|raw| {
            // Leading trim only: a `type:` arg may end in a real space.
            let raw = raw.trim_start();
            if raw.is_empty() {
                return None;
            }
            let (cmd, arg) = raw
                .split_once(':')
                .map_or((raw.trim_end(), ""), |(c, a)| (c.trim(), a));
            let step = match cmd {
                "new" => Step::New,
                "type" => Step::Type(unescape(arg)),
                "search" => Step::Search(unescape(arg)),
                "select" => Step::Select(arg.trim().parse().ok()?),
                "pin" => Step::Pin,
                "trash" => Step::Trash,
                "folder" => Step::Folder(unescape(arg)),
                "format" => Step::Format(arg.trim().to_owned()),
                "selectall" => Step::SelectAll,
                "dock" => Step::Dock,
                "undo" => Step::Undo,
                "shortcuts" => Step::Shortcuts,
                "savedinfo" => Step::SavedInfo,
                "redo" => Step::Redo,
                "themes" => Step::Themes,
                "solo" => Step::Solo,
                "image" => Step::Image(unescape(arg)),
                "pick" => Step::Pick,
                "pickdir" => Step::PickDir(unescape(arg)),
                "notesdir" => Step::NotesDir(unescape(arg)),
                "imgframe" => {
                    let (n, key) = arg.trim().split_once(':')?;
                    Step::ImgFrame(n.parse().ok()?, key.to_owned())
                }
                "imgalign" => {
                    let (n, key) = arg.trim().split_once(':')?;
                    Step::ImgAlign(n.parse().ok()?, key.to_owned())
                }
                "imgwidth" => {
                    let (n, w) = arg.trim().split_once(':')?;
                    Step::ImgWidth(n.parse().ok()?, w.parse().ok()?)
                }
                "imgmenu" => Step::ImgMenu(arg.trim().parse().ok()?),
                "imgdrag" => {
                    let (n, line) = arg.trim().split_once(':')?;
                    Step::ImgDrag(n.parse().ok()?, line.parse().ok()?)
                }
                "linkdrag" => {
                    let (n, line) = arg.trim().split_once(':')?;
                    Step::LinkDrag(n.parse().ok()?, line.parse().ok()?)
                }
                "imgmove" => {
                    let (n, line) = arg.trim().split_once(':')?;
                    Step::ImgMove(n.parse().ok()?, line.parse().ok()?)
                }
                "imgcaption" => {
                    let (n, text) = arg.split_once(':')?;
                    Step::ImgCaption(n.trim().parse().ok()?, unescape(text))
                }
                "theme" => Step::Theme(arg.trim().to_owned()),
                "fpick" => {
                    let nums: Vec<usize> = arg
                        .trim()
                        .split(',')
                        .map(|n| n.trim().parse())
                        .collect::<Result<_, _>>()
                        .ok()?;
                    match nums.as_slice() {
                        [r, c] => Step::Pick2(*r, *c, None),
                        [r, c, r2, c2] => Step::Pick2(*r, *c, Some((*r2, *c2))),
                        _ => return None,
                    }
                }
                "fpickover" => {
                    let (r, c) = arg.trim().split_once(',')?;
                    Step::PickOver2(r.trim().parse().ok()?, c.trim().parse().ok()?)
                }
                "pickdone" => Step::PickDone,
                "draft" => Step::Draft(unescape(arg)),
                "tsel" => {
                    let nums: Vec<usize> = arg
                        .trim()
                        .split(',')
                        .map(|n| n.trim().parse())
                        .collect::<Result<_, _>>()
                        .ok()?;
                    match nums.as_slice() {
                        [r, c, r2, c2] => Step::TSel(*r, *c, *r2, *c2),
                        _ => return None,
                    }
                }
                "fill" => {
                    let nums: Vec<usize> = arg
                        .trim()
                        .split(',')
                        .map(|n| n.trim().parse())
                        .collect::<Result<_, _>>()
                        .ok()?;
                    match nums.as_slice() {
                        [r, c, r2, c2] => Step::Fill(*r, *c, *r2, *c2),
                        _ => return None,
                    }
                }
                "editcell" => {
                    let (r, c) = arg.trim().split_once(',')?;
                    Step::EditCell(r.trim().parse().ok()?, c.trim().parse().ok()?)
                }
                "cell" => {
                    let (r, rest) = arg.trim_start().split_once(',')?;
                    let (c, text) = rest.split_once(',')?;
                    Step::Cell(
                        r.trim().parse().ok()?,
                        c.trim().parse().ok()?,
                        unescape(text),
                    )
                }
                "sel" => {
                    let nums: Vec<usize> = arg
                        .trim()
                        .split(',')
                        .map(|n| n.trim().parse())
                        .collect::<Result<_, _>>()
                        .ok()?;
                    match nums.as_slice() {
                        [l, c] => Step::Sel(*l, *c, None),
                        [l, c, l2, c2] => Step::Sel(*l, *c, Some((*l2, *c2))),
                        _ => return None,
                    }
                }
                "sweep" => {
                    let nums: Vec<usize> = arg
                        .trim()
                        .split(',')
                        .map(|n| n.trim().parse())
                        .collect::<Result<_, _>>()
                        .ok()?;
                    match nums.as_slice() {
                        [l, c, l2, c2] => Step::Sweep(*l, *c, *l2, *c2),
                        _ => return None,
                    }
                }
                "buffet" => {
                    let parts: Vec<&str> = arg.trim().split(',').map(str::trim).collect();
                    match parts.as_slice() {
                        [h, d] => Step::Buffet((*h).to_owned(), (*d).to_owned()),
                        [h, p, i] => Step::BuffetLight(
                            (*h).to_owned(),
                            (*p).to_owned(),
                            (*i).to_owned(),
                        ),
                        _ => return None,
                    }
                }
                "fold" => Step::Fold(arg.trim().to_owned()),
                "font" => Step::Font(arg.trim().to_owned()),
                "pairing" => Step::Pairing(arg.trim().to_owned()),
                "size" => {
                    let (pane, delta) = arg.trim().split_once(':')?;
                    Step::Size(
                        pane.to_owned(),
                        delta.trim().trim_start_matches('+').parse().ok()?,
                    )
                }
                "docksize" => Step::DockSize(arg.trim().to_owned()),
                "section" => Step::Section(arg.trim().to_owned()),
                "attach" => Step::Attach(arg.trim().to_owned()),
                "tagmenu" => Step::TagMenu(arg.trim().to_owned()),
                "nav" => Step::Nav(arg.trim().trim_start_matches('+').parse().ok()?),
                "marker" => Step::Marker(arg.trim().to_owned()),
                "measure" => Step::Measure(arg.trim().to_owned()),
                "follow" => Step::Follow(unescape(arg)),
                "icon" => Step::Icon(arg.trim().to_owned()),
                "coffee" => Step::Coffee,
                "iconset" => Step::IconSet(arg.trim().to_owned()),
                "tagicon" => {
                    let (tag, key) = arg.trim().split_once(':')?;
                    Step::TagIcon(tag.to_owned(), key.to_owned())
                }
                "togglebox" => {
                    let (l, c) = arg.trim().split_once(':')?;
                    Step::ToggleBox(l.parse().ok()?, c.parse().ok()?)
                }
                "renametag" => {
                    let (old, new) = arg.trim().split_once(':')?;
                    Step::RenameTag(old.to_owned(), new.to_owned())
                }
                "tagdrag" => {
                    let (n, slot) = arg.trim().split_once(':')?;
                    Step::TagDrag(n.parse().ok()?, slot.parse().ok()?)
                }
                "tagmove" => {
                    let (n, slot) = arg.trim().split_once(':')?;
                    Step::TagMove(n.parse().ok()?, slot.parse().ok()?)
                }
                "addspace" => Step::AddSpace,
                "weight" => Step::Weight(arg.trim().parse().ok()?),
                "fontfor" => Step::FontFor(arg.trim().to_owned()),
                "quit" => Step::Quit,
                "sync" => {
                    let parts: Vec<&str> = arg.trim().split(',').map(str::trim).collect();
                    match parts.as_slice() {
                        [u, e, p] => Step::Sync((*u).to_owned(), (*e).to_owned(), (*p).to_owned(), false),
                        [u, e, p, "new"] => Step::Sync((*u).to_owned(), (*e).to_owned(), (*p).to_owned(), true),
                        _ => return None,
                    }
                }
                "syncnow" => Step::SyncNow,
                "wait" => Step::Wait(arg.trim().parse().ok()?),
                "key" => {
                    let arg = arg.trim();
                    let (name, times) = arg
                        .split_once(':')
                        .map_or((arg, 1), |(n, t)| (n, t.trim().parse().unwrap_or(1)));
                    Step::Key(name.to_owned(), times)
                }
                "exit" => Step::Exit,
                other => {
                    tracing::warn!(step = other, "unknown JJB_SCRIPT step");
                    return None;
                }
            };
            Some(step)
        })
        .collect()
}

/// Split on `;` but keep `\;` intact.
fn split_steps(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&';') {
            cur.push_str("\\;");
            chars.next();
        } else if c == ';' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\;", ";")
}

/// Runner state kept in the app model.
#[derive(Debug)]
pub struct Runner {
    steps: std::collections::VecDeque<Step>,
    resume_at: Instant,
}

impl Runner {
    pub fn from_env() -> Option<Self> {
        let script = std::env::var("JJB_SCRIPT").ok()?;
        let steps = parse(&script);
        tracing::info!(count = steps.len(), "JJB_SCRIPT loaded");
        Some(Self {
            steps: steps.into(),
            // Give the window a moment to appear before driving it.
            resume_at: Instant::now() + Duration::from_millis(1200),
        })
    }

    /// Re-queue a step to run on the next tick (used to split multi-line typing).
    pub fn push_front(&mut self, step: Step) {
        self.steps.push_front(step);
    }

    pub fn is_active(&self) -> bool {
        !self.steps.is_empty()
    }

    /// The next step, if its time has come.
    pub fn next(&mut self) -> Option<Step> {
        if Instant::now() < self.resume_at {
            return None;
        }
        let step = self.steps.pop_front()?;
        if let Step::Wait(ms) = step {
            self.resume_at = Instant::now() + Duration::from_millis(ms);
        }
        Some(step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steps() {
        let steps = parse(
            "new; type:Hello\\nworld\\; ok ;wait:500;select:2;search:milk;pin;trash;bogus;exit",
        );
        assert_eq!(
            steps,
            vec![
                Step::New,
                // The trailing space survives: typed text is verbatim.
                Step::Type("Hello\nworld; ok ".into()),
                Step::Wait(500),
                Step::Select(2),
                Step::Search("milk".into()),
                Step::Pin,
                Step::Trash,
                Step::Exit,
            ]
        );
    }
}
