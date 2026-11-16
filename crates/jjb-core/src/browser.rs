// SPDX-License-Identifier: GPL-3.0-only

//! Web capture through a real browser, for the pages a plain fetch cannot
//! read: those behind a login, and those that only fill in their text
//! with JavaScript.
//!
//! The browser is the Firefox already installed, run with a profile of
//! Attic's own (so the user's everyday Firefox is never touched) and
//! driven over Marionette, the remote-control server built into Firefox
//! (`--marionette`): length-prefixed JSON over a local TCP port. No
//! helper program is needed. The user logs in once in an ordinary window
//! on that profile (`login_window`); captures then run the same profile
//! headless and are already signed in.

use crate::capture::{self, Captured};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use url::Url;

/// How long Firefox gets to start and open its control port.
const START_TIMEOUT: Duration = Duration::from_secs(40);
/// How long one page may take to load.
const PAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// After the page has loaded, how long its scripts get to fill in the
/// text; the wait ends early once the text stops growing.
const SETTLE_MAX: Duration = Duration::from_secs(8);
/// A capture shorter than this is taken for a page whose text never
/// arrived, and is tried again through the browser.
const THIN_CHARS: usize = 500;
/// The file in the profile that lists the sites the user has logged in to.
const SITES_FILE: &str = "attic-sites.txt";

/// The Firefox to drive: `ATTIC_FIREFOX` if set, else `firefox` on `PATH`.
pub fn find_firefox() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ATTIC_FIREFOX").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) { "firefox.exe" } else { "firefox" };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

/// The site an address belongs to, for remembering logins: the host
/// without a leading `www.`.
pub fn site_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()?
        .host_str()
        .map(|h| h.trim_start_matches("www.").to_lowercase())
}

/// The sites the user has logged in to with Attic's Firefox profile.
pub fn known_sites(profile: &Path) -> Vec<String> {
    std::fs::read_to_string(profile.join(SITES_FILE))
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Note that `url`'s site has been logged in to: its captures go through
/// the browser from now on.
pub fn remember_site(profile: &Path, url: &str) -> Result<()> {
    let Some(site) = site_of(url) else {
        return Ok(());
    };
    let mut sites = known_sites(profile);
    if sites.contains(&site) {
        return Ok(());
    }
    sites.push(site);
    std::fs::create_dir_all(profile).context("creating the browser profile")?;
    std::fs::write(profile.join(SITES_FILE), sites.join("\n") + "\n")
        .context("remembering the site")
}

/// The settings every launch writes into the profile: no first-run
/// pages, no default-browser question, and windows and their session
/// cookies restored, so a login that only lasts "until the browser
/// closes" survives the login window being closed.
fn write_prefs(profile: &Path, marionette_port: Option<u16>) -> Result<()> {
    std::fs::create_dir_all(profile).context("creating the browser profile")?;
    let mut prefs = String::from(
        "user_pref(\"browser.shell.checkDefaultBrowser\", false);\n\
         user_pref(\"browser.aboutwelcome.enabled\", false);\n\
         user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\n\
         user_pref(\"startup.homepage_welcome_url\", \"\");\n\
         user_pref(\"startup.homepage_welcome_url.additional\", \"\");\n\
         user_pref(\"datareporting.policy.dataSubmissionPolicyBypassNotification\", true);\n\
         user_pref(\"toolkit.telemetry.reportingpolicy.firstRun\", false);\n\
         user_pref(\"browser.startup.page\", 3);\n\
         user_pref(\"browser.sessionstore.resume_from_crash\", true);\n\
         user_pref(\"browser.tabs.warnOnClose\", false);\n\
         user_pref(\"browser.warnOnQuit\", false);\n",
    );
    if let Some(port) = marionette_port {
        prefs.push_str(&format!("user_pref(\"marionette.port\", {port});\n"));
    }
    std::fs::write(profile.join("user.js"), prefs).context("writing the browser settings")
}

/// Open an ordinary Firefox window on Attic's profile at `url`, for the
/// user to log in, and wait until they close it. Blocking.
pub fn login_window(firefox: &Path, profile: &Path, url: &str) -> Result<()> {
    write_prefs(profile, None)?;
    let status = Command::new(firefox)
        .arg("--no-remote")
        .arg("--profile")
        .arg(profile)
        .arg("--new-window")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("starting Firefox")?;
    if !status.success() {
        bail!("Firefox closed with an error ({status})");
    }
    remember_site(profile, url)
}

/// A headless Firefox on Attic's profile, under remote control.
pub struct Firefox {
    child: Child,
    sock: TcpStream,
    pending: Vec<u8>,
    next_id: u64,
}

impl Firefox {
    /// Start it and open a session. Fails if the profile is in use: the
    /// login window is still open.
    pub fn launch(firefox: &Path, profile: &Path) -> Result<Self> {
        // A free port, found by asking the system for one and letting go.
        let port = TcpListener::bind(("127.0.0.1", 0))
            .and_then(|l| l.local_addr())
            .context("finding a free port")?
            .port();
        write_prefs(profile, Some(port))?;
        let mut child = Command::new(firefox)
            .arg("--headless")
            .arg("--no-remote")
            .arg("--marionette")
            .arg("--profile")
            .arg(profile)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting Firefox")?;
        let started = Instant::now();
        let sock = loop {
            if let Ok(sock) = TcpStream::connect(("127.0.0.1", port)) {
                break sock;
            }
            if let Ok(Some(status)) = child.try_wait() {
                bail!(
                    "Firefox would not start ({status}); if its login window is still open, close it first"
                );
            }
            if started.elapsed() > START_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Firefox did not answer in time");
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        sock.set_read_timeout(Some(PAGE_TIMEOUT + Duration::from_secs(10)))
            .context("setting up the connection")?;
        let mut firefox = Firefox {
            child,
            sock,
            pending: Vec::new(),
            next_id: 0,
        };
        // Firefox greets first, then takes commands.
        firefox.read_packet().context("no greeting from Firefox")?;
        firefox.command(
            "WebDriver:NewSession",
            json!({ "capabilities": { "alwaysMatch": {
                "pageLoadStrategy": "normal",
                "timeouts": { "pageLoad": PAGE_TIMEOUT.as_millis() as u64 },
            } } }),
        )?;
        Ok(firefox)
    }

    /// One packet: `<length>:<json>`.
    fn read_packet(&mut self) -> Result<Value> {
        let mut chunk = [0u8; 65536];
        let colon = loop {
            if let Some(i) = self.pending.iter().position(|b| *b == b':') {
                break i;
            }
            let n = self.sock.read(&mut chunk).context("reading from Firefox")?;
            if n == 0 {
                bail!("Firefox closed the connection");
            }
            self.pending.extend_from_slice(&chunk[..n]);
        };
        let len: usize = std::str::from_utf8(&self.pending[..colon])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("a garbled answer from Firefox"))?;
        while self.pending.len() < colon + 1 + len {
            let n = self.sock.read(&mut chunk).context("reading from Firefox")?;
            if n == 0 {
                bail!("Firefox closed the connection");
            }
            self.pending.extend_from_slice(&chunk[..n]);
        }
        let body: Vec<u8> = self.pending.drain(..colon + 1 + len).skip(colon + 1).collect();
        serde_json::from_slice(&body).context("a garbled answer from Firefox")
    }

    /// Send `[0, id, name, params]`, wait for `[1, id, error, result]`.
    fn command(&mut self, name: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let body = serde_json::to_vec(&json!([0, self.next_id, name, params]))?;
        self.sock
            .write_all(format!("{}:", body.len()).as_bytes())
            .and_then(|()| self.sock.write_all(&body))
            .context("talking to Firefox")?;
        loop {
            let reply = self.read_packet()?;
            if reply.get(1).and_then(Value::as_u64) != Some(self.next_id) {
                continue;
            }
            if let Some(err) = reply.get(2).filter(|e| !e.is_null()) {
                let what = err
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| err.get("error").and_then(Value::as_str))
                    .unwrap_or("an error");
                bail!("Firefox: {what}");
            }
            return Ok(reply.get(3).cloned().unwrap_or(Value::Null));
        }
    }

    /// How much text the page shows right now.
    fn text_length(&mut self) -> usize {
        self.command(
            "WebDriver:ExecuteScript",
            json!({
                "script": "return document.body ? document.body.innerText.length : 0;",
                "args": [],
            }),
        )
        .ok()
        .and_then(|v| v.get("value").and_then(Value::as_u64))
        .unwrap_or(0) as usize
    }

    /// Open `url`, give its scripts time to put the text in, and return
    /// the page as it then stands.
    pub fn page_html(&mut self, url: &str) -> Result<String> {
        self.command("WebDriver:Navigate", json!({ "url": url }))?;
        // Loaded is not finished: wait until the text has stopped
        // growing for two looks in a row.
        let started = Instant::now();
        let mut last = self.text_length();
        let mut steady = 0;
        while started.elapsed() < SETTLE_MAX && steady < 2 {
            std::thread::sleep(Duration::from_millis(400));
            let now = self.text_length();
            steady = if now == last && now > 0 { steady + 1 } else { 0 };
            last = now;
        }
        let source = self.command("WebDriver:GetPageSource", json!({}))?;
        source
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("Firefox returned no page"))
    }

    /// Close Firefox the polite way, so it writes its cookies and
    /// session down; kill it if it will not go.
    pub fn quit(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        let _ = self.sock.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = self.command("Marionette:Quit", json!({ "flags": ["eAttemptQuit"] }));
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Firefox {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.shut_down();
        }
    }
}

/// Which way a capture went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Plain,
    Browser,
}

/// Capture `url` the best way there is.
///
/// The plain fetch goes first, unless `browser_first` (a site the user
/// has logged in to: its plain page is at best a teaser). The browser is
/// tried when the plain fetch met a login wall or a refusal (many sites
/// refuse anything that is not a browser), found no text, or came back
/// thin, which is how a page built by JavaScript looks from outside.
/// Whichever read more wins. `browser` is `None` when there is no
/// Firefox to use.
pub fn capture_with_fallback(
    url: &str,
    plain: &mut dyn FnMut(&str) -> Result<String>,
    browser: Option<&mut dyn FnMut(&str) -> Result<String>>,
    browser_first: bool,
    progress: &mut dyn FnMut(usize, Option<usize>),
    pause: Duration,
) -> Result<(Captured, Route)> {
    let Some(browser) = browser else {
        return capture::capture(url, plain, progress, pause).map(|c| (c, Route::Plain));
    };
    if browser_first {
        return capture::capture(url, browser, progress, pause).map(|c| (c, Route::Browser));
    }
    let first = capture::capture(url, plain, progress, pause);
    let thin = |c: &Captured| c.needs_login || c.markdown.chars().count() < THIN_CHARS;
    match first {
        Ok(c) if !thin(&c) => Ok((c, Route::Plain)),
        first => {
            let second = capture::capture(url, browser, progress, pause);
            match (first, second) {
                (_, Ok(b)) if !thin(&b) => Ok((b, Route::Browser)),
                // Neither read much: the one that got some text, or
                // failing that the browser's word on a login wall.
                (Ok(a), Ok(b)) => {
                    if !a.needs_login && a.markdown.len() >= b.markdown.len() {
                        Ok((a, Route::Plain))
                    } else {
                        Ok((b, Route::Browser))
                    }
                }
                (Ok(a), Err(_)) => Ok((a, Route::Plain)),
                (Err(_), Ok(b)) => Ok((b, Route::Browser)),
                (Err(a), Err(_)) => Err(a),
            }
        }
    }
}

/// `capture_with_fallback` with the real fetchers: `fetch_html`, and a
/// headless Firefox on `profile` that is only started if it is needed
/// and is closed when the capture ends. Blocking.
pub fn capture_auto(
    url: &str,
    profile: &Path,
    force_browser: bool,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<(Captured, Route)> {
    let firefox = find_firefox();
    let browser_first = force_browser
        || site_of(url).is_some_and(|site| known_sites(profile).contains(&site));
    let mut session: Option<Firefox> = None;
    let mut browser_fetch = |u: &str| -> Result<String> {
        if session.is_none() {
            let path = firefox.as_deref().ok_or_else(|| anyhow!("Firefox is not installed"))?;
            session = Some(Firefox::launch(path, profile)?);
        }
        session.as_mut().expect("just started").page_html(u)
    };
    let result = capture_with_fallback(
        url,
        &mut |u| capture::fetch_html(u),
        // The offline fixtures stand in for the network; a browser
        // would go looking for the real thing.
        (firefox.is_some() && std::env::var_os("JJB_CAPTURE_FIXTURE").is_none())
            .then_some(&mut browser_fetch as &mut dyn FnMut(&str) -> Result<String>),
        browser_first,
        progress,
        capture::PAUSE,
    );
    if let Some(session) = session {
        session.quit();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(text: &str) -> String {
        let para = format!("<p>{text} {}</p>", "It goes on at some length, as articles do. ".repeat(12));
        format!("<html><head><title>T</title></head><body><article><h1>T</h1>{para}{para}</article></body></html>")
    }

    const WALL: &str = "<html><head><title>Sign in</title></head><body><form>\
                        <input type=\"password\"></form></body></html>";
    const SHELL: &str = "<html><head><title>App</title></head><body><div id=\"root\"></div>\
                         <script src=\"app.js\"></script></body></html>";

    fn run(
        plain: &str,
        browser: Option<&str>,
        browser_first: bool,
    ) -> (Captured, Route, usize, usize) {
        let (mut plain_calls, mut browser_calls) = (0, 0);
        let mut plain_fetch = |_: &str| {
            plain_calls += 1;
            Ok(plain.to_owned())
        };
        let mut browser_fetch = |_: &str| {
            browser_calls += 1;
            Ok(browser.unwrap_or_default().to_owned())
        };
        let (c, route) = capture_with_fallback(
            "https://x.example/a",
            &mut plain_fetch,
            browser.is_some().then_some(&mut browser_fetch as &mut dyn FnMut(&str) -> Result<String>),
            browser_first,
            &mut |_, _| {},
            Duration::ZERO,
        )
        .unwrap();
        (c, route, plain_calls, browser_calls)
    }

    #[test]
    fn the_browser_is_only_used_when_it_is_needed() {
        // A page that reads fine never starts the browser.
        let (c, route, plain, browser) = run(&article("Public text."), Some(&article("x")), false);
        assert_eq!((route, plain, browser), (Route::Plain, 1, 0));
        assert!(c.markdown.contains("Public text."));
        // A login wall, or an empty JavaScript shell, goes to the browser.
        let (c, route, _, browser) = run(WALL, Some(&article("Members' text.")), false);
        assert_eq!((route, browser), (Route::Browser, 1));
        assert!(c.markdown.contains("Members' text.") && !c.needs_login);
        let (c, route, ..) = run(SHELL, Some(&article("Rendered text.")), false);
        assert_eq!(route, Route::Browser);
        assert!(c.markdown.contains("Rendered text."));
        // The browser hits the wall too: the capture says a login is wanted.
        let (c, ..) = run(WALL, Some(WALL), false);
        assert!(c.needs_login);
        // A site that has been logged in to skips the plain fetch: its
        // public page would only be the teaser.
        let (c, route, plain, _) = run(&article("Teaser."), Some(&article("Whole article.")), true);
        assert_eq!((route, plain), (Route::Browser, 0));
        assert!(c.markdown.contains("Whole article."));
        // No Firefox: the plain result stands, wall and all.
        let (c, route, ..) = run(WALL, None, false);
        assert!(c.needs_login && route == Route::Plain);
    }

    #[test]
    fn logged_in_sites_are_remembered_with_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile");
        assert!(known_sites(&profile).is_empty());
        remember_site(&profile, "https://www.Paid.example/article/1").unwrap();
        remember_site(&profile, "https://paid.example/other").unwrap();
        remember_site(&profile, "https://second.example/").unwrap();
        assert_eq!(known_sites(&profile), vec!["paid.example", "second.example"]);
    }

    /// Not a test: `JJB_BROWSER_PROBE=<address> cargo test -p jjb-core
    /// browser_probe -- --ignored --nocapture` reads a real page through
    /// headless Firefox on a scratch profile (`JJB_BROWSER_PROFILE` to
    /// use a particular one) and prints how much text it found.
    #[test]
    #[ignore]
    fn browser_probe() {
        let Ok(url) = std::env::var("JJB_BROWSER_PROBE") else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let profile = std::env::var_os("JJB_BROWSER_PROFILE")
            .map_or_else(|| scratch.path().join("profile"), PathBuf::from);
        let started = Instant::now();
        match capture_auto(&url, &profile, true, &mut |n, of| eprintln!("page {n} of {of:?}")) {
            Ok((c, route)) => eprintln!(
                "{route:?} in {:.1}s: title={:?} pages={} chars={} needs_login={}\n{}",
                started.elapsed().as_secs_f32(),
                c.title,
                c.pages,
                c.markdown.chars().count(),
                c.needs_login,
                c.markdown.chars().take(300).collect::<String>()
            ),
            Err(err) => eprintln!("FAILED: {err:#}"),
        }
    }
}
