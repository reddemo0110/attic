// SPDX-License-Identifier: GPL-3.0-only

//! Images in notes: the `![alt](assets/x.png){frame=tint size=m}` line format,
//! the asset store, and the retro treatments applied before display.

use anyhow::{Context, Result};
use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

pub const ASSETS_DIR: &str = "assets";
/// Largest edge we keep for display; treatments run on this size.
const DISPLAY_MAX: u32 = 720;
/// The plain style has no treatment to pay for and fills the text column,
/// so it keeps more of the picture.
const PLAIN_DISPLAY_MAX: u32 = 1600;

/// Largest edge kept for display under a style.
pub fn display_max(style: FrameStyle) -> u32 {
    match style {
        FrameStyle::Plain => PLAIN_DISPLAY_MAX,
        _ => DISPLAY_MAX,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum FrameStyle {
    /// No treatment, no card: the picture as it is, across the text column.
    Plain,
    #[default]
    Box,
    Tint,
    Dither,
    Bezel,
    Print,
    Ascii,
    Film,
    Pixel,
    Comic,
}

impl FrameStyle {
    pub const ALL: [FrameStyle; 10] = [
        FrameStyle::Plain,
        FrameStyle::Box,
        FrameStyle::Tint,
        FrameStyle::Dither,
        FrameStyle::Bezel,
        FrameStyle::Print,
        FrameStyle::Ascii,
        FrameStyle::Film,
        FrameStyle::Pixel,
        FrameStyle::Comic,
    ];

    pub fn key(self) -> &'static str {
        match self {
            FrameStyle::Plain => "plain",
            FrameStyle::Box => "box",
            FrameStyle::Tint => "tint",
            FrameStyle::Dither => "dither",
            FrameStyle::Bezel => "bezel",
            FrameStyle::Print => "print",
            FrameStyle::Ascii => "ascii",
            FrameStyle::Film => "film",
            FrameStyle::Pixel => "pixel",
            FrameStyle::Comic => "comic",
        }
    }

    pub fn from_key(key: &str) -> Option<FrameStyle> {
        FrameStyle::ALL.into_iter().find(|f| f.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            FrameStyle::Plain => "plain",
            FrameStyle::Box => "box frame",
            FrameStyle::Tint => "phosphor tint",
            FrameStyle::Dither => "dithered",
            FrameStyle::Bezel => "CRT bezel",
            FrameStyle::Print => "instant print",
            FrameStyle::Ascii => "ASCII",
            FrameStyle::Film => "film strip",
            FrameStyle::Pixel => "chunky pixels",
            FrameStyle::Comic => "comic book",
        }
    }

    /// Whether the pixels depend on the theme palette (cache key needs it).
    pub fn themed(self) -> bool {
        matches!(
            self,
            FrameStyle::Tint | FrameStyle::Dither | FrameStyle::Ascii
        )
    }
}

/// How an inline image sits against the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Align {
    /// Full-width block between paragraphs (centred when narrower).
    #[default]
    Center,
    /// Picture on the left, the following paragraphs beside it.
    Left,
    /// Picture on the right, the following paragraphs beside it.
    Right,
}

impl Align {
    pub const ALL: [Align; 3] = [Align::Left, Align::Center, Align::Right];
    pub fn key(self) -> &'static str {
        match self {
            Align::Center => "center",
            Align::Left => "left",
            Align::Right => "right",
        }
    }
    pub fn from_key(key: &str) -> Option<Align> {
        match key {
            "center" | "centre" => Some(Align::Center),
            "left" => Some(Align::Left),
            "right" => Some(Align::Right),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Align::Center => "centre",
            Align::Left => "left",
            Align::Right => "right",
        }
    }
}

/// Width presets offered in the menu (pixels).
pub const WIDTH_PRESETS: [(&str, u32); 3] = [("small", 240), ("medium", 420), ("large", 720)];
pub const MIN_WIDTH: u32 = 96;
pub const MAX_WIDTH: u32 = 2000;

/// One `![alt](path){attrs}` reference in a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// 0-based line in the body.
    pub line: usize,
    pub alt: String,
    /// As written (relative to the notes dir, usually `assets/…`).
    pub path: String,
    pub frame: FrameStyle,
    pub align: Align,
    /// Display width in pixels; `None` = as wide as the text column.
    pub width: Option<u32>,
}

impl ImageRef {
    /// The markdown line for this reference (attributes omitted when default).
    pub fn to_markdown(&self) -> String {
        let mut attrs = Vec::new();
        if self.frame != FrameStyle::default() {
            attrs.push(format!("frame={}", self.frame.key()));
        }
        if self.align != Align::default() {
            attrs.push(format!("align={}", self.align.key()));
        }
        if let Some(w) = self.width {
            attrs.push(format!("w={w}"));
        }
        if attrs.is_empty() {
            format!("![{}]({})", self.alt, self.path)
        } else {
            format!("![{}]({}){{{}}}", self.alt, self.path, attrs.join(" "))
        }
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// Find every image line. Only whole-line references count (an image in the
/// middle of a sentence stays plain markdown).
#[cfg(test)]
pub fn parse_refs(body: &str) -> Vec<ImageRef> {
    body.lines()
        .enumerate()
        .filter_map(|(line, text)| {
            parse_line(text).map(|mut r| {
                r.line = line;
                r
            })
        })
        .collect()
}

/// Parse a single line as an image reference (line index left at 0).
pub fn parse_line(text: &str) -> Option<ImageRef> {
    let t = text.trim();
    let rest = t.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = rest[..close].to_owned();
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let path = after[..end].trim().to_owned();
    if path.is_empty() || path.contains("://") {
        return None;
    }
    let tail = after[end + 1..].trim();
    let mut frame = FrameStyle::default();
    let mut align = Align::default();
    let mut width = None;
    if let Some(attrs) = tail.strip_prefix('{').and_then(|a| a.strip_suffix('}')) {
        for attr in attrs.split_whitespace() {
            match attr.split_once('=') {
                Some(("frame", v)) => frame = FrameStyle::from_key(v).unwrap_or_default(),
                Some(("align", v)) => align = Align::from_key(v).unwrap_or_default(),
                Some(("w", v)) => {
                    width = v.parse::<u32>().ok().map(|w| w.clamp(MIN_WIDTH, MAX_WIDTH))
                }
                // Legacy size presets from the first cut.
                Some(("size", "s")) => width = Some(240),
                Some(("size", "m")) => width = Some(420),
                Some(("size", "l")) => width = None,
                _ => {}
            }
        }
    } else if !tail.is_empty() {
        return None;
    }
    Some(ImageRef {
        line: 0,
        alt,
        path,
        frame,
        align,
        width,
    })
}

/// Replace the line `line` of `body` with `new_line`, keeping everything else.
#[cfg(test)]
pub fn replace_line(body: &str, line: usize, new_line: &str) -> String {
    let mut out = String::with_capacity(body.len() + new_line.len());
    for (i, l) in body.split_inclusive('\n').enumerate() {
        if i == line {
            out.push_str(new_line);
            if l.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(l);
        }
    }
    out
}

// ---------- block segments ----------

/// A note body split into runs of text and standalone images. Text segments
/// always bracket images (possibly empty) so there is somewhere to type
/// before, between and after pictures.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Text(String),
    Image(ImageRef),
    /// A web address or attached file on a line of its own (a card).
    Link(crate::links::LinkRef),
    /// A thematic break (`---`, `***`, `___`), kept verbatim.
    Rule(String),
    /// A pipe table (with its optional trailing size comment).
    Table(crate::table::Table),
}

/// `---` / `***` / `___` (three or more, spaces allowed) on a line of its own.
pub fn is_rule_line(line: &str) -> bool {
    let t = line.trim();
    let Some(first) = t.chars().next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_')
        && t.chars().filter(|c| *c == first).count() >= 3
        && t.chars().all(|c| c == first || c == ' ')
}

pub fn split(body: &str) -> Vec<Segment> {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<Segment> = Vec::new();
    let mut text: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if !in_fence && let Some(r) = parse_line(line) {
            out.push(Segment::Text(text.join("\n")));
            text.clear();
            out.push(Segment::Image(r));
        } else if !in_fence && is_rule_line(line) {
            out.push(Segment::Text(text.join("\n")));
            text.clear();
            out.push(Segment::Rule(line.to_owned()));
        } else if !in_fence && let Some(l) = crate::links::parse_line(line) {
            out.push(Segment::Text(text.join("\n")));
            text.clear();
            out.push(Segment::Link(l));
        } else if !in_fence
            && crate::table::table_line(line)
            && lines
                .get(i + 1)
                .is_some_and(|l| crate::table::separator_line(l))
        {
            // A pipe table: rows, then maybe its size comment.
            let start = i;
            i += 2;
            while i < lines.len() && crate::table::table_line(lines[i]) {
                i += 1;
            }
            if i < lines.len() && crate::table::size_comment(lines[i]) {
                i += 1;
            }
            if let Some(table) = crate::table::Table::parse(&lines[start..i]) {
                out.push(Segment::Text(text.join("\n")));
                text.clear();
                out.push(Segment::Table(table));
                continue;
            }
            // Not actually parseable: keep the lines as text.
            for l in &lines[start..i] {
                text.push(l);
            }
            continue;
        } else {
            text.push(line);
        }
        i += 1;
    }
    out.push(Segment::Text(text.join("\n")));
    out
}

pub fn join(segments: &[Segment]) -> String {
    // Empty text segments are just the gaps between/around images; they
    // contribute no line of their own.
    let mut parts: Vec<String> = Vec::with_capacity(segments.len());
    for seg in segments {
        match seg {
            Segment::Text(t) if t.is_empty() => {}
            Segment::Text(t) => parts.push(t.clone()),
            Segment::Image(r) => parts.push(r.to_markdown()),
            Segment::Rule(t) => parts.push(t.clone()),
            Segment::Link(l) => parts.push(l.to_markdown()),
            Segment::Table(t) => parts.push(t.to_markdown()),
        }
    }
    let mut body = parts.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body
}

/// Percent-decoded `file://` URIs from a `text/uri-list` payload (files
/// dragged from a file manager). Comment lines and other schemes are skipped.
pub fn uri_list_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://").map(percent_decode))
        .map(|p| {
            // file://host/path — drop a host component if present.
            let p = p.strip_prefix("localhost").unwrap_or(&p).to_owned();
            PathBuf::from(p)
        })
        .collect()
}

/// A file named on the command line or over D-Bus: a `file://` URI or a
/// path, made absolute. `None` for other URI schemes.
pub fn path_from_arg(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let path = if let Some(rest) = raw.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        PathBuf::from(decoded.strip_prefix("localhost").unwrap_or(&decoded))
    } else if raw.contains("://") {
        return None;
    } else {
        PathBuf::from(raw)
    };
    if path.is_absolute() {
        Some(path)
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A directory listing for the in-app picker: folders first, then images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// Folders and pictures, or every ordinary file when `any_file` (attaching).
pub fn list_dir_filtered(dir: &Path, any_file: bool) -> Vec<PickerEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<PickerEntry> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_owned();
            if name.starts_with('.') {
                return None;
            }
            let is_dir = path.is_dir();
            (is_dir || any_file || is_image_file(&path)).then_some(PickerEntry {
                path,
                name,
                is_dir,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

/// A small RGBA thumbnail of an image file, at most `max` px on its long
/// edge (for the picker's grid).
pub fn thumbnail(path: &Path, max: u32) -> Result<(u32, u32, Vec<u8>)> {
    let img = image::open(path).with_context(|| format!("opening {}", path.display()))?;
    let img = img.thumbnail(max, max);
    let rgba = img.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

// ---------- asset store ----------

/// Copy `src` into `<notes dir>/assets/`, returning the relative path to
/// reference from markdown. Names are kept readable and made unique.
pub fn import_asset(notes_dir: &Path, src: &Path) -> Result<String> {
    let assets = notes_dir.join(ASSETS_DIR);
    std::fs::create_dir_all(&assets).context("creating assets dir")?;
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .map(crate::note::slug_filename)
        .unwrap_or_else(|| "image".to_owned())
        .replace(' ', "-")
        .to_lowercase();
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "png".to_owned());
    let mut name = format!("{stem}.{ext}");
    let mut n = 2;
    while assets.join(&name).exists() {
        name = format!("{stem}-{n}.{ext}");
        n += 1;
    }
    std::fs::copy(src, assets.join(&name)).with_context(|| format!("copying {}", src.display()))?;
    Ok(format!("{ASSETS_DIR}/{name}"))
}

/// Write raw image bytes (e.g. from the clipboard) as a new PNG asset.
/// Clipboard paste is not wired yet; kept for that.
#[allow(dead_code)]
pub fn import_bytes(notes_dir: &Path, bytes: &[u8], stem: &str) -> Result<String> {
    let assets = notes_dir.join(ASSETS_DIR);
    std::fs::create_dir_all(&assets).context("creating assets dir")?;
    let img = image::load_from_memory(bytes).context("decoding pasted image")?;
    let mut name = format!("{stem}.png");
    let mut n = 2;
    while assets.join(&name).exists() {
        name = format!("{stem}-{n}.png");
        n += 1;
    }
    img.save(assets.join(&name))
        .context("saving pasted image")?;
    Ok(format!("{ASSETS_DIR}/{name}"))
}

pub fn resolve(notes_dir: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        p.to_owned()
    } else {
        notes_dir.join(p)
    }
}

pub fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}

// ---------- treatments ----------

/// The theme colours the pixel treatments paint with, as 0–255 RGB. The
/// shell builds one from its palette; the core never sees a toolkit colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inks {
    pub bg: [f32; 3],
    pub mute: [f32; 3],
    pub dim: [f32; 3],
    pub fg: [f32; 3],
    pub accent: [f32; 3],
}

/// What the renderer gets back for one image.
#[derive(Debug, Clone)]
pub enum Processed {
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    Ascii(String),
}

/// Columns used for ASCII at a given display width (px), and the glyph size
/// that makes those columns fill it (monospace advance ≈ 0.6em).
pub fn ascii_layout(width: Option<u32>) -> (u32, f32) {
    // Card padding + border eat ~16px; monospace advance is ~0.62em.
    let w = (width.unwrap_or(720) as f32 - 16.0).max(80.0);
    let cols = (w / 7.2).clamp(48.0, 140.0).round() as u32;
    let size = (w / (cols as f32 * 0.62)).clamp(5.0, 14.0);
    (cols, size)
}

/// Load, downscale for display, and apply the frame's pixel treatment.
/// `ascii_cols` only matters for [`FrameStyle::Ascii`].
pub fn load_and_process(
    path: &Path,
    style: FrameStyle,
    inks: Inks,
    ascii_cols: u32,
) -> Result<Processed> {
    let img = image::open(path).with_context(|| format!("opening {}", path.display()))?;
    let (w, h) = (img.width(), img.height());
    let scale = (display_max(style) as f32 / w.max(h) as f32).min(1.0);
    let img = if scale < 1.0 {
        img.resize(
            (w as f32 * scale) as u32,
            (h as f32 * scale) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(process(rgba, style, &inks, ascii_cols))
}

pub fn process(rgba: RgbaImage, style: FrameStyle, p: &Inks, ascii_cols: u32) -> Processed {
    let out = match style {
        FrameStyle::Tint => tint(&rgba, p),
        FrameStyle::Dither => dither(&rgba, p),
        FrameStyle::Pixel => pixelate(&rgba),
        FrameStyle::Bezel => scanlines(rgba),
        FrameStyle::Comic => comic(&rgba),
        FrameStyle::Ascii => return Processed::Ascii(ascii(&rgba, ascii_cols)),
        FrameStyle::Plain | FrameStyle::Box | FrameStyle::Print | FrameStyle::Film => rgba,
    };
    Processed::Pixels {
        width: out.width(),
        height: out.height(),
        rgba: out.into_raw(),
    }
}

fn luma(px: &Rgba<u8>) -> f32 {
    (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> Rgba<u8> {
    Rgba([
        (a[0] + (b[0] - a[0]) * t) as u8,
        (a[1] + (b[1] - a[1]) * t) as u8,
        (a[2] + (b[2] - a[2]) * t) as u8,
        255,
    ])
}

/// Greyscale mapped onto bg → fg of the palette (with a slight lift).
fn tint(src: &RgbaImage, p: &Inks) -> RgbaImage {
    let (bg, fg) = (p.bg, p.accent);
    ImageBuffer::from_fn(src.width(), src.height(), |x, y| {
        let t = luma(src.get_pixel(x, y)).powf(0.9);
        let mut px = lerp(bg, fg, t);
        px[3] = src.get_pixel(x, y)[3];
        px
    })
}

/// Floyd–Steinberg to four palette shades: bg, mute, dim, fg.
fn dither(src: &RgbaImage, p: &Inks) -> RgbaImage {
    let shades = [p.bg, p.mute, p.dim, p.fg];
    let (w, h) = (src.width() as usize, src.height() as usize);
    let mut buf: Vec<f32> = src.pixels().map(|px| luma(px) * 255.0).collect();
    let mut out = ImageBuffer::new(src.width(), src.height());
    for y in 0..h {
        for x in 0..w {
            let old = buf[y * w + x];
            let idx = ((old / 255.0) * 3.0).round().clamp(0.0, 3.0) as usize;
            let new = idx as f32 * 255.0 / 3.0;
            let err = old - new;
            let s = shades[idx];
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([
                    s[0] as u8,
                    s[1] as u8,
                    s[2] as u8,
                    src.get_pixel(x as u32, y as u32)[3],
                ]),
            );
            let mut spread = |dx: isize, dy: isize, wgt: f32| {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && (nx as usize) < w && (ny as usize) < h {
                    buf[ny as usize * w + nx as usize] += err * wgt;
                }
            };
            spread(1, 0, 7.0 / 16.0);
            spread(-1, 1, 3.0 / 16.0);
            spread(0, 1, 5.0 / 16.0);
            spread(1, 1, 1.0 / 16.0);
        }
    }
    out
}

/// Nearest-neighbour down to ~96px on the long edge and back up.
fn pixelate(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width(), src.height());
    let scale = 96.0 / w.max(h) as f32;
    let (sw, sh) = (
        ((w as f32 * scale) as u32).max(1),
        ((h as f32 * scale) as u32).max(1),
    );
    let small = image::imageops::resize(src, sw, sh, image::imageops::FilterType::Nearest);
    image::imageops::resize(&small, w, h, image::imageops::FilterType::Nearest)
}

/// Darken every third row and vignette the corners — the inside of a CRT bezel.
fn scanlines(mut img: RgbaImage) -> RgbaImage {
    let (w, h) = (img.width() as f32, img.height() as f32);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let mut k = if y % 3 == 0 { 0.72 } else { 1.0 };
        let dx = (x as f32 / w - 0.5) * 2.0;
        let dy = (y as f32 / h - 0.5) * 2.0;
        let r = (dx * dx + dy * dy).sqrt();
        if r > 0.7 {
            k *= 1.0 - ((r - 0.7) / 0.72).min(1.0) * 0.55;
        }
        px[0] = (px[0] as f32 * k) as u8;
        px[1] = (px[1] as f32 * k) as u8;
        px[2] = (px[2] as f32 * k) as u8;
    }
    img
}

/// Comic-book halftone: the photo, slightly desaturated to account for the
/// ink, printed as a 45° screen of dots whose size follows the tone — paper
/// between the dots, a darker ink of the local colour inside them.
fn comic(src: &RgbaImage) -> RgbaImage {
    const CELL: f32 = 5.0;
    const PAPER: [f32; 3] = [0.96, 0.925, 0.86];
    let (w, h) = (src.width() as usize, src.height() as usize);
    let lum = |c: &[f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let rgb: Vec<[f32; 3]> = src
        .pixels()
        .map(|p| {
            let c = [
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
            ];
            let y = lum(&c);
            c.map(|v| y + (v - y) * 0.8)
        })
        .collect();
    let tone_at = |x: i64, y: i64| -> f32 {
        let x = x.clamp(0, w as i64 - 1) as usize;
        let y = y.clamp(0, h as i64 - 1) as usize;
        lum(&rgb[y * w + x])
    };
    let (sn, cs) = 45.0f32.to_radians().sin_cos();
    ImageBuffer::from_fn(w as u32, h as u32, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let u = (fx * cs + fy * sn) / CELL;
        let v = (-fx * sn + fy * cs) / CELL;
        let (cu, cv) = (u.round(), v.round());
        let cx = (cu * cs - cv * sn) * CELL;
        let cy = (cu * sn + cv * cs) * CELL;
        let (cxi, cyi) = (cx.round() as i64, cy.round() as i64);
        let mut t = 0.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                t += tone_at(cxi + dx, cyi + dy);
            }
        }
        let cover = (1.0 - t / 9.0).clamp(0.0, 1.0);
        let r = CELL * (cover / std::f32::consts::PI).sqrt() * 1.12;
        let dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        let c = rgb[y as usize * w + x as usize];
        let px = if dist < r {
            c.map(|v| v * 0.5)
        } else {
            // Paper shows between the dots; lifted so the average tone holds.
            let mut out = [0.0; 3];
            for i in 0..3 {
                out[i] = (c[i] * 0.8 + PAPER[i] * 0.2).min(1.0);
            }
            out
        };
        Rgba([
            (px[0] * 255.0).round() as u8,
            (px[1] * 255.0).round() as u8,
            (px[2] * 255.0).round() as u8,
            src.get_pixel(x, y)[3],
        ])
    })
}

/// Characters for luminance at `cols` columns, 2:1 cell aspect.
fn ascii(src: &RgbaImage, cols: u32) -> String {
    const RAMP: &[u8] = b" .:-=+*#%@";
    let cols = cols.clamp(16, 200).min(src.width().max(1));
    let rows =
        ((src.height() as f32 / src.width().max(1) as f32) * cols as f32 * 0.5).max(1.0) as u32;
    let small = image::imageops::resize(src, cols, rows, image::imageops::FilterType::Triangle);
    let mut out = String::with_capacity((cols as usize + 1) * rows as usize);
    for y in 0..rows {
        for x in 0..cols {
            let l = luma(small.get_pixel(x, y));
            let i = ((l * (RAMP.len() - 1) as f32).round() as usize).min(RAMP.len() - 1);
            out.push(RAMP[i] as char);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serialise_refs() {
        let body = "# T\n![sunset](assets/sunset.png)\ntext ![inline](x.png) more\n![ridge](assets/ridge.png){frame=tint align=left w=300}\n![web](https://x/y.png)\n![old](a.png){size=m}\n";
        let refs = parse_refs(body);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].line, 1);
        assert_eq!(refs[0].frame, FrameStyle::Box);
        assert_eq!(
            (refs[1].frame, refs[1].align, refs[1].width),
            (FrameStyle::Tint, Align::Left, Some(300))
        );
        assert_eq!(refs[2].width, Some(420));
        assert_eq!(refs[0].to_markdown(), "![sunset](assets/sunset.png)");
        assert_eq!(
            refs[1].to_markdown(),
            "![ridge](assets/ridge.png){frame=tint align=left w=300}"
        );
        let mut r = refs[0].clone();
        r.frame = FrameStyle::Tint;
        let new = replace_line(body, r.line, &r.to_markdown());
        assert!(new.contains("![sunset](assets/sunset.png){frame=tint}\n"));
        assert_eq!(new.lines().count(), body.lines().count());
        // Plain is a style of its own, not the absence of one: it is written.
        let plain = parse_line("![p](assets/p.jpg){frame=plain}").unwrap();
        assert_eq!((plain.frame, plain.align, plain.width), (FrameStyle::Plain, Align::Center, None));
        assert_eq!(plain.to_markdown(), "![p](assets/p.jpg){frame=plain}");
        assert_eq!(display_max(FrameStyle::Plain), 1600);
        assert_eq!(display_max(FrameStyle::Box), 720);
    }

    #[test]
    fn split_and_join_round_trip() {
        for body in [
            "a\n![x](p.png)\nb\n",
            "a\n\n![x](p.png)\n\nb\n",
            "![x](p.png)\n",
            "![x](p.png)\n![y](q.png)\n",
            "just text\n",
            "",
        ] {
            let segs = split(body);
            assert!(matches!(segs.first(), Some(Segment::Text(_))));
            assert!(matches!(segs.last(), Some(Segment::Text(_))));
            let expect = if body.is_empty() {
                "\n".to_owned()
            } else {
                body.to_owned()
            };
            assert_eq!(join(&segs), expect, "{body:?}");
        }
        let segs = split("a\n![x](p.png)\n![y](q.png)\nb");
        assert_eq!(segs.len(), 5);
        // Rules become their own segment; inside a fence they are text.
        let body = "a\n---\nb\n```\n---\n```\n* * *\n";
        let segs = split(body);
        assert!(matches!(&segs[1], Segment::Rule(r) if r == "---"));
        assert!(matches!(&segs[2], Segment::Text(t) if t == "b\n```\n---\n```"));
        assert!(matches!(&segs[3], Segment::Rule(r) if r == "* * *"));
        assert_eq!(join(&segs), body);
        assert!(is_rule_line("  ___  ") && !is_rule_line("--") && !is_rule_line("---|---"));
    }

    #[test]
    fn treatments_keep_dimensions() {
        let p = Inks {
            bg: [8.0, 12.0, 8.0],
            mute: [40.0, 60.0, 40.0],
            dim: [90.0, 140.0, 90.0],
            fg: [180.0, 255.0, 180.0],
            accent: [120.0, 255.0, 120.0],
        };
        let img = ImageBuffer::from_fn(40, 30, |x, y| {
            Rgba([(x * 6) as u8, (y * 8) as u8, 128, 255])
        });
        for style in FrameStyle::ALL {
            match process(img.clone(), style, &p, 72) {
                Processed::Pixels {
                    width,
                    height,
                    rgba,
                } => {
                    assert_eq!((width, height), (40, 30), "{style:?}");
                    assert_eq!(rgba.len(), 40 * 30 * 4);
                }
                Processed::Ascii(text) => assert!(text.lines().count() >= 1),
            }
        }
    }

    #[test]
    fn uri_list_parses_file_urls() {
        let data = "# comment\r\nfile:///home/me/My%20Pics/a%20b.png\r\nfile://localhost/tmp/c.jpg\r\nhttp://x/y.png\r\n";
        assert_eq!(
            uri_list_paths(data),
            vec![
                PathBuf::from("/home/me/My Pics/a b.png"),
                PathBuf::from("/tmp/c.jpg")
            ]
        );
    }

    #[test]
    fn arg_to_path() {
        assert_eq!(
            path_from_arg("file:///home/me/a%20b.md"),
            Some(PathBuf::from("/home/me/a b.md"))
        );
        assert_eq!(
            path_from_arg("file://localhost/tmp/c.md"),
            Some(PathBuf::from("/tmp/c.md"))
        );
        assert_eq!(path_from_arg("/x/y.md"), Some(PathBuf::from("/x/y.md")));
        assert!(path_from_arg("notes.md").is_some_and(|p| p.is_absolute()));
        assert_eq!(path_from_arg("https://example.com/a.md"), None);
        assert_eq!(path_from_arg("  "), None);
    }

    #[test]
    fn asset_import_names() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("My Photo.PNG");
        image::RgbaImage::new(4, 4).save(&src).unwrap();
        let notes = tmp.path().join("notes");
        assert_eq!(import_asset(&notes, &src).unwrap(), "assets/my-photo.png");
        assert_eq!(import_asset(&notes, &src).unwrap(), "assets/my-photo-2.png");
        assert!(is_image_file(&src));
        assert!(!is_image_file(Path::new("a.txt")));
    }
}
