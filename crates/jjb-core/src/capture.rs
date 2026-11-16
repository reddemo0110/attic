// SPDX-License-Identifier: GPL-3.0-only

//! Web capture: fetch a public page, keep the article and drop the rest
//! (the way a browser's reader mode does), turn it into markdown, and
//! follow "page 2 of 5" style pagination so a split article comes back
//! as one text.
//!
//! Everything but `fetch_html` is pure: `capture` is handed its fetcher,
//! so the tests serve pages from memory and a browser-driven fetcher (for
//! pages behind a login) can stand in for the plain one.

use anyhow::{Context, Result, bail};
use dom_query::Document;
use std::collections::HashSet;
use std::time::Duration;
use url::Url;

/// The most pages one address is followed through.
pub const MAX_PAGES: usize = 30;
/// Wait between the pages of one article, to go easy on the site.
pub const PAUSE: Duration = Duration::from_millis(700);

const MAX_HTML: u64 = 4_000_000;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux) Attic/0.2 capture";
/// Under this much text, a page with a password box is taken for a login
/// wall rather than an article that happens to have a sign-in form.
const LOGIN_WALL_CHARS: usize = 1500;

/// The server answered with an error status.
#[derive(Debug)]
pub struct HttpStatus(pub u16);

impl std::fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the site answered {}", self.0)
    }
}

impl std::error::Error for HttpStatus {}

/// One fetched page, cleaned.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Page {
    pub title: String,
    pub byline: Option<String>,
    pub site: Option<String>,
    /// The article as markdown; empty when nothing readable was found.
    pub markdown: String,
    /// The address of the next page of the same article, if there is one.
    pub next: Option<String>,
    /// "Page 2 of 5" gives 5.
    pub total: Option<usize>,
    /// The page shows a sign-in form in place of the text.
    pub needs_login: bool,
    /// The page is only a signpost (`<meta http-equiv="refresh">`): the
    /// real one is here.
    pub redirect: Option<String>,
}

/// A whole capture: every page of one address, joined.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Captured {
    pub url: String,
    pub title: String,
    pub byline: Option<String>,
    pub site: Option<String>,
    pub markdown: String,
    pub pages: usize,
    /// Nothing was captured because the site wants a login first.
    pub needs_login: bool,
    /// Why the capture stopped early (page cap, a page that failed), if
    /// it did; what was read up to then is kept.
    pub stopped: Option<String>,
}

/// The addresses in a pasted block of text: one per line or separated by
/// spaces, `https://` assumed when missing, repeats dropped, order kept.
pub fn parse_urls(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c| matches!(c, ',' | ';' | '<' | '>' | '"' | '\''));
        if word.is_empty() {
            continue;
        }
        let candidate = if word.starts_with("http://") || word.starts_with("https://") {
            word.to_owned()
        } else if word.contains('.') && !word.contains("://") {
            format!("https://{word}")
        } else {
            continue;
        };
        let Ok(url) = Url::parse(&candidate) else {
            continue;
        };
        if url.host_str().is_none_or(|h| !h.contains('.')) {
            continue;
        }
        let url = url.to_string();
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Fetch a page's HTML. Blocking: run on a worker thread.
///
/// `JJB_CAPTURE_FIXTURE=/dir` serves files from that directory instead of
/// going online (the headless harness has no network): the address after
/// the host, with everything but letters, digits, `.` and `-` turned into
/// `_`, is the file name (`https://x.test/story.html?page=2` reads
/// `story.html_page_2`; the bare host reads `index.html`).
pub fn fetch_html(url: &str) -> Result<String> {
    if let Some(dir) = std::env::var_os("JJB_CAPTURE_FIXTURE") {
        let parsed = Url::parse(url).context("reading the address")?;
        let mut name = parsed.path().to_owned();
        if let Some(q) = parsed.query() {
            name.push('?');
            name.push_str(q);
        }
        let name: String = name
            .trim_matches('/')
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-') { c } else { '_' })
            .collect();
        let name = if name.is_empty() { "index.html".to_owned() } else { name };
        let path = std::path::Path::new(&dir).join(name);
        return std::fs::read_to_string(&path)
            .with_context(|| format!("no fixture at {}", path.display()));
    }
    let mut resp = match ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "text/html,application/xhtml+xml")
        .call()
    {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(code)) => return Err(HttpStatus(code).into()),
        Err(err) => return Err(err).with_context(|| format!("fetching {url}")),
    };
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !(ct.contains("html") || ct.contains("xml") || ct.is_empty()) {
        bail!("not a web page ({ct})");
    }
    resp.body_mut()
        .with_config()
        .limit(MAX_HTML)
        .read_to_string()
        .context("reading the page")
}

/// Capture one address: fetch it, follow its pages, join them.
///
/// `fetch` gets each page's HTML; `progress` hears `(page, of)` before
/// each fetch (`of` once the site has said how many there are). `pause`
/// is waited between pages.
pub fn capture(
    url: &str,
    fetch: &mut dyn FnMut(&str) -> Result<String>,
    progress: &mut dyn FnMut(usize, Option<usize>),
    pause: Duration,
) -> Result<Captured> {
    let first = clean_url(Url::parse(url).context("reading the address")?);
    let mut out = Captured {
        url: first.to_string(),
        ..Captured::default()
    };
    let mut current = first.clone();
    let mut visited: HashSet<String> = HashSet::from([first.to_string()]);
    let mut seen_text: HashSet<String> = HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    let mut total = None;
    let mut hops = 0;
    loop {
        let n = parts.len() + 1;
        progress(n, total);
        let page = fetch(current.as_str()).and_then(|html| extract(&html, &current, &first));
        // A signpost page: go where it points, a few times at most.
        if let Ok(Page {
            redirect: Some(to), ..
        }) = &page
            && hops < 3
            && let Ok(to) = Url::parse(to)
            && visited.insert(to.to_string())
        {
            hops += 1;
            current = to;
            continue;
        }
        let page = match page {
            Ok(page) => page,
            // A site that answers "not allowed" to the first page wants a
            // login (or a subscription) before it shows anything.
            Err(err)
                if n == 1
                    && err
                        .downcast_ref::<HttpStatus>()
                        .is_some_and(|s| matches!(s.0, 401 | 402 | 403)) =>
            {
                out.needs_login = true;
                return Ok(out);
            }
            Err(err) if n == 1 => return Err(err),
            Err(err) => {
                out.stopped = Some(format!("page {n} could not be read: {err:#}"));
                break;
            }
        };
        if n == 1 {
            out.title = page.title.clone();
            out.byline = page.byline.clone();
            out.site = page.site.clone();
            if page.needs_login {
                out.needs_login = true;
                return Ok(out);
            }
            if page.markdown.is_empty() {
                bail!("no readable text on the page");
            }
        }
        // The same text again means the site looped us back.
        if page.markdown.is_empty() || !seen_text.insert(page.markdown.clone()) {
            break;
        }
        total = total.or(page.total);
        parts.push(if n == 1 {
            page.markdown
        } else {
            without_title(&page.markdown, &out.title)
        });
        let Some(next) = page.next.and_then(|n| Url::parse(&n).ok()) else {
            break;
        };
        if !visited.insert(next.to_string()) {
            break;
        }
        if parts.len() >= MAX_PAGES {
            out.stopped = Some(format!("stopped after {MAX_PAGES} pages; there are more"));
            break;
        }
        current = next;
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }
    out.pages = parts.len();
    out.markdown = parts.join("\n\n");
    Ok(out)
}

/// The note a capture becomes: the title, the source as a link card, a
/// line saying when and what, the folder tag if it was started in one,
/// then the text.
pub fn note_body(c: &Captured, date: &str, tag: Option<&str>) -> String {
    let title = if c.title.trim().is_empty() {
        host_of(&c.url)
    } else {
        c.title.trim().to_owned()
    };
    let label = host_of(&c.url);
    let mut facts = vec![format!("Captured {date}")];
    if let Some(by) = c.byline.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        facts.push(by.to_owned());
    }
    if c.pages > 1 {
        facts.push(format!("{} pages", c.pages));
    }
    if let Some(why) = &c.stopped {
        facts.push(why.clone());
    }
    let mut body = format!(
        "# {}\n\n[{}]({})\n*{}*\n",
        one_line(&title),
        one_line(&label).replace(['[', ']'], ""),
        c.url,
        one_line(&facts.join(" · ")).replace('*', "")
    );
    if let Some(tag) = tag {
        body.push_str(&format!("\n#{tag}\n"));
    }
    body.push('\n');
    body.push_str(without_title(&c.markdown, &title).trim());
    body.push('\n');
    body
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_owned()))
        .unwrap_or_else(|| url.to_owned())
}

/// The address without its `#fragment`.
fn clean_url(mut url: Url) -> Url {
    url.set_fragment(None);
    url
}

/// Clean one page: the article as markdown, and where it continues.
/// `first` is the address the capture started from (the same as `url` on
/// page one); "next" links are only trusted when they stay inside it.
pub fn extract(html: &str, url: &Url, first: &Url) -> Result<Page> {
    let doc = Document::from(html);
    let (page_no, total) = page_of(&doc.select("body").text())
        .map_or((None, None), |(n, m)| (Some(n), Some(m)));
    let next = next_link(&doc, url, first, page_no);
    let has_password = doc.select("input[type=password]").exists();
    let redirect = meta_refresh(&doc, url);

    let cfg = dom_smoothie::Config::default();
    let article = dom_smoothie::Readability::new(html, Some(url.as_str()), Some(cfg))
        .ok()
        .and_then(|mut r| r.parse().ok());
    let Some(article) = article else {
        return Ok(Page {
            needs_login: has_password,
            next,
            total,
            redirect,
            ..Page::default()
        });
    };
    let markdown = to_markdown(&article.content);
    let text_len = markdown.chars().count();
    let needs_login = has_password && text_len < LOGIN_WALL_CHARS;
    Ok(Page {
        title: one_line(&article.title),
        byline: article.byline.map(|b| one_line(&b)).filter(|b| !b.is_empty()),
        site: article.site_name.map(|s| one_line(&s)).filter(|s| !s.is_empty()),
        markdown,
        next,
        total,
        needs_login,
        // A real article is not a signpost, whatever its header says.
        redirect: redirect.filter(|_| text_len < LOGIN_WALL_CHARS),
    })
}

/// `<meta http-equiv="refresh" content="0; url=…">` with a short delay.
fn meta_refresh(doc: &Document, url: &Url) -> Option<String> {
    doc.select("meta[http-equiv]").iter().find_map(|meta| {
        if !meta.attr("http-equiv")?.eq_ignore_ascii_case("refresh") {
            return None;
        }
        let content = meta.attr("content")?;
        let (delay, rest) = content.split_once(';')?;
        if delay.trim().parse::<f32>().ok()? > 5.0 {
            return None;
        }
        let rest = rest.trim();
        let target = rest
            .get(..4)
            .filter(|p| p.eq_ignore_ascii_case("url="))
            .map_or(rest, |_| &rest[4..])
            .trim_matches(|c| matches!(c, '"' | '\'' | ' '));
        let to = clean_url(url.join(target).ok()?);
        (to != *url).then(|| to.to_string())
    })
}

/// Article HTML → markdown, text only: pictures, media, forms and
/// scripts are left out.
fn to_markdown(html: &str) -> String {
    // Out before converting: links that only jump around the page (a
    // table of contents keeps its words, a footnote mark like [6] goes
    // altogether), Wikipedia's [edit] links, and link tooltips.
    let doc = Document::from(html);
    doc.select(".mw-editsection, sup.reference, sup.noprint").remove();
    for a in doc.select("a[href]").iter() {
        a.remove_attr("title");
        if !a.attr("href").is_some_and(|h| h.trim().starts_with('#')) {
            continue;
        }
        let text = a.text();
        let mark = text.trim().trim_matches(|c| matches!(c, '[' | ']' | '^' | '↑' | '↩'));
        if mark.is_empty() || mark.chars().all(|c| c.is_ascii_digit()) {
            a.remove();
        } else {
            a.remove_attr("href");
        }
    }
    let html = doc.select("body").inner_html();
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "noscript", "img", "picture", "svg", "iframe", "video", "audio",
            "canvas", "form", "button", "input", "select", "textarea",
        ])
        .options(htmd::options::Options {
            bullet_list_marker: htmd::options::BulletListMarker::Dash,
            ul_bullet_spacing: 1,
            ol_number_spacing: 1,
            hr_style: htmd::options::HrStyle::Dashes,
            ..Default::default()
        })
        .build();
    let md = converter.convert(&html).unwrap_or_default();
    tidy(&md)
}

/// Markdown as a note wants it: no runs of blank lines, no trailing
/// spaces, no links left empty by a dropped picture, and no `#word` that
/// the note would take for one of its own tags.
fn tidy(md: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut blank = true;
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if in_fence {
            out.push(line.to_owned());
            blank = false;
            continue;
        }
        // A hard line break is two trailing spaces; keep the text, drop
        // the break, since the lines of a paragraph stay their own lines.
        let line = line.trim_end();
        let line = empty_links_removed(line);
        if line.trim().is_empty() {
            if !blank {
                out.push(String::new());
            }
            blank = true;
            continue;
        }
        blank = false;
        out.push(untagged(&line));
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

/// `[](address)` — a link whose only content was a picture.
fn empty_links_removed(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("[](") {
        // The address may hold brackets of its own: find the one that
        // closes the link.
        let mut depth = 0;
        let close = rest[i + 2..].char_indices().find_map(|(j, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(i + 2 + j)
        });
        let Some(close) = close else {
            break;
        };
        out.push_str(&rest[..i]);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Put a backslash before a `#` that the note format would read as a
/// tag (`#word` at the start of a line or after a space or bracket).
/// Headings (`# `, `## `) and code spans are left alone.
fn untagged(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len() + 4);
    let mut in_code = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == '`' {
            in_code = !in_code;
        }
        let starts_tag = c == '#'
            && !in_code
            && (i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '{'))
            // Not the `#section` of a link's address: `](#…`.
            && !(i >= 2 && chars[i - 1] == '(' && chars[i - 2] == ']')
            && chars
                .get(i + 1)
                .is_some_and(|n| n.is_alphanumeric() || *n == '_');
        if starts_tag {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `markdown` without a leading heading that only repeats the title.
fn without_title(markdown: &str, title: &str) -> String {
    let mut lines = markdown.lines();
    let Some(first) = lines.by_ref().find(|l| !l.trim().is_empty()) else {
        return String::new();
    };
    let heading = first.trim_start_matches('#').trim();
    let same = first.trim_start().starts_with('#')
        && one_line(heading).eq_ignore_ascii_case(&one_line(title));
    let rest: Vec<&str> = lines.collect();
    if same {
        rest.join("\n").trim_start_matches('\n').to_owned()
    } else {
        std::iter::once(first)
            .chain(rest)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// "Page 2 of 5" anywhere in the page's text → `(2, 5)`.
fn page_of(text: &str) -> Option<(usize, usize)> {
    let lower = text.to_lowercase();
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("page") {
        rest = &rest[i + 4..];
        let mut words = rest.split_whitespace();
        let (Some(n), Some(of), Some(m)) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        let digits = |w: &str| {
            w.trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<usize>()
                .ok()
        };
        if matches!(of, "of" | "/")
            && let (Some(n), Some(m)) = (digits(n), digits(m))
            && n >= 1
            && n <= m
            && m <= 500
        {
            return Some((n, m));
        }
    }
    None
}

/// What a "next page" link says once case, spaces and arrows are gone.
fn next_label(text: &str) -> bool {
    let arrows = |c: char| matches!(c, '›' | '»' | '→' | '>' | '▶' | '⟩');
    let t = text.trim();
    if t.is_empty() || t.chars().count() > 24 {
        return false;
    }
    let words: String = t
        .to_lowercase()
        .chars()
        .filter(|c| !arrows(*c) && !c.is_whitespace())
        .collect();
    match words.as_str() {
        // Only arrows: "›", "»", ">>".
        "" => t.chars().all(|c| arrows(c) || c.is_whitespace()),
        "next" | "nextpage" => true,
        _ => false,
    }
}

/// The link to the next page of this article, if the page has one.
///
/// Tried in order of trust: the page's own `rel="next"`, a link that says
/// "Next" (or wears a `next` class), then the number after the current
/// one. Every candidate must stay inside the article (`same_article`):
/// on a blog "next" is very often the next post, which is not ours.
fn next_link(doc: &Document, current: &Url, first: &Url, page_no: Option<usize>) -> Option<String> {
    let resolve = |href: &str| -> Option<Url> {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") {
            return None;
        }
        let url = clean_url(current.join(href).ok()?);
        (url != *current && same_article(first, current, &url)).then_some(url)
    };
    let mut rel: Vec<Url> = Vec::new();
    let mut labelled: Vec<Url> = Vec::new();
    let mut numbered: Vec<(usize, Url)> = Vec::new();
    for link in doc.select("link[rel], a[href]").iter() {
        let Some(url) = link.attr("href").and_then(|h| resolve(&h)) else {
            continue;
        };
        let has_rel_next = link
            .attr("rel")
            .is_some_and(|r| r.split_whitespace().any(|w| w.eq_ignore_ascii_case("next")));
        if has_rel_next {
            rel.push(url);
            continue;
        }
        if link.is("link") {
            continue;
        }
        let text = link.text();
        let says_next = next_label(&text)
            || link.attr("aria-label").is_some_and(|l| next_label(&l))
            || link.attr("title").is_some_and(|l| next_label(&l))
            || link.attr("class").is_some_and(|c| {
                c.split_whitespace().any(|w| {
                    let w = w.to_lowercase();
                    w == "next" || w.starts_with("next-") || w.ends_with("-next") || w.ends_with("__next")
                })
            });
        if says_next {
            labelled.push(url);
        } else if let Ok(n) = text.trim().parse::<usize>() {
            numbered.push((n, url));
        }
    }
    // The page we are on: said in so many words, or read off the address.
    let here = page_no.or_else(|| page_in_url(current)).unwrap_or(1);
    rel.into_iter()
        .next()
        .or_else(|| labelled.into_iter().next())
        .or_else(|| {
            numbered
                .into_iter()
                .find(|(n, url)| *n == here + 1 && (here > 1 || page_in_url(url) == Some(2)))
                .map(|(_, url)| url)
        })
        .map(|u| u.to_string())
}

/// The page number an address carries: `?page=3`, `/page/3`, `/3`.
fn page_in_url(url: &Url) -> Option<usize> {
    let from_query = url.query_pairs().find_map(|(k, v)| {
        matches!(k.to_lowercase().as_str(), "page" | "p" | "pg" | "paged" | "pagenum")
            .then(|| v.parse::<usize>().ok())
            .flatten()
    });
    from_query.or_else(|| {
        let last = url.path().trim_end_matches('/').rsplit('/').next()?;
        let digits = last
            .trim_end_matches(".html")
            .trim_end_matches(".htm")
            .trim_start_matches("page-")
            .trim_start_matches("page");
        digits.parse::<usize>().ok().filter(|n| (1..=500).contains(n))
    })
}

/// Is `candidate` another page of the article that started at `first`
/// (and that we are reading at `current`)? Same site, and one of:
/// the same path with a different query; a path that extends the first
/// page's (`/story` → `/story/2`, `/story.html` → `/story-2.html`); or
/// the current address with exactly one number in it counted up.
fn same_article(first: &Url, current: &Url, candidate: &Url) -> bool {
    let host = |u: &Url| u.host_str().map(|h| h.trim_start_matches("www.").to_lowercase());
    if host(first) != host(candidate) {
        return false;
    }
    if candidate.path() == first.path() || candidate.path() == current.path() {
        return candidate.query() != current.query();
    }
    let stem = |p: &str| {
        let p = p.trim_end_matches('/');
        p.rsplit_once('.')
            .filter(|(_, ext)| ext.len() <= 5 && !ext.contains('/'))
            .map_or(p, |(base, _)| base)
            .to_owned()
    };
    // `page-2`, `p2`, `page/2`: a page marker and nothing else.
    let marker = |s: &str, must_say_page: bool| {
        let s = s.to_lowercase();
        let s = s.trim_matches(['/', '-', '_', '.']);
        let word = s.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, '/' | '-' | '_'));
        s.len() > word.len()
            && matches!(word, "" | "page" | "p" | "pg" | "part")
            && !(must_say_page && word.is_empty())
    };
    let base = stem(first.path());
    // What follows the first page's path must be a page marker, not a
    // longer slug (`/story` vs `/story-of-my-life`). From the site's
    // front page it must say "page": `/2` there could be anything.
    if let Some(tail) = candidate.path().strip_prefix(base.as_str())
        && marker(&stem(tail), base.len() <= 1)
    {
        return true;
    }
    // A sibling file in the same folder that is named as a page
    // (`…/index.html` → `…/page-2.html`).
    let dir_file = |p: &str| {
        let (dir, file) = p.rsplit_once('/').unwrap_or(("", p));
        (dir.to_owned(), stem(file))
    };
    let (first_dir, _) = dir_file(first.path());
    let (cand_dir, cand_file) = dir_file(candidate.path());
    if first_dir == cand_dir && marker(&cand_file, true) {
        return true;
    }
    counted_up(current, candidate)
}

/// The two addresses are the same but for one number, which went up,
/// and that number is a page number: the last one in the address, and
/// either named as one (`page-3`, `?p=3`, `start=40`) or small and up by
/// exactly one (`story-2.html` → `story-3.html`). A bigger unnamed
/// number counting up is an id — the next thread, the next article.
fn counted_up(current: &Url, candidate: &Url) -> bool {
    // (address with each number replaced by `#`, the numbers, the word
    // just before each number)
    let split = |u: &Url| -> (String, Vec<u64>, Vec<String>) {
        let s = format!("{}?{}", u.path(), u.query().unwrap_or(""));
        let mut skeleton = String::new();
        let mut numbers = Vec::new();
        let mut names = Vec::new();
        let mut run = String::new();
        for c in s.chars().chain(std::iter::once('\0')) {
            if c.is_ascii_digit() {
                run.push(c);
                continue;
            }
            if !run.is_empty() {
                numbers.push(run.parse().unwrap_or(u64::MAX));
                let name: String = skeleton
                    .trim_end_matches(['-', '_', '=', '/'])
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                names.push(name.to_lowercase());
                skeleton.push('#');
                run.clear();
            }
            skeleton.push(c);
        }
        (skeleton, numbers, names)
    };
    let (a, an, names) = split(current);
    let (b, bn, _) = split(candidate);
    if a != b || an.len() != bn.len() {
        return false;
    }
    let changed: Vec<usize> = (0..an.len()).filter(|&i| an[i] != bn[i]).collect();
    let [i] = changed.as_slice() else {
        return false;
    };
    let (i, x, y) = (*i, an[*i], bn[*i]);
    if i + 1 != an.len() || y <= x {
        return false;
    }
    let named = matches!(
        names[i].as_str(),
        "page" | "p" | "pg" | "paged" | "pagenum" | "start" | "offset" | "part"
    );
    named || (y == x + 1 && x <= 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn article(title: &str, text: &str, extra: &str) -> String {
        // Enough prose for reader mode to accept it as an article.
        let para = format!("<p>{text} {}</p>", "It goes on at some length, as articles do. ".repeat(12));
        format!(
            "<html><head><title>{title}</title></head><body>\
             <nav><a href=\"/\">Home</a> <a href=\"/about\">About</a></nav>\
             <article><h1>{title}</h1>{para}{para}{para}</article>\
             {extra}<footer>Copyright, all of it.</footer></body></html>"
        )
    }

    fn run(pages: &HashMap<&str, String>, start: &str) -> (Captured, Vec<(usize, Option<usize>)>) {
        let mut seen = Vec::new();
        let captured = capture(
            start,
            &mut |url| {
                pages
                    .get(url)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("no such page {url}"))
            },
            &mut |n, of| seen.push((n, of)),
            Duration::ZERO,
        )
        .unwrap();
        (captured, seen)
    }

    /// Not a test: `JJB_CAPTURE_PROBE=<address> cargo test -p jjb-core
    /// live_probe -- --ignored --nocapture` captures a real page and
    /// prints the note it would become.
    #[test]
    #[ignore]
    fn live_probe() {
        let Ok(url) = std::env::var("JJB_CAPTURE_PROBE") else {
            return;
        };
        let c = capture(
            &url,
            &mut |u| fetch_html(u),
            &mut |n, of| eprintln!("page {n} of {of:?}"),
            PAUSE,
        );
        match c {
            Ok(c) => {
                eprintln!("pages={} needs_login={} stopped={:?}", c.pages, c.needs_login, c.stopped);
                println!("{}", note_body(&c, "today", None));
            }
            Err(err) => eprintln!("FAILED: {err:#}"),
        }
    }

    #[test]
    fn addresses_are_read_from_pasted_text() {
        assert_eq!(
            parse_urls("https://a.example/one\n  b.example/two, https://a.example/one\nnot-an-address\n"),
            vec!["https://a.example/one", "https://b.example/two"]
        );
        assert!(parse_urls("hello world").is_empty());
    }

    #[test]
    fn one_page_becomes_clean_markdown() {
        let html = article(
            "On Tea",
            "Tea is <strong>good</strong> and <a href=\"/more\">well documented</a>. #teatime",
            "<img src=\"cup.png\"><aside>Subscribe to our newsletter!</aside>",
        );
        let pages = HashMap::from([("https://x.example/tea", html)]);
        let (c, seen) = run(&pages, "https://x.example/tea");
        assert_eq!(c.title, "On Tea");
        assert_eq!(c.pages, 1);
        assert_eq!(seen, vec![(1, None)]);
        assert!(c.markdown.contains("Tea is **good**"), "{}", c.markdown);
        assert!(c.markdown.contains("[well documented](https://x.example/more)"));
        // The page furniture and the picture are gone.
        assert!(!c.markdown.contains("Home"));
        assert!(!c.markdown.contains("Copyright"));
        assert!(!c.markdown.contains("cup.png"));
        // A hashtag in the text must not become one of the note's tags.
        assert!(c.markdown.contains("\\#teatime"));
        let note = note_body(&c, "16 Nov 2026", Some("reading"));
        assert!(note.starts_with("# On Tea\n\n[x.example](https://x.example/tea)\n*Captured 16 Nov 2026*\n\n#reading\n\n"));
        assert_eq!(crate::note::extract_tags(&note), vec!["reading"]);
        // The title is not said twice.
        assert_eq!(note.matches("On Tea").count(), 1);
    }

    #[test]
    fn pages_are_followed_and_joined() {
        let nav = |n: usize| {
            let mut s = format!("<div class=\"pager\">Page {n} of 3 ");
            for i in 1..=3 {
                s.push_str(&format!("<a href=\"/story?page={i}\">{i}</a> "));
            }
            // A decoy: the next *post*, which must not be followed.
            s.push_str("<a rel=\"next\" href=\"/another-story\">Next post</a></div>");
            s
        };
        let pages = HashMap::from([
            ("https://x.example/story", article("Story", "Part one.", &nav(1))),
            ("https://x.example/story?page=2", article("Story", "Part two.", &nav(2))),
            ("https://x.example/story?page=3", article("Story", "Part three.", &nav(3))),
            ("https://x.example/another-story", article("Other", "Unrelated.", "")),
        ]);
        let (c, seen) = run(&pages, "https://x.example/story");
        assert_eq!(c.pages, 3);
        assert_eq!(seen, vec![(1, None), (2, Some(3)), (3, Some(3))]);
        let (one, two, three) = (
            c.markdown.find("Part one.").unwrap(),
            c.markdown.find("Part two.").unwrap(),
            c.markdown.find("Part three.").unwrap(),
        );
        assert!(one < two && two < three);
        assert!(!c.markdown.contains("Unrelated"));
        assert_eq!(c.stopped, None);
        // The title belongs to the note, not to every page of the text.
        assert!(!c.markdown.contains("# Story"), "{}", c.markdown);
    }

    #[test]
    fn next_links_by_label_and_path_and_loops_end() {
        let pages = HashMap::from([
            (
                "https://x.example/guide.html",
                article("Guide", "Start.", "<a href=\"guide-2.html\">Next ›</a>"),
            ),
            (
                "https://x.example/guide-2.html",
                article("Guide", "Middle.", "<a class=\"pager__next\" href=\"/guide-3.html\">More</a>"),
            ),
            // The last page points back at the first: the capture ends.
            (
                "https://x.example/guide-3.html",
                article("Guide", "End.", "<a href=\"/guide.html\">Next</a>"),
            ),
        ]);
        let (c, _) = run(&pages, "https://x.example/guide.html");
        assert_eq!(c.pages, 3);
        assert!(c.markdown.contains("Middle.") && c.markdown.contains("End."));
    }

    #[test]
    fn signposts_are_followed_and_page_jumps_cleaned() {
        let stub = "<html><head><meta http-equiv=\"refresh\" content=\"0; url=/real/\">\
                    <title>Redirect</title></head><body><a href=\"/real/\">Click here</a></body></html>";
        let real = article(
            "Real",
            "See <a href=\"#notes\" title=\"Jump\">the notes</a><a href=\"#cite-1\">[1]</a> and \
             <a href=\"/w/File:A_(b).jpg\"><img src=\"a.jpg\"></a> <a href=\"/other\" title=\"Other page\">another</a>.",
            "",
        );
        let pages = HashMap::from([
            ("https://x.example/old.html", stub.to_owned()),
            ("https://x.example/real/", real),
        ]);
        let (c, _) = run(&pages, "https://x.example/old.html");
        assert_eq!(c.title, "Real");
        assert_eq!(c.pages, 1);
        assert!(c.markdown.contains("See the notes and"), "{}", c.markdown);
        assert!(c.markdown.contains("[another](https://x.example/other)."), "{}", c.markdown);
        assert!(!c.markdown.contains(".jpg") && !c.markdown.contains("[]"), "{}", c.markdown);
    }

    #[test]
    fn a_failing_later_page_keeps_what_was_read() {
        let pages = HashMap::from([(
            "https://x.example/a/1",
            article("A", "Only this.", "<a href=\"/a/2\">Next</a>"),
        )]);
        let (c, _) = run(&pages, "https://x.example/a/1");
        assert_eq!(c.pages, 1);
        assert!(c.stopped.as_deref().is_some_and(|s| s.starts_with("page 2")));
    }

    #[test]
    fn a_login_wall_is_recognised() {
        let html = "<html><head><title>Sign in</title></head><body><form>\
                    <input type=\"email\"><input type=\"password\"><button>Log in</button>\
                    </form><p>Subscribers only.</p></body></html>";
        let pages = HashMap::from([("https://paid.example/article", html.to_owned())]);
        let (c, _) = run(&pages, "https://paid.example/article");
        assert!(c.needs_login);
        assert_eq!(c.pages, 0);
        // …and so is a plain "forbidden".
        let c = capture(
            "https://paid.example/x",
            &mut |_| Err(HttpStatus(403).into()),
            &mut |_, _| {},
            Duration::ZERO,
        )
        .unwrap();
        assert!(c.needs_login);
    }

    #[test]
    fn only_pages_of_the_same_article_count() {
        let u = |s: &str| Url::parse(s).unwrap();
        let first = u("https://x.example/news/big-story");
        assert!(same_article(&first, &first, &u("https://x.example/news/big-story/2")));
        assert!(same_article(&first, &first, &u("https://www.x.example/news/big-story?page=2")));
        assert!(same_article(&first, &first, &u("https://x.example/news/big-story/page/2/")));
        assert!(!same_article(&first, &first, &u("https://x.example/news/big-story-sequel")));
        assert!(!same_article(&first, &first, &u("https://x.example/news/other-story")));
        assert!(!same_article(&first, &first, &u("https://y.example/news/big-story/2")));
        let forum = u("https://x.example/thread/881/page-2");
        assert!(same_article(&u("https://x.example/thread/881"), &forum, &u("https://x.example/thread/881/page-3")));
        assert!(!same_article(&forum, &forum, &u("https://x.example/thread/882/page-2")));
        // A big number counting up is the next thread, not the next page…
        let thread = u("https://x.example/thread/881");
        assert!(!same_article(&thread, &thread, &u("https://x.example/thread/882")));
        // …a small one after a page of the same name is.
        // From a front page, and from an index file to its siblings.
        let front = u("https://x.example/");
        assert!(same_article(&front, &front, &u("https://x.example/page/2/")));
        assert!(!same_article(&front, &front, &u("https://x.example/2")));
        assert!(!same_article(&front, &front, &u("https://x.example/pricing")));
        let index = u("https://x.example/cat/mystery_3/index.html");
        assert!(same_article(&index, &index, &u("https://x.example/cat/mystery_3/page-2.html")));
        assert!(!same_article(&index, &index, &u("https://x.example/cat/mystery_3/about.html")));
        let part = u("https://x.example/tale-1.html");
        assert!(same_article(&part, &part, &u("https://x.example/tale-2.html")));
        assert_eq!(page_of("Showing page 2 of 5 results"), Some((2, 5)));
        assert_eq!(page_of("the front page of the internet"), None);
    }
}
