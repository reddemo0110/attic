// SPDX-License-Identifier: GPL-3.0-only

//! GNOME Shell search provider: notes show up in the Activities overview as
//! you type, and picking one opens it in the app.
//!
//! GNOME Shell reads `share/gnome-shell/search-providers/*.ini`, D-Bus
//! activates the bus name named there (`server/` has nothing to do with
//! this — the `.service` file under `share/dbus-1/services` points at
//! `attic --search-provider`) and talks `org.gnome.Shell.SearchProvider2`
//! to it. This process is that service: headless, answers from the SQLite
//! index read-only, launches the real app to open a result, and quits after
//! a while with nothing to do so it does not sit in memory. The shell just
//! activates it again next time.

use crate::config::Config;
use crate::store::Index;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::Value;

/// The bus name the shell activates. Distinct from the app's own name so
/// the provider and the app never fight over who owns what.
pub const BUS_NAME: &str = "io.github.reddemo0110.Attic.SearchProvider";
pub const OBJECT_PATH: &str = "/io/github/reddemo0110/Attic/SearchProvider";

/// Quit after this long without a request.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// The overview shows a handful of results per provider; no point ranking
/// more than this.
const MAX_RESULTS: usize = 12;

struct Provider {
    app_id: &'static str,
    index_path: PathBuf,
    last_used: Arc<Mutex<Instant>>,
}

impl Provider {
    fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    fn index(&self) -> Option<Index> {
        match Index::open(&self.index_path) {
            Ok(idx) => idx,
            Err(err) => {
                tracing::warn!(%err, "opening the index");
                None
            }
        }
    }

    fn search(&self, terms: &[String]) -> Vec<String> {
        let query = terms.join(" ");
        if query.trim().is_empty() {
            return Vec::new();
        }
        let Some(index) = self.index() else {
            return Vec::new();
        };
        match index.search(&query) {
            Ok(hits) => hits.into_iter().take(MAX_RESULTS).map(|n| n.id).collect(),
            Err(err) => {
                tracing::warn!(%err, query, "searching the index");
                Vec::new()
            }
        }
    }

}

/// Launch (or hand to the running instance, via single-instance
/// activation) the app with these arguments. Shared with the COSMIC
/// launcher plugin, which opens hits the same way.
pub(crate) fn launch_app(args: &[String]) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            tracing::error!(%err, "locating the app binary");
            return;
        }
    };
    match std::process::Command::new(&exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!(?args, "launched the app"),
        Err(err) => tracing::error!(%err, exe = %exe.display(), "launching the app"),
    }
}

// zbus hands method arguments in by value.
#[allow(clippy::needless_pass_by_value, clippy::unused_self)]
#[zbus::interface(name = "org.gnome.Shell.SearchProvider2")]
impl Provider {
    fn get_initial_result_set(&self, terms: Vec<String>) -> Vec<String> {
        self.touch();
        self.search(&terms)
    }

    fn get_subsearch_result_set(
        &self,
        _previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Vec<String> {
        self.touch();
        // The FTS index is fast enough that narrowing the previous set
        // buys nothing; a fresh query also re-ranks properly.
        self.search(&terms)
    }

    fn get_result_metas(&self, identifiers: Vec<String>) -> Vec<HashMap<String, Value<'static>>> {
        self.touch();
        let Some(index) = self.index() else {
            return Vec::new();
        };
        identifiers
            .iter()
            .filter_map(|id| {
                let row = index.path(id).ok().flatten()?;
                let text = std::fs::read_to_string(&row).ok()?;
                let (_, body) = crate::note::parse_document(&text);
                let mut meta = HashMap::new();
                meta.insert("id".to_owned(), Value::from(id.clone()));
                meta.insert(
                    "name".to_owned(),
                    Value::from(crate::note::derive_title(body)),
                );
                meta.insert(
                    "description".to_owned(),
                    Value::from(crate::note::preview(body)),
                );
                meta.insert("gicon".to_owned(), Value::from(self.app_id.to_owned()));
                Some(meta)
            })
            .collect()
    }

    fn activate_result(&self, identifier: String, _terms: Vec<String>, _timestamp: u32) {
        self.touch();
        let Some(path) = self
            .index()
            .and_then(|i| i.path(&identifier).ok().flatten())
        else {
            tracing::warn!(identifier, "result vanished from the index");
            return;
        };
        launch_app(&[path.to_string_lossy().into_owned()]);
    }

    fn launch_search(&self, terms: Vec<String>, _timestamp: u32) {
        self.touch();
        launch_app(&["--search".to_owned(), terms.join(" ")]);
    }
}

/// Serve searches until nobody has asked for a while. Blocks.
pub fn serve(app_id: &'static str) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let last_used = Arc::new(Mutex::new(Instant::now()));
        let provider = Provider {
            app_id,
            index_path: Config::index_path(app_id),
            last_used: last_used.clone(),
        };
        let _conn = zbus::connection::Builder::session()?
            .name(BUS_NAME)?
            .serve_at(OBJECT_PATH, provider)?
            .build()
            .await?;
        tracing::info!(name = BUS_NAME, "search provider up");
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            if last_used.lock().unwrap().elapsed() > IDLE_TIMEOUT {
                tracing::info!("idle; search provider exiting");
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    })
}
