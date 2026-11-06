// SPDX-License-Identifier: GPL-3.0-only

mod anim;
mod app;
mod blocks;
mod coffee;
mod config;
mod debug_script;
mod desktop;
mod dockicon;
mod editor;
mod glyph;
mod i18n;
mod icon;
mod images;
mod launcher;
mod markdown;
mod probe;
mod retro;
mod search_provider;
mod secrets;

// The core's modules keep their old `crate::` paths inside this crate.
use jjb_core::{links, note, store, sync, table};

use cosmic::Application as _;

const USAGE: &str = "\
Usage: attic [OPTIONS] [FILE]...

Retro-flavoured markdown notes. With no arguments, opens (or raises) the
notes window. Files inside the notes folder open in place; other markdown
files are imported as copies.

Options:
  --new              Start on a fresh note
  --search TEXT...   Open with TEXT in the search box
  --search-provider  Run the GNOME Shell search service (D-Bus activated)
  --launcher-plugin  Run the COSMIC launcher plugin (started by pop-launcher)
  -V, --version      Print the version
  -h, --help         Show this help";

fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launcher_plugin = args.iter().any(|a| a == "--launcher-plugin");

    // Off COSMIC there is no cosmic-theme config, and libcosmic logs an
    // error per missing key at every start; that is expected there.
    let desktop = desktop::Desktop::current();
    let default_filter = if desktop == desktop::Desktop::Cosmic {
        "attic=info,warn"
    } else {
        "attic=info,warn,cosmic::theme=off"
    };
    let log = tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| default_filter.into()),
    );
    // The launcher plugin's stdout is its protocol; keep the log off it.
    if launcher_plugin {
        log.with_writer(std::io::stderr).init();
    } else {
        log.init();
    }
    tracing::info!(desktop = desktop.name(), "session");
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("attic {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // `attic --search-provider` is the headless GNOME Shell search
    // service, D-Bus activated by the shell, and `attic --launcher-plugin`
    // its COSMIC twin, started by pop-launcher; no window, no store.
    let Some(flags) = app::Flags::from_args(args) else {
        let served = if launcher_plugin {
            launcher::serve(app::AppModel::APP_ID)
        } else {
            search_provider::serve(app::AppModel::APP_ID)
        };
        if let Err(err) = served {
            tracing::error!(%err, "headless service");
            std::process::exit(1);
        }
        return Ok(());
    };

    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Settings for configuring the application window and iced runtime.
    let mut settings = cosmic::app::Settings::default().size_limits(
        cosmic::iced::Limits::NONE
            .min_width(480.0)
            .min_height(320.0),
    );
    // Reopen at the size the window closed at.
    if let Some((w, h)) = last_window_size() {
        settings = settings.size(cosmic::iced::Size::new(w, h));
    }

    // Off COSMIC, borrow the desktop's fonts, icon theme and title-bar
    // buttons (GNOME via the portal) rather than assume COSMIC's. The app
    // keeps following them through `desktop::watch()`.
    if let Some(theme) = desktop::settings().icon_theme {
        settings = settings.default_icon_theme(theme);
    }
    desktop::reassert();

    // One window per user: a second launch (from the dock, `attic
    // file.md`, the search provider) hands its arguments to the running
    // instance over D-Bus and exits.
    cosmic::app::run_single_instance::<app::AppModel>(settings, flags)
}

/// The window size saved on the last run, if any.
fn last_window_size() -> Option<(f32, f32)> {
    use cosmic::cosmic_config::CosmicConfigEntry;
    let ctx = cosmic::cosmic_config::Config::new(
        "io.github.reddemo0110.Attic",
        config::Config::VERSION,
    )
    .ok()?;
    let cfg = config::Config::get_entry(&ctx).unwrap_or_else(|(_, cfg)| cfg);
    (cfg.window_width >= 480 && cfg.window_height >= 320)
        .then(|| (cfg.window_width as f32, cfg.window_height as f32))
}
