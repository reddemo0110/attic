// SPDX-License-Identifier: GPL-3.0-only

//! COSMIC launcher plugin: `note <words>` in the launcher (Super) lists the
//! matching notes, and picking one opens it in the app.
//!
//! pop-launcher, the service behind cosmic-launcher, finds plugins under
//! `~/.local/share/pop-launcher/plugins/*/plugin.ron` (also `/etc/…` and
//! `/usr/lib/…`), starts the plugin's binary when the typed text matches
//! the manifest's regex, and talks line-delimited JSON over its stdin and
//! stdout: a `Search` per keystroke, `Activate` when a result is chosen,
//! `Exit` when it is done with us. This process is that plugin
//! (`attic --launcher-plugin`, wrapped by the `attic-launcher` script the
//! installer writes next to the manifest): the GNOME search provider's twin
//! (`search_provider.rs`), headless, answering from the SQLite index
//! read-only and launching the real app to open a hit. Logs go to stderr;
//! stdout is the protocol.

use crate::config::Config;
use crate::note::NoteSummary;
use crate::store::Index;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;

/// The words that wake the plugin; `resources/launcher-plugin.ron`'s
/// regex must agree.
pub const PREFIXES: &[&str] = &["note", "attic"];
/// The launcher shows a short list; the app is one keystroke away for more.
const MAX_RESULTS: usize = 8;

/// What pop-launcher sends us (`pop_launcher::Request`), fields and all so
/// every message parses; we act on a few of them.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
enum Request {
    Activate(u32),
    ActivateContext { id: u32, context: u32 },
    Complete(u32),
    Context(u32),
    Exit,
    Close,
    Interrupt,
    Quit(u32),
    Search(String),
}

/// What we send back (`pop_launcher::PluginResponse`).
#[derive(Debug, Serialize)]
enum Response<'a> {
    Append(SearchResult<'a>),
    Close,
    Fill(String),
    Finished,
}

/// `pop_launcher::PluginSearchResult`; every field is sent, the optional
/// ones as `null`, which is how the crate's own serde layout reads them.
#[derive(Debug, Serialize)]
struct SearchResult<'a> {
    id: u32,
    name: &'a str,
    description: &'a str,
    keywords: Option<Vec<String>>,
    icon: Option<IconSource<'a>>,
    exec: Option<String>,
    window: Option<(u32, u32)>,
}

#[derive(Debug, Serialize)]
enum IconSource<'a> {
    Name(&'a str),
}

/// The words after the `note` / `attic` prefix, or the whole query when
/// the launcher sent it without one (the manifest's regex should prevent
/// that, but a plugin must not choke on it). `""` means "just the prefix".
pub fn words(query: &str) -> &str {
    let query = query.trim_start();
    let (first, rest) = query
        .split_once(char::is_whitespace)
        .unwrap_or((query, ""));
    if PREFIXES.iter().any(|p| first.eq_ignore_ascii_case(p)) {
        rest.trim()
    } else {
        query.trim_end()
    }
}

fn search(index_path: &Path, query: &str) -> Vec<NoteSummary> {
    let index = match Index::open(index_path) {
        Ok(Some(index)) => index,
        Ok(None) => return Vec::new(),
        Err(err) => {
            tracing::warn!(%err, "opening the index");
            return Vec::new();
        }
    };
    // An empty query lists the most recent notes, so `note ` alone is a
    // useful thing to type.
    match index.search(query) {
        Ok(hits) => hits.into_iter().take(MAX_RESULTS).collect(),
        Err(err) => {
            tracing::warn!(%err, query, "searching the index");
            Vec::new()
        }
    }
}

fn open(index_path: &Path, id: &str) {
    let path = Index::open(index_path)
        .ok()
        .flatten()
        .and_then(|index| index.path(id).ok().flatten());
    if let Some(path) = path {
        crate::search_provider::launch_app(&[path.to_string_lossy().into_owned()]);
    } else {
        tracing::warn!(id, "result vanished from the index");
    }
}

fn send(out: &mut impl Write, response: &Response<'_>) -> std::io::Result<()> {
    serde_json::to_writer(&mut *out, response)?;
    out.write_all(b"\n")?;
    out.flush()
}

/// Serve the launcher until it says `Exit` or closes our stdin. Blocks.
pub fn serve(app_id: &'static str) -> anyhow::Result<()> {
    let index_path = Config::index_path(app_id);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    let mut hits: Vec<NoteSummary> = Vec::new();
    tracing::info!("launcher plugin up");
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                tracing::debug!(%err, line, "request not understood");
                continue;
            }
        };
        match request {
            Request::Search(query) => {
                hits = search(&index_path, words(&query));
                for (i, hit) in hits.iter().enumerate() {
                    #[allow(clippy::cast_possible_truncation)]
                    let result = SearchResult {
                        id: i as u32,
                        name: &hit.title,
                        description: &hit.preview,
                        keywords: None,
                        icon: Some(IconSource::Name(app_id)),
                        exec: None,
                        window: None,
                    };
                    send(&mut out, &Response::Append(result))?;
                }
                send(&mut out, &Response::Finished)?;
            }
            Request::Activate(i) | Request::ActivateContext { id: i, .. } => {
                if let Some(hit) = hits.get(i as usize) {
                    open(&index_path, &hit.id);
                }
                send(&mut out, &Response::Close)?;
            }
            Request::Complete(i) => {
                if let Some(hit) = hits.get(i as usize) {
                    send(&mut out, &Response::Fill(format!("{} {}", PREFIXES[0], hit.title)))?;
                }
            }
            Request::Context(_) | Request::Interrupt | Request::Quit(_) | Request::Close => {}
            Request::Exit => break,
        }
    }
    tracing::info!("launcher plugin exiting");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_prefix() {
        assert_eq!(words("note tuscany"), "tuscany");
        assert_eq!(words("Note  packing list "), "packing list");
        assert_eq!(words("attic japan"), "japan");
        // Just the prefix, with or without the space: list recent notes.
        assert_eq!(words("note "), "");
        assert_eq!(words("note"), "");
        assert_eq!(words("notebook ideas"), "notebook ideas");
    }

    #[test]
    fn speaks_pop_launcher_json() {
        let request: Request = serde_json::from_str(r#"{"Search":"note x"}"#).unwrap();
        assert!(matches!(request, Request::Search(ref q) if q == "note x"));
        let request: Request = serde_json::from_str(r#"{"Activate":2}"#).unwrap();
        assert!(matches!(request, Request::Activate(2)));
        let request: Request = serde_json::from_str(r#""Exit""#).unwrap();
        assert!(matches!(request, Request::Exit));

        let result = SearchResult {
            id: 0,
            name: "Japan",
            description: "Tokyo first",
            keywords: None,
            icon: Some(IconSource::Name("io.github.reddemo0110.Attic")),
            exec: None,
            window: None,
        };
        assert_eq!(
            serde_json::to_string(&Response::Append(result)).unwrap(),
            r#"{"Append":{"id":0,"name":"Japan","description":"Tokyo first","keywords":null,"icon":{"Name":"io.github.reddemo0110.Attic"},"exec":null,"window":null}}"#
        );
        assert_eq!(serde_json::to_string(&Response::Finished).unwrap(), r#""Finished""#);
        assert_eq!(serde_json::to_string(&Response::Close).unwrap(), r#""Close""#);
    }
}
