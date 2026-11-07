// SPDX-License-Identifier: GPL-3.0-only

//! Which desktop we are running on, and what to borrow from it.
//!
//! libcosmic assumes COSMIC: it reads fonts, icon theme and header-button
//! preferences from cosmic-config, which on GNOME does not exist, so it
//! falls back to "Open Sans", "Noto Sans Mono" and the "Cosmic" icon theme —
//! none of which a GNOME machine has installed. On GNOME (and anything else
//! that is not COSMIC) we ask the XDG desktop portal for the user's actual
//! settings instead: interface and monospace font, icon theme, which
//! window buttons the shell shows, and the accent colour. Dark/light and
//! contrast arrive through the portal via libcosmic itself; the accent does
//! not (libcosmic has a handler for it but never asks the portal), so the
//! app reads `org.freedesktop.appearance accent-color` and rebuilds the
//! system theme around it, see [`softened_theme`].
//!
//! The values are pushed into libcosmic's toolkit config (`COSMIC_TK`).
//! libcosmic replaces that struct wholesale when its own config subscription
//! reports (which it does once at start-up, with defaults, when the file is
//! missing), so [`reassert`] is cheap and idempotent and the app calls it
//! on every view. [`watch`] follows the portal's `SettingChanged` signal so
//! a font or icon theme changed in GNOME Settings takes effect without a
//! restart, the way it does for native apps.
//!
//! Which desktop we are on comes from the session environment
//! (`XDG_CURRENT_DESKTOP`, with the older variables as fallbacks); a
//! process started without one (D-Bus activation, a launcher plugin) asks
//! the session bus who is running instead.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, RwLock};

/// Owned on the session bus only while that desktop runs. Checked in this
/// order: COSMIC implements some `org.gnome.*` names for compatibility.
const COSMIC_BUS_NAME: &str = "com.system76.CosmicSettingsDaemon";
const GNOME_BUS_NAME: &str = "org.gnome.Shell";

/// The desktop environment the app was launched from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    Cosmic,
    Gnome,
    Other,
}

impl Desktop {
    /// Classify `XDG_CURRENT_DESKTOP` (a colon-separated list, e.g.
    /// `ubuntu:GNOME`, `pop:GNOME`, `COSMIC`, `KDE`).
    pub fn from_xdg(value: &str) -> Desktop {
        let mut names = value.split(':').map(|n| n.trim().to_ascii_lowercase());
        if names.clone().any(|n| n == "cosmic") {
            Desktop::Cosmic
        } else if names.any(|n| n == "gnome" || n.ends_with("-gnome") || n.starts_with("gnome")) {
            Desktop::Gnome
        } else {
            Desktop::Other
        }
    }

    /// Classify from the session's variables, most authoritative first:
    /// the first one that names a desktop we know wins. `None` when none
    /// of them is set at all (the process has no session environment).
    pub fn from_env<'a>(vars: impl IntoIterator<Item = &'a str>) -> Option<Desktop> {
        let mut any = false;
        for value in vars {
            any = true;
            let desktop = Desktop::from_xdg(value);
            if desktop != Desktop::Other {
                return Some(desktop);
            }
        }
        any.then_some(Desktop::Other)
    }

    pub fn current() -> Desktop {
        static CURRENT: OnceLock<Desktop> = OnceLock::new();
        *CURRENT.get_or_init(|| {
            let vars: Vec<String> = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
                .iter()
                .filter_map(|key| std::env::var(key).ok())
                .filter(|v| !v.trim().is_empty())
                .collect();
            if let Some(desktop) = Desktop::from_env(vars.iter().map(String::as_str)) {
                return desktop;
            }
            let desktop = probe_bus();
            tracing::debug!(?desktop, "no session variables; desktop guessed from the bus");
            desktop
        })
    }

    /// For logs and the About dialog.
    pub fn name(self) -> &'static str {
        match self {
            Desktop::Cosmic => "COSMIC",
            Desktop::Gnome => "GNOME",
            Desktop::Other => "another desktop",
        }
    }
}

/// Which desktop owns its well-known name on the session bus.
fn probe_bus() -> Desktop {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return Desktop::Other;
    };
    let owned = |name: &str| -> bool {
        conn.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(name,),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
    };
    if owned(COSMIC_BUS_NAME) {
        Desktop::Cosmic
    } else if owned(GNOME_BUS_NAME) {
        Desktop::Gnome
    } else {
        Desktop::Other
    }
}

/// What the desktop told us. Every field is optional: a key the portal
/// does not know leaves libcosmic's default in place.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Family name of the interface font (Pango description with the size
    /// and style stripped), e.g. `Cantarell`.
    pub interface_font: Option<String>,
    /// Family name of the monospace font, e.g. `Source Code Pro`.
    pub monospace_font: Option<String>,
    /// Icon theme name, e.g. `Adwaita`.
    pub icon_theme: Option<String>,
    /// Whether the shell shows minimize / maximize buttons in title bars
    /// (GNOME's `button-layout`). `None` = unknown, keep the default.
    pub show_minimize: Option<bool>,
    pub show_maximize: Option<bool>,
    /// The desktop's accent colour (`org.freedesktop.appearance
    /// accent-color`), 8-bit sRGB. `None` when the desktop has none.
    pub accent: Option<[u8; 3]>,
}

/// What the desktop last told us; `None` until first asked.
static SETTINGS: RwLock<Option<Settings>> = RwLock::new(None);

/// The desktop's settings as last read: at start-up, then whenever
/// [`apply`] adopted a change. Empty on COSMIC, where libcosmic reads its
/// own config.
pub fn settings() -> Settings {
    if let Some(s) = SETTINGS.read().unwrap().as_ref() {
        return s.clone();
    }
    let s = read_current();
    tracing::info!(desktop = ?Desktop::current(), ?s, "desktop settings");
    SETTINGS.write().unwrap().get_or_insert(s).clone()
}

fn read_current() -> Settings {
    if Desktop::current() == Desktop::Cosmic {
        return Settings::default();
    }
    read_portal().unwrap_or_else(read_gsettings)
}

/// Adopt settings the desktop changed while we run (from [`watch`]):
/// remember them, switch the icon theme and push the rest into libcosmic.
/// Returns whether anything was different.
pub fn apply(new: Settings) -> bool {
    if SETTINGS.read().unwrap().as_ref() == Some(&new) {
        return false;
    }
    if let Some(theme) = &new.icon_theme
        && cosmic::icon_theme::default() != *theme
    {
        cosmic::icon_theme::set_default(theme.clone());
    }
    push(&new);
    tracing::info!(?new, "desktop settings changed");
    *SETTINGS.write().unwrap() = Some(new);
    true
}

/// Push the desktop's fonts and button layout into libcosmic's toolkit
/// config if they are not there already. Safe to call often.
pub fn reassert() {
    let s = settings();
    if s == Settings::default() {
        return;
    }
    let tk = cosmic::config::COSMIC_TK.read().unwrap();
    let stale = s
        .interface_font
        .as_ref()
        .is_some_and(|f| tk.interface_font.family != *f)
        || s.monospace_font
            .as_ref()
            .is_some_and(|f| tk.monospace_font.family != *f)
        || s.show_minimize.is_some_and(|b| tk.show_minimize != b)
        || s.show_maximize.is_some_and(|b| tk.show_maximize != b);
    drop(tk);
    if stale {
        push(&s);
    }
}

fn push(s: &Settings) {
    let mut tk = cosmic::config::COSMIC_TK.write().unwrap();
    if let Some(f) = &s.interface_font {
        tk.interface_font.family.clone_from(f);
    }
    if let Some(f) = &s.monospace_font {
        tk.monospace_font.family.clone_from(f);
    }
    if let Some(b) = s.show_minimize {
        tk.show_minimize = b;
    }
    if let Some(b) = s.show_maximize {
        tk.show_maximize = b;
    }
}

/// The window outline GNOME draws: Adwaita's faint translucent line, a
/// 10 % white highlight on dark and a 12 % black line on light.
fn outline_colour(is_dark: bool) -> cosmic::cosmic_theme::palette::Srgba {
    use cosmic::cosmic_theme::palette::Srgba;
    if is_dark {
        Srgba::new(1.0, 1.0, 1.0, 0.10)
    } else {
        Srgba::new(0.0, 0.0, 0.0, 0.12)
    }
}

/// How many times to fix up one and the same system theme before giving
/// up on it. At start-up libcosmic installs the theme, replaces it when
/// the portal reports the colour scheme and rebuilds it for the accent,
/// each time dropping our divider; three attempts were seen to be needed
/// before one stuck. Past this it would be a redraw loop, not a race.
const OUTLINE_ATTEMPTS: u8 = 6;
/// (`is_dark`, accent) of the theme last fixed up, and how often.
static OUTLINE_TRIES: Mutex<Option<((bool, [u8; 3]), u8)>> = Mutex::new(None);

type Srgba = cosmic::cosmic_theme::palette::Srgba;

fn rgb8(c: Srgba) -> [u8; 3] {
    let c = c.into_format::<u8, u8>();
    [c.red, c.green, c.blue]
}

/// The accent the theme would carry once rebuilt around the desktop's:
/// libcosmic keeps the theme's own accent lightness and takes hue and
/// chroma from the desktop (`Theme::with_accent`), so a straight
/// comparison with the portal colour would never settle. `None` when the
/// desktop has no accent.
fn wanted_accent(theme: &cosmic::cosmic_theme::Theme) -> Option<Srgba> {
    use cosmic::cosmic_theme::palette::{IntoColor, Oklcha, Srgb};
    let [r, g, b] = settings().accent?;
    let desktop = Srgba::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    );
    let mut oklcha: Oklcha = desktop.into_color();
    let current: Oklcha = theme.accent_color().into_color();
    oklcha.l = current.l;
    let adjusted: Srgb = oklcha.into_color();
    Some(Srgba::new(adjusted.red, adjusted.green, adjusted.blue, 1.0))
}

/// What the active system theme still lacks of the desktop's look, or
/// `None` on COSMIC, under a custom or high-contrast theme, or once it is
/// all in place: (the theme, needs the accent, needs the outline).
fn fixups() -> Option<(cosmic::Theme, bool, bool)> {
    use cosmic::theme::ThemeType;
    if Desktop::current() == Desktop::Cosmic {
        return None;
    }
    let active = cosmic::theme::active();
    if !matches!(
        active.theme_type,
        ThemeType::System { .. } | ThemeType::Dark | ThemeType::Light
    ) {
        return None;
    }
    let theme = active.cosmic();
    let accent = wanted_accent(theme).is_some_and(|c| rgb8(c) != rgb8(theme.accent_color()));
    let outline = theme.bg_divider() != outline_colour(theme.is_dark);
    (accent || outline).then_some((active, accent, outline))
}

/// libcosmic draws the 1 px border around a floating window in the
/// theme's `bg_divider`, which it composites to an opaque 20 % blend of
/// the text colour: the hard grey line around the window next to GNOME's
/// own, which have a faint translucent outline (and a shadow Mutter leaves
/// to the client, which libcosmic cannot draw). The border is hard-wired
/// in libcosmic's window layout, so the theme is the lever: the system
/// theme with its divider softened to GNOME's, and, since libcosmic never
/// asks the portal for it, rebuilt around the desktop's accent colour
/// (the menu bar, links, selection and the buffet's system palette all
/// wear the accent). `None` when there is nothing to do (on COSMIC, under
/// a custom or high-contrast theme, or once it is already in place).
/// libcosmic reinstalls a fresh system theme at start-up and on every
/// dark/light change, so the app checks [`outline_stale`] on each redraw
/// and re-applies. The theme's colour fields are private; the divider
/// goes through its serde form.
pub fn softened_theme() -> Option<cosmic::Theme> {
    let (active, fix_accent, fix_outline) = fixups()?;
    let theme = active.cosmic();
    let key = (theme.is_dark, rgb8(theme.accent_color()));
    {
        let mut tries = OUTLINE_TRIES.lock().unwrap();
        let n = match *tries {
            Some((k, n)) if k == key => n,
            _ => 0,
        };
        if n >= OUTLINE_ATTEMPTS {
            return None;
        }
        *tries = Some((key, n + 1));
    }
    let accented;
    let theme = if fix_accent && let Some(accent) = wanted_accent(theme) {
        accented = theme.with_accent(accent);
        &accented
    } else {
        theme
    };
    let wanted = outline_colour(theme.is_dark);
    let softened = (|| -> serde_json::Result<cosmic::cosmic_theme::Theme> {
        let mut value = serde_json::to_value(theme)?;
        value["background"]["divider"] = serde_json::json!({
            "red": wanted.red,
            "green": wanted.green,
            "blue": wanted.blue,
            "alpha": wanted.alpha,
        });
        serde_json::from_value(value)
    })();
    match softened {
        Ok(softened) => {
            tracing::info!(
                dark = theme.is_dark,
                accent = fix_accent,
                outline = fix_outline,
                "fitting the system theme to the desktop"
            );
            Some(cosmic::Theme::system(std::sync::Arc::new(softened)))
        }
        Err(err) => {
            tracing::warn!(%err, "not fitting the system theme to the desktop");
            None
        }
    }
}

/// Cheap enough for the event listener: does the active theme still
/// carry libcosmic's outline or its own accent?
pub fn outline_stale() -> bool {
    let Some((active, ..)) = fixups() else {
        return false;
    };
    let theme = active.cosmic();
    let key = (theme.is_dark, rgb8(theme.accent_color()));
    !matches!(*OUTLINE_TRIES.lock().unwrap(), Some((k, n)) if k == key && n >= OUTLINE_ATTEMPTS)
}

/// `org.freedesktop.portal.Settings`, the signal half: the portal relays
/// every gsettings change in the namespaces it serves.
#[zbus::proxy(
    interface = "org.freedesktop.portal.Settings",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop",
    gen_blocking = false
)]
trait PortalSettings {
    #[zbus(signal)]
    fn setting_changed(
        &self,
        namespace: &str,
        key: &str,
        value: zbus::zvariant::Value<'_>,
    ) -> zbus::Result<()>;
}

/// The desktop's settings as they change while the app runs: a new
/// [`Settings`] each time GNOME's fonts, icon theme or title-bar buttons
/// change, for the app to hand to [`apply`]. Never yields on COSMIC
/// (libcosmic follows its own config there) or where no portal answers.
pub fn watch() -> cosmic::iced::Subscription<Settings> {
    cosmic::iced::Subscription::run(|| {
        cosmic::iced::stream::channel(4, |tx| async move {
            if Desktop::current() != Desktop::Cosmic {
                // The bus client lives on its own thread with its own
                // runtime: the app's executor is iced's business, and the
                // blocking reader (`read_current`) is at home there.
                std::thread::Builder::new()
                    .name("desktop-settings".into())
                    .spawn(move || {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build();
                        let served = runtime.map_err(zbus::Error::from).and_then(|rt| {
                            rt.block_on(follow(tx))
                        });
                        if let Err(err) = served {
                            tracing::warn!(%err, "not following desktop settings");
                        }
                    })
                    .ok();
            }
            std::future::pending::<()>().await;
        })
    })
}

async fn follow(
    mut tx: cosmic::iced::futures::channel::mpsc::Sender<Settings>,
) -> zbus::Result<()> {
    use cosmic::iced::futures::{SinkExt, StreamExt};
    let conn = zbus::Connection::session().await?;
    let proxy = PortalSettingsProxy::new(&conn).await?;
    let mut changes = proxy.receive_setting_changed().await?;
    tracing::debug!("following desktop settings through the portal");
    while let Some(signal) = changes.next().await {
        let args = match signal.args() {
            Ok(args) => args,
            Err(err) => {
                tracing::warn!(%err, "portal signal not understood");
                continue;
            }
        };
        let ns: &str = args.namespace();
        tracing::debug!(ns, key = args.key(), "portal setting changed");
        if ns != INTERFACE_NS && ns != WM_NS && ns != APPEARANCE_NS {
            continue;
        }
        // The signal carries the one key, but GNOME fires a burst of them
        // for a font change and ReadAll is one cheap round trip: re-read
        // the lot rather than patch a cache. Off the async thread, since
        // the reader is the blocking client.
        let s = match tokio::task::spawn_blocking(read_current).await {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(%err, "re-reading desktop settings");
                continue;
            }
        };
        if tx.send(s).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// Style words a Pango font description may end with, after the family.
const STYLE: &[&str] = &[
    "regular",
    "normal",
    "italic",
    "oblique",
    "bold",
    "semi-bold",
    "semibold",
    "demi-bold",
    "demibold",
    "extra-bold",
    "ultra-bold",
    "light",
    "extra-light",
    "ultra-light",
    "thin",
    "medium",
    "black",
    "heavy",
    "book",
    "condensed",
    "semi-condensed",
    "extra-condensed",
    "ultra-condensed",
    "expanded",
    "semi-expanded",
    "extra-expanded",
    "ultra-expanded",
    "small-caps",
];

/// The family part of a Pango font description: `"Cantarell 11"` →
/// `"Cantarell"`, `"Source Code Pro Semi-Bold Italic 10.5"` →
/// `"Source Code Pro"`. GTK/GNOME settings store fonts this way.
pub fn pango_family(desc: &str) -> Option<String> {
    let desc = desc.trim().trim_matches('\'').trim();
    let mut words: Vec<&str> = desc.split_whitespace().collect();
    // Trailing size, possibly with a `px` suffix.
    if let Some(last) = words.last()
        && last
            .trim_end_matches("px")
            .parse::<f32>()
            .is_ok_and(|n| n > 0.0)
    {
        words.pop();
    }
    // Trailing style words. Pango allows any number of them, in any order.
    while words.len() > 1
        && words
            .last()
            .is_some_and(|w| STYLE.contains(&w.to_ascii_lowercase().as_str()))
    {
        words.pop();
    }
    let family = words.join(" ").trim_end_matches(',').trim().to_owned();
    (!family.is_empty()).then_some(family)
}

/// Which buttons GNOME's `button-layout` shows, as (minimize, maximize).
/// The value looks like `appmenu:minimize,maximize,close` or `:close`.
pub fn button_layout(layout: &str) -> (bool, bool) {
    let has = |name: &str| {
        layout
            .split([':', ','])
            .any(|b| b.trim().eq_ignore_ascii_case(name))
    };
    (has("minimize"), has("maximize"))
}

const INTERFACE_NS: &str = "org.gnome.desktop.interface";
const WM_NS: &str = "org.gnome.desktop.wm.preferences";
/// The desktop-neutral namespace every portal backend serves: GNOME's
/// accent (`slate`, `teal`, …) arrives as one `(ddd)` sRGB triple;
/// out-of-range components mean "no accent set".
const APPEARANCE_NS: &str = "org.freedesktop.appearance";

/// The portal's `accent-color` as 8-bit sRGB, if the desktop has one.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // range-checked first
pub fn accent_from(rgb: (f64, f64, f64)) -> Option<[u8; 3]> {
    let (r, g, b) = rgb;
    let in_range = |v: f64| (0.0..=1.0).contains(&v);
    (in_range(r) && in_range(g) && in_range(b))
        .then(|| [r, g, b].map(|v| (v * 255.0).round() as u8))
}

fn settings_from(interface: &HashMap<String, String>, wm: &HashMap<String, String>) -> Settings {
    let (show_minimize, show_maximize) = match wm.get("button-layout") {
        Some(layout) => {
            let (min, max) = button_layout(layout);
            (Some(min), Some(max))
        }
        None => (None, None),
    };
    Settings {
        interface_font: interface.get("font-name").and_then(|f| pango_family(f)),
        monospace_font: interface
            .get("monospace-font-name")
            .and_then(|f| pango_family(f)),
        icon_theme: interface
            .get("icon-theme")
            .map(|t| t.trim().trim_matches('\'').to_owned())
            .filter(|t| !t.is_empty()),
        show_minimize,
        show_maximize,
        accent: None,
    }
}

/// `org.freedesktop.portal.Settings.ReadAll` for the GNOME namespaces
/// and the portal's own appearance namespace. xdg-desktop-portal-gtk /
/// -gnome expose the gsettings keys one-to-one.
fn read_portal() -> Option<Settings> {
    use zbus::zvariant::OwnedValue;
    let conn = zbus::blocking::Connection::session().ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadAll",
            &(&[INTERFACE_NS, WM_NS, APPEARANCE_NS][..],),
        )
        .ok()?;
    let all: HashMap<String, HashMap<String, OwnedValue>> = reply.body().deserialize().ok()?;
    let accent = all
        .get(APPEARANCE_NS)
        .and_then(|m| m.get("accent-color"))
        .and_then(|v| <(f64, f64, f64)>::try_from(v.clone()).ok())
        .and_then(accent_from);
    let strings = |ns: &str| -> HashMap<String, String> {
        all.get(ns)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| {
                        <&str>::try_from(&**v)
                            .ok()
                            .map(|s| (k.clone(), s.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let interface = strings(INTERFACE_NS);
    if interface.is_empty() {
        // The portal answered but does not serve GNOME's namespaces (KDE,
        // a bare compositor): only the accent, if there is one, to borrow.
        return accent.map(|a| Settings {
            accent: Some(a),
            ..Settings::default()
        });
    }
    Some(Settings {
        accent,
        ..settings_from(&interface, &strings(WM_NS))
    })
}

/// Fallback when there is no portal on the bus: the `gsettings` tool.
fn read_gsettings() -> Settings {
    let get = |schema: &str, key: &str| -> Option<String> {
        let out = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let mut interface = HashMap::new();
    for key in ["font-name", "monospace-font-name", "icon-theme"] {
        if let Some(v) = get(INTERFACE_NS, key) {
            interface.insert(key.to_owned(), v);
        }
    }
    let mut wm = HashMap::new();
    if let Some(v) = get(WM_NS, "button-layout") {
        wm.insert("button-layout".to_owned(), v);
    }
    settings_from(&interface, &wm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_desktops() {
        assert_eq!(Desktop::from_xdg("COSMIC"), Desktop::Cosmic);
        assert_eq!(Desktop::from_xdg("GNOME"), Desktop::Gnome);
        assert_eq!(Desktop::from_xdg("ubuntu:GNOME"), Desktop::Gnome);
        assert_eq!(Desktop::from_xdg("pop:GNOME"), Desktop::Gnome);
        assert_eq!(Desktop::from_xdg("GNOME-Classic:GNOME"), Desktop::Gnome);
        assert_eq!(Desktop::from_xdg("KDE"), Desktop::Other);
        assert_eq!(Desktop::from_xdg(""), Desktop::Other);
    }

    /// Needs a desktop session with a portal; change a font in the
    /// desktop's settings while it waits.
    #[test]
    #[ignore = "needs a live desktop session: change a font while it runs"]
    fn portal_signal_probe() {
        use cosmic::iced::futures::StreamExt;
        let _ = tracing_subscriber::fmt()
            .with_env_filter("attic=debug,zbus=debug")
            .with_test_writer()
            .try_init();
        let (tx, mut rx) = cosmic::iced::futures::channel::mpsc::channel(4);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            if let Err(err) = rt.block_on(follow(tx)) {
                eprintln!("follow: {err}");
            }
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let got = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(12), rx.next()).await
        });
        eprintln!("probe got: {got:?}");
        assert!(got.is_ok(), "no settings change arrived");
    }

    #[test]
    fn falls_back_through_session_variables() {
        assert_eq!(Desktop::from_env(["GNOME"]), Some(Desktop::Gnome));
        // XDG_CURRENT_DESKTOP wins over the older variables.
        assert_eq!(
            Desktop::from_env(["COSMIC", "gnome"]),
            Some(Desktop::Cosmic)
        );
        // An empty or foreign first variable falls through to the next.
        assert_eq!(Desktop::from_env(["", "cosmic"]), Some(Desktop::Cosmic));
        assert_eq!(Desktop::from_env(["X-Generic", "GNOME"]), Some(Desktop::Gnome));
        // Set but unrecognised: it is some other desktop, not unknown.
        assert_eq!(Desktop::from_env(["KDE"]), Some(Desktop::Other));
        // Nothing set at all: the caller has to look elsewhere.
        assert_eq!(Desktop::from_env([]), None);
    }

    #[test]
    fn strips_pango_size_and_style() {
        assert_eq!(pango_family("Cantarell 11").as_deref(), Some("Cantarell"));
        assert_eq!(pango_family("'Cantarell 11'").as_deref(), Some("Cantarell"));
        assert_eq!(
            pango_family("Source Code Pro 10").as_deref(),
            Some("Source Code Pro")
        );
        assert_eq!(
            pango_family("Adwaita Sans Semi-Bold Italic 10.5").as_deref(),
            Some("Adwaita Sans")
        );
        assert_eq!(pango_family("Noto Sans 12px").as_deref(), Some("Noto Sans"));
        assert_eq!(pango_family("Monospace").as_deref(), Some("Monospace"));
        // A family that is itself a style word survives.
        assert_eq!(pango_family("Light 12").as_deref(), Some("Light"));
        assert_eq!(pango_family("   "), None);
    }

    #[test]
    fn reads_accent_colour() {
        // GNOME's "slate" as the portal reports it.
        assert_eq!(
            accent_from((0.435_294_1, 0.513_725_5, 0.588_235_3)),
            Some([111, 131, 150])
        );
        assert_eq!(accent_from((0.0, 0.0, 0.0)), Some([0, 0, 0]));
        // The portal's "no accent" is any component out of range.
        assert_eq!(accent_from((-1.0, -1.0, -1.0)), None);
        assert_eq!(accent_from((0.5, 1.5, 0.5)), None);
    }

    #[test]
    fn reads_button_layout() {
        assert_eq!(
            button_layout("appmenu:minimize,maximize,close"),
            (true, true)
        );
        assert_eq!(button_layout(":close"), (false, false));
        assert_eq!(button_layout("close,maximize:"), (false, true));
        assert_eq!(button_layout("icon:minimize,close"), (true, false));
    }

    #[test]
    fn builds_settings_from_keys() {
        let mut interface = HashMap::new();
        interface.insert("font-name".to_owned(), "'Cantarell 11'".to_owned());
        interface.insert("icon-theme".to_owned(), "'Adwaita'".to_owned());
        let mut wm = HashMap::new();
        wm.insert("button-layout".to_owned(), "'appmenu:close'".to_owned());
        let s = settings_from(&interface, &wm);
        assert_eq!(s.interface_font.as_deref(), Some("Cantarell"));
        assert_eq!(s.monospace_font, None);
        assert_eq!(s.icon_theme.as_deref(), Some("Adwaita"));
        assert_eq!(s.show_minimize, Some(false));
        assert_eq!(s.show_maximize, Some(false));
    }
}
