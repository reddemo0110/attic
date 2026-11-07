// SPDX-License-Identifier: GPL-3.0-only

//! The store ties the notes directory (source of truth) to the SQLite index.

// Some of the API is ahead of the UI (wiki-link resolution, settings); keep it.
#![allow(dead_code)]

mod db;
mod fs;
mod sync;

pub use db::View;
pub use sync::Applied;

use crate::note::{self, Note, NoteSummary};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use db::Db;
use fs::NotesDir;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Store {
    dir: NotesDir,
    db: Db,
    device_id: String,
    /// Tags the user created as "folders" before any note carried them.
    /// Persisted in `<notes dir>/.folders`, one per line, so they survive
    /// an index rebuild.
    folders: Vec<String>,
}

/// A read-only view of the index for processes that are not the app: the
/// GNOME Shell search provider answers overview searches from it without
/// taking the notes directory or racing the app's writes. It reflects the
/// last time the app ran, which is the nature of a derived index.
pub struct Index {
    db: Db,
}

impl Index {
    /// `None` when there is no index yet (the app has never run) or it is
    /// from another schema version.
    pub fn open(index_path: &Path) -> Result<Option<Self>> {
        Ok(Db::open_read_only(index_path)?.map(|db| Self { db }))
    }

    /// Full-text search across the notes that are not in the trash.
    pub fn search(&self, query: &str) -> Result<Vec<NoteSummary>> {
        self.db.search(query, &View::All)
    }

    /// Where the note with `id` lives on disk.
    pub fn path(&self, id: &str) -> Result<Option<PathBuf>> {
        Ok(self.db.get(id)?.map(|row| row.path))
    }
}

const FOLDERS_FILE: &str = ".folders";
/// Every install's welcome note shares this id (see `create_welcome_note`).
const WELCOME_ID: &str = "00000000-0000-7000-8000-000000000001";

impl Store {
    pub fn open(notes_dir: PathBuf, index_path: &Path, device_id: String) -> Result<Self> {
        let dir = NotesDir::open(notes_dir)?;
        let db = Db::open(index_path)?;
        let folders = read_folders(&dir.root().join(FOLDERS_FILE));
        let mut store = Self {
            dir,
            db,
            device_id,
            folders,
        };
        let changed = store.reindex()?;
        tracing::info!(notes = store.db.count()?, changed, dir = %store.dir.root().display(), "store opened");
        if store.db.count()? == 0 {
            store.create_welcome_note()?;
        }
        Ok(store)
    }

    pub fn notes_dir(&self) -> &Path {
        self.dir.root()
    }

    /// The note kept at `path`, if it is one of ours. Symlinks and `..` are
    /// resolved first so a path handed in from the shell matches the index.
    pub fn find_by_path(&self, path: &Path) -> Result<Option<String>> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        if let Some(id) = self.db.id_for_path(&canonical)? {
            return Ok(Some(id));
        }
        self.db.id_for_path(path)
    }

    /// Bring a markdown file from outside the notes folder in as a new note
    /// (a copy — the original is left alone) and return its id.
    pub fn import(&mut self, path: &Path) -> Result<Note> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let (fm, body) = note::parse_document(&text);
        let now = Utc::now();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);
        let title = {
            let derived = note::derive_title(body);
            if derived == note::UNTITLED {
                stem.clone().unwrap_or(derived)
            } else {
                derived
            }
        };
        let mut n = Note {
            // A fresh id: the copy is a different note from wherever the
            // file came from, even if that was another Attic folder.
            id: note::new_id(),
            title,
            body: body.to_owned(),
            created: fm.created.unwrap_or(now),
            modified: now,
            pinned: false,
            trashed: false,
            extra_frontmatter: fm.extra,
            path: PathBuf::new(),
        };
        let name = stem.as_deref().unwrap_or(&n.title);
        n.path = self.dir.unique_path(self.dir.root(), name, None);
        self.write(&mut n)?;
        Ok(n)
    }

    /// Bring the index in line with the files on disk. Returns how many
    /// notes were (re)indexed or dropped.
    pub fn reindex(&mut self) -> Result<usize> {
        let indexed = self.db.indexed_files()?;
        let mut seen = std::collections::HashSet::new();
        let mut changed = 0;
        for entry in self.dir.scan()? {
            seen.insert(entry.path.clone());
            let disk_modified: DateTime<Utc> = entry.modified.into();
            if let Some(known) = indexed.get(&entry.path) {
                // Cheap check first: unchanged mtime means unchanged content.
                if (known.modified - disk_modified).num_milliseconds().abs() < 1000 {
                    continue;
                }
                let text = self.dir.read(&entry.path)?;
                if note::content_hash(&text) == known.hash {
                    continue;
                }
            }
            match self.index_file(&entry.path, entry.trashed, entry.modified) {
                Ok(_) => changed += 1,
                Err(err) => {
                    tracing::warn!(path = %entry.path.display(), %err, "skipping unreadable note")
                }
            }
        }
        for (path, known) in &indexed {
            if !seen.contains(path) {
                // A note renamed elsewhere (another machine, through a
                // sync client) was just indexed under its new name with
                // the same id; only a note that is really gone goes.
                let moved = self.db.get(&known.id)?.is_some_and(|row| row.path != *path);
                if !moved {
                    self.db.remove(&known.id)?;
                }
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Re-read `.folders` after someone else wrote it (a sync client
    /// bringing another machine's list). Whether it differed.
    pub fn reload_folders(&mut self) -> bool {
        let fresh = read_folders(&self.dir.root().join(FOLDERS_FILE));
        if fresh == self.folders {
            return false;
        }
        self.folders = fresh;
        true
    }

    /// Parse a file and (re)write its index row. Files without an id get one
    /// stamped in — that's the only time we write a file we didn't author.
    fn index_file(&mut self, path: &Path, trashed: bool, modified: SystemTime) -> Result<Note> {
        let text = self.dir.read(path)?;
        let (fm, body) = note::parse_document(&text);
        let needs_stamp = fm.id.is_none() || fm.created.is_none();
        let modified: DateTime<Utc> = modified.into();
        let mut n = Note {
            id: fm.id.unwrap_or_else(note::new_id),
            title: note::derive_title(body),
            body: body.to_owned(),
            created: fm.created.unwrap_or(modified),
            modified,
            pinned: fm.pinned,
            trashed,
            extra_frontmatter: fm.extra,
            path: path.to_owned(),
        };
        let hash = if needs_stamp {
            let text = note::serialize_document(&n);
            self.dir.write_atomic(path, &text)?;
            n.modified = Utc::now();
            note::content_hash(&text)
        } else {
            note::content_hash(&text)
        };
        self.index(&n, &hash)?;
        Ok(n)
    }

    fn index(&mut self, n: &Note, hash: &str) -> Result<()> {
        let tags = note::extract_tags(&n.body);
        let links = note::extract_links(&n.body);
        let preview = note::preview(&n.body);
        self.db.upsert(n, &preview, hash, &tags, &links, &n.body)
    }

    pub fn list(&self, view: &View) -> Result<Vec<NoteSummary>> {
        self.db.list(view)
    }

    pub fn search(&self, query: &str, view: &View) -> Result<Vec<NoteSummary>> {
        self.db.search(query, view)
    }

    /// Tags carried by notes plus user-created folders (count 0 when empty).
    pub fn tags(&self) -> Result<Vec<(String, usize)>> {
        let mut tags = self.db.tags()?;
        for folder in &self.folders {
            if !tags.iter().any(|(t, _)| t == folder) {
                tags.push((folder.clone(), 0));
            }
        }
        tags.sort();
        Ok(tags)
    }

    pub fn folders(&self) -> &[String] {
        &self.folders
    }

    /// Create a folder (a tag with no notes yet). Returns the normalised name.
    pub fn add_folder(&mut self, name: &str) -> Result<Option<String>> {
        let Some(tag) = note::normalize_tag(name) else {
            return Ok(None);
        };
        if !self.folders.contains(&tag) {
            self.folders.push(tag.clone());
            self.folders.sort();
            self.write_folders()?;
        }
        Ok(Some(tag))
    }

    pub fn remove_folder(&mut self, name: &str) -> Result<()> {
        self.folders.retain(|f| f != name);
        self.write_folders()
    }

    /// Rename a tag — and every sub-tag beneath it — in all notes (trash
    /// included) and in the folder list. Returns how many notes changed.
    /// The caller must flush the open note first; files are rewritten here.
    pub fn rename_tag(&mut self, old: &str, new: &str) -> Result<usize> {
        let Some(new) = note::normalize_tag(new) else {
            anyhow::bail!("not a valid tag name");
        };
        if new == old {
            return Ok(0);
        }
        let mut changed = 0;
        for entry in self.dir.scan()? {
            let text = self.dir.read(&entry.path)?;
            let (_, body) = note::parse_document(&text);
            let Some(new_body) = note::rename_tag(body, old, &new) else {
                continue;
            };
            if text.ends_with(body) {
                let head = &text[..text.len() - body.len()];
                self.dir
                    .write_atomic(&entry.path, &format!("{head}{new_body}"))?;
                let modified = std::fs::metadata(&entry.path)
                    .and_then(|m| m.modified())
                    .unwrap_or(entry.modified);
                self.index_file(&entry.path, entry.trashed, modified)?;
            } else {
                let mut n = self.index_file(&entry.path, entry.trashed, entry.modified)?;
                n.body = new_body;
                self.write(&mut n)?;
            }
            changed += 1;
        }
        let mut folders_changed = false;
        for f in &mut self.folders {
            if *f == old || (f.starts_with(old) && f[old.len()..].starts_with('/')) {
                *f = format!("{new}{}", &f[old.len()..]);
                folders_changed = true;
            }
        }
        if folders_changed {
            self.folders.sort();
            self.folders.dedup();
            self.write_folders()?;
        }
        Ok(changed)
    }

    fn write_folders(&self) -> Result<()> {
        let text = self.folders.join("\n") + "\n";
        self.dir
            .write_atomic(&self.dir.root().join(FOLDERS_FILE), &text)
    }

    pub fn backlinks(&self, title: &str) -> Result<Vec<NoteSummary>> {
        self.db.backlinks(title)
    }

    pub fn find_by_title(&self, title: &str) -> Result<Option<String>> {
        self.db.find_by_title(title)
    }

    /// Load a note from disk (re-indexing if the file changed underneath us).
    pub fn load(&mut self, id: &str) -> Result<Option<Note>> {
        let Some(row) = self.db.get(id)? else {
            return Ok(None);
        };
        let text = match self.dir.read(&row.path) {
            Ok(t) => t,
            Err(err) => {
                tracing::warn!(id, path = %row.path.display(), %err, "note file vanished; dropping from index");
                self.db.remove(id)?;
                return Ok(None);
            }
        };
        let (fm, body) = note::parse_document(&text);
        let disk_modified: DateTime<Utc> = std::fs::metadata(&row.path)
            .and_then(|m| m.modified())
            .map(Into::into)
            .unwrap_or(row.modified);
        let n = Note {
            id: row.id,
            title: note::derive_title(body),
            body: body.to_owned(),
            created: fm.created.unwrap_or(row.created),
            modified: disk_modified,
            pinned: fm.pinned,
            trashed: row.trashed,
            extra_frontmatter: fm.extra,
            path: row.path,
        };
        if n.title != row.title || n.pinned != row.pinned || disk_modified != row.modified {
            self.index(&n, &note::content_hash(&text))?;
        }
        Ok(Some(n))
    }

    pub fn create(&mut self) -> Result<Note> {
        let now = Utc::now();
        let mut n = Note {
            id: note::new_id(),
            title: note::UNTITLED.to_owned(),
            body: String::new(),
            created: now,
            modified: now,
            pinned: false,
            trashed: false,
            extra_frontmatter: vec![],
            path: PathBuf::new(),
        };
        n.path = self.dir.unique_path(self.dir.root(), &n.title, None);
        self.write(&mut n)?;
        Ok(n)
    }

    /// Persist the note's current body. Renames the file when the title changed.
    pub fn save(&mut self, n: &mut Note) -> Result<()> {
        n.title = note::derive_title(&n.body);
        let dir = if n.trashed {
            self.dir.trash_dir()
        } else {
            self.dir.root().to_owned()
        };
        let new_path = self.place(n, &dir);
        if new_path != n.path {
            if n.path.exists() {
                self.dir.rename(&n.path, &new_path)?;
            }
            n.path = new_path;
        }
        self.write(n)
    }

    /// Where `n` belongs inside `dir`: its current file while the title
    /// still matches it (so "Title (2).md" stays stable), otherwise a free
    /// name for the title.
    fn place(&self, n: &Note, dir: &Path) -> PathBuf {
        let current_stem = n.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let wanted_stem = note::slug_filename(&n.title);
        let keep = current_stem == wanted_stem
            || (current_stem.starts_with(&wanted_stem)
                && current_stem[wanted_stem.len()..].starts_with(" (")
                && current_stem.ends_with(')'));
        if keep && n.path.parent() == Some(dir) {
            return n.path.clone();
        }
        let current = if n.path.as_os_str().is_empty() {
            None
        } else {
            Some(n.path.as_path())
        };
        self.dir.unique_path(dir, &n.title, current)
    }

    fn write(&mut self, n: &mut Note) -> Result<()> {
        n.modified = Utc::now();
        let text = note::serialize_document(n);
        self.dir.write_atomic(&n.path, &text)?;
        // Read the mtime back so the index matches what a later scan sees.
        if let Ok(m) = std::fs::metadata(&n.path).and_then(|m| m.modified()) {
            n.modified = m.into();
        }
        let hash = note::content_hash(&text);
        self.index(n, &hash)?;
        self.db
            .append_oplog(&n.id, n.modified, &self.device_id, &hash)?;
        Ok(())
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<Option<Note>> {
        let Some(mut n) = self.load(id)? else {
            return Ok(None);
        };
        n.pinned = pinned;
        self.write(&mut n)?;
        Ok(Some(n))
    }

    pub fn trash(&mut self, id: &str) -> Result<()> {
        self.move_between(id, true)
    }

    pub fn restore(&mut self, id: &str) -> Result<()> {
        self.move_between(id, false)
    }

    fn move_between(&mut self, id: &str, trashed: bool) -> Result<()> {
        let Some(mut n) = self.load(id)? else {
            return Ok(());
        };
        n.trashed = trashed;
        let dir = if trashed {
            self.dir.trash_dir()
        } else {
            self.dir.root().to_owned()
        };
        let new_path = self.dir.unique_path(&dir, &n.title, None);
        self.dir.rename(&n.path, &new_path)?;
        n.path = new_path;
        let text = self.dir.read(&n.path)?;
        self.index(&n, &note::content_hash(&text))?;
        self.db.append_oplog(
            &n.id,
            Utc::now(),
            &self.device_id,
            &note::content_hash(&text),
        )?;
        Ok(())
    }

    pub fn delete_forever(&mut self, id: &str) -> Result<()> {
        if let Some(row) = self.db.get(id)? {
            self.dir.remove(&row.path)?;
        }
        self.db.remove(id)
    }

    /// Notes created and never typed into (a pre-filled folder tag doesn't
    /// count) are dropped rather than left as `Untitled.md` litter.
    /// Returns true if it was deleted.
    pub fn delete_if_empty(&mut self, id: &str) -> Result<bool> {
        let Some(n) = self.load(id)? else {
            return Ok(false);
        };
        if note::is_blank(&n.body) && !n.trashed {
            self.delete_forever(id)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn empty_trash(&mut self) -> Result<()> {
        for n in self.db.list(&View::Trash)? {
            self.delete_forever(&n.id)?;
        }
        Ok(())
    }

    /// The welcome note is the same bytes on every install — fixed id and
    /// creation time — so two devices syncing agree on it without a copy.
    fn create_welcome_note(&mut self) -> Result<()> {
        let created = chrono::DateTime::parse_from_rfc3339("2026-08-29T00:00:00Z")
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let mut n = Note {
            id: WELCOME_ID.to_owned(),
            title: note::UNTITLED.to_owned(),
            body: WELCOME.to_owned(),
            created,
            modified: created,
            pinned: false,
            trashed: false,
            extra_frontmatter: vec![],
            path: PathBuf::new(),
        };
        self.save(&mut n).context("creating welcome note")
    }
}

fn read_folders(path: &Path) -> Vec<String> {
    parse_folders(&std::fs::read_to_string(path).unwrap_or_default())
}

/// The folder list as written in `.folders`: one tag per line.
fn parse_folders(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text.lines().filter_map(note::normalize_tag).collect();
    out.sort();
    out.dedup();
    out
}

const WELCOME: &str = "# Welcome to Attic

Your notes are plain markdown files in this folder — open them with anything.

- Tags are just `#words` in the text: #welcome #tips/tags
- Nested tags use a slash, like #tips/tags above
- Link notes with double brackets: [[Welcome to Attic]]
- The first line is the title, and the filename follows it

Press **Ctrl+N** for a new note. Everything saves as you type.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(
            tmp.path().join("notes"),
            &tmp.path().join("index.db"),
            "test-device".into(),
        )
        .unwrap();
        (store, tmp)
    }

    #[test]
    fn find_by_path_import_and_readonly_index() {
        let (mut store, tmp) = temp_store();
        let mut n = store.create().unwrap();
        n.body = "# Meeting notes\n\nDiscussed the roadmap #work".into();
        store.save(&mut n).unwrap();

        // A note in the folder is found by its path; a stranger is not.
        assert_eq!(
            store.find_by_path(&n.path).unwrap().as_deref(),
            Some(n.id.as_str())
        );
        let outside = tmp.path().join("elsewhere.md");
        std::fs::write(&outside, "Just a body line\n\nwith #ideas").unwrap();
        assert_eq!(store.find_by_path(&outside).unwrap(), None);

        // Importing copies it in under its own filename and leaves the original.
        let imported = store.import(&outside).unwrap();
        assert!(imported.path.starts_with(store.notes_dir()));
        assert!(
            imported.path.ends_with("elsewhere.md"),
            "{}",
            imported.path.display()
        );
        assert_eq!(imported.title, "Just a body line");
        assert!(outside.exists());
        assert_eq!(
            store.find_by_path(&imported.path).unwrap().as_deref(),
            Some(imported.id.as_str())
        );
        assert_eq!(store.list(&View::Tag("ideas".into())).unwrap().len(), 1);

        // The read-only index the search provider uses sees the same notes.
        let index = Index::open(&tmp.path().join("index.db")).unwrap().unwrap();
        let hits = index.search("roadmap").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, n.id);
        assert_eq!(
            index.path(&n.id).unwrap().as_deref(),
            Some(n.path.as_path())
        );
        assert!(
            Index::open(&tmp.path().join("missing.db"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn welcome_note_then_create_save_rename_trash() {
        let (mut store, _tmp) = temp_store();
        assert_eq!(store.list(&View::All).unwrap().len(), 1);
        assert_eq!(store.tags().unwrap().len(), 2);

        let mut n = store.create().unwrap();
        assert!(n.path.ends_with("Untitled.md"));
        n.body = "# Shopping list\n\n- eggs #home".into();
        store.save(&mut n).unwrap();
        assert!(n.path.ends_with("Shopping list.md"), "{}", n.path.display());
        assert_eq!(n.title, "Shopping list");
        let all = store.list(&View::All).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "Shopping list");
        assert_eq!(all[0].preview, "eggs #home");

        // Title collision gets a suffix.
        let mut m = store.create().unwrap();
        m.body = "Shopping list".into();
        store.save(&mut m).unwrap();
        assert!(
            m.path.ends_with("Shopping list (2).md"),
            "{}",
            m.path.display()
        );
        // Saving again with same title keeps the suffixed name.
        m.body = "Shopping list\nmore".into();
        store.save(&mut m).unwrap();
        assert!(m.path.ends_with("Shopping list (2).md"));

        store.trash(&n.id).unwrap();
        assert_eq!(store.list(&View::Trash).unwrap().len(), 1);
        assert!(
            store
                .load(&n.id)
                .unwrap()
                .unwrap()
                .path
                .to_string_lossy()
                .contains(".trash")
        );
        store.restore(&n.id).unwrap();
        assert!(store.list(&View::Trash).unwrap().is_empty());

        // Pinned floats to the top and survives a reload.
        store.set_pinned(&m.id, true).unwrap();
        assert_eq!(store.list(&View::All).unwrap()[0].id, m.id);
        assert!(store.load(&m.id).unwrap().unwrap().pinned);

        // Empty notes are dropped.
        let e = store.create().unwrap();
        assert!(store.delete_if_empty(&e.id).unwrap());
        assert!(!e.path.exists());
    }

    #[test]
    fn folders_persist_and_merge_into_tags() {
        let (mut store, tmp) = temp_store();
        assert_eq!(
            store.add_folder("Work Stuff").unwrap().as_deref(),
            Some("work-stuff")
        );
        assert_eq!(store.add_folder("#").unwrap(), None);
        assert!(
            store
                .tags()
                .unwrap()
                .contains(&("work-stuff".to_string(), 0))
        );
        drop(store);
        let mut store = Store::open(
            tmp.path().join("notes"),
            &tmp.path().join("index.db"),
            "dev".into(),
        )
        .unwrap();
        assert_eq!(store.folders(), ["work-stuff"]);
        // A tag-only note is still "blank" and gets dropped.
        let mut n = store.create().unwrap();
        n.body = "\n\n#work-stuff\n".into();
        store.save(&mut n).unwrap();
        assert_eq!(n.title, note::UNTITLED);
        assert!(store.delete_if_empty(&n.id).unwrap());
    }

    #[test]
    fn rename_tag_rewrites_files_and_folders() {
        let (mut store, _tmp) = temp_store();
        let mut a = store.create().unwrap();
        a.body = "Osaka\n\n#travels/japan and #travels\n".into();
        store.save(&mut a).unwrap();
        let mut b = store.create().unwrap();
        b.body = "Other\n\n#travelsx stays\n".into();
        store.save(&mut b).unwrap();
        store.add_folder("travels/food").unwrap();

        assert_eq!(store.rename_tag("travels", "journeys").unwrap(), 1);
        let a2 = store.load(&a.id).unwrap().unwrap();
        assert_eq!(a2.body, "Osaka\n\n#journeys/japan and #journeys\n");
        assert_eq!(store.load(&b.id).unwrap().unwrap().body, b.body);
        assert!(store.folders().contains(&"journeys/food".to_string()));
        assert!(!store.folders().iter().any(|f| f.starts_with("travels")));
        let tags = store.tags().unwrap();
        assert!(tags.iter().any(|(t, n)| t == "journeys/japan" && *n == 1));
        assert!(!tags.iter().any(|(t, _)| t == "travels"));
        assert!(store.rename_tag("journeys", "!!!").is_err());
    }

    #[test]
    fn reindex_picks_up_external_files() {
        let (mut store, _tmp) = temp_store();
        let path = store.notes_dir().join("From Obsidian.md");
        std::fs::write(&path, "---\ntags: [x]\n---\n# From Obsidian\nhello #ext\n").unwrap();
        assert_eq!(store.reindex().unwrap(), 1);
        let all = store.list(&View::All).unwrap();
        let ext = all.iter().find(|n| n.title == "From Obsidian").unwrap();
        // The file got an id stamped in, with the foreign frontmatter preserved.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("id: "));
        assert!(text.contains("tags: [x]"));
        let loaded = store.load(&ext.id).unwrap().unwrap();
        assert_eq!(loaded.extra_frontmatter, vec!["tags: [x]".to_string()]);
        // Deleting the file drops it from the index.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(store.reindex().unwrap(), 1);
        assert!(store.load(&ext.id).unwrap().is_none());
    }

    #[test]
    fn reindex_follows_a_rename_and_reloads_folders() {
        let (mut store, _tmp) = temp_store();
        let mut n = store.create().unwrap();
        n.body = "# Kyoto\n\nnight one\n".into();
        store.save(&mut n).unwrap();
        let old = store.notes_dir().join(&n.path);
        let new = store.notes_dir().join("Kyoto, night one.md");
        let before = store.list(&View::All).unwrap().len();
        // Another machine renamed the file; Dropbox replays the rename here.
        std::fs::rename(&old, &new).unwrap();
        store.reindex().unwrap();
        let again = store.load(&n.id).unwrap().expect("same id, new name");
        assert_eq!(again.path, new);
        assert_eq!(store.list(&View::All).unwrap().len(), before);

        std::fs::write(store.notes_dir().join(".folders"), "travel\nfood\n").unwrap();
        assert!(store.reload_folders());
        assert_eq!(store.folders(), ["food", "travel"]);
        assert!(!store.reload_folders());
    }
}
