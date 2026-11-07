// SPDX-License-Identifier: GPL-3.0-only

//! The notes folder, watched for changes made by someone else: a sync
//! client such as Dropbox bringing another machine's edits, or any other
//! editor. Pure plumbing; what to do about a change is the app's call
//! (`App::external_change`).

use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc;
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

/// How long to let a burst settle before reporting it: a sync client
/// writes a batch of files in quick succession, and an atomic write is a
/// create plus a rename.
const SETTLE: Duration = Duration::from_millis(500);

/// Changed paths under `dir`, in batches. Our own atomic-write temp files
/// are left out; everything else (notes, `.trash/`, `assets/`, `.folders`)
/// is reported, and finding out what actually changed is the receiver's
/// job. A new `dir` is a new subscription; the old watcher goes with it.
pub fn notes_dir(dir: PathBuf) -> Subscription<Vec<PathBuf>> {
    Subscription::run_with(dir, |dir| {
        let dir = dir.clone();
        cosmic::iced::stream::channel(8, move |tx: mpsc::Sender<Vec<PathBuf>>| async move {
            std::thread::Builder::new()
                .name("notes-watch".into())
                .spawn(move || watch(&dir, tx))
                .ok();
            std::future::pending::<()>().await;
        })
    })
}

fn watch(dir: &Path, mut tx: mpsc::Sender<Vec<PathBuf>>) {
    let (events_tx, events_rx) = std_mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(move |ev| {
        let _ = events_tx.send(ev);
    }) {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(%err, "not watching the notes folder");
            return;
        }
    };
    if let Err(err) = watcher.watch(dir, RecursiveMode::Recursive) {
        tracing::warn!(%err, dir = %dir.display(), "not watching the notes folder");
        return;
    }
    tracing::debug!(dir = %dir.display(), "watching the notes folder");
    loop {
        let Ok(first) = events_rx.recv() else {
            return;
        };
        let mut paths = Vec::new();
        collect(first, &mut paths);
        // Let the burst settle, then report it in one go.
        let deadline = Instant::now() + SETTLE;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match events_rx.recv_timeout(left) {
                Ok(ev) => collect(ev, &mut paths),
                Err(std_mpsc::RecvTimeoutError::Timeout) => break,
                Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        if paths.is_empty() {
            continue;
        }
        paths.sort();
        paths.dedup();
        // The app drains this quickly; a full channel means it is busy,
        // and the batch after the next re-scan covers for a dropped one.
        for _ in 0..20 {
            match tx.try_send(std::mem::take(&mut paths)) {
                Ok(()) => break,
                Err(err) if err.is_full() => {
                    paths = err.into_inner();
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return,
            }
        }
    }
}

fn collect(ev: notify::Result<notify::Event>, into: &mut Vec<PathBuf>) {
    match ev {
        Ok(ev) => into.extend(ev.paths.into_iter().filter(|p| !is_temp(p))),
        Err(err) => tracing::debug!(%err, "notes watcher"),
    }
}

/// Our own atomic writes go through `.name.tmp` beside the file.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // our own exact name
fn is_temp(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.') && n.ends_with(".tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_files_are_ours() {
        assert!(is_temp(Path::new("/n/.Kyoto.md.tmp")));
        assert!(!is_temp(Path::new("/n/Kyoto.md")));
        assert!(!is_temp(Path::new("/n/.folders")));
        assert!(!is_temp(Path::new("/n/.trash/Old.md")));
    }
}
