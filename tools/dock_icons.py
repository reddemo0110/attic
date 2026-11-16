#!/usr/bin/env python3
"""Generate apps/jjb-cosmic/src/dockicon.rs — the dock's action icons in the seven tag icon sets.

Reads the Iconify collection JSON for each set (downloaded from the
iconify/icon-sets repository into a cache directory) and writes a Rust module
with one SVG body per (icon, set). A set that lacks a drawing borrows the
Boxicons one at runtime, the same rule the tag icons follow.

    tools/dock_icons.py [--cache DIR] [--out apps/jjb-cosmic/src/dockicon.rs]

Sets and their Iconify prefixes: Boxicons `bx`, Iconoir `iconoir`, Solar
`solar`, Myna UI `mynaui`, Majesticons `majesticons`, Pixelarticons
`pixelarticons`, Duoicons `duo-icons`.
"""

import argparse
import json
import os
import sys
import urllib.request

RAW = "https://raw.githubusercontent.com/iconify/icon-sets/master/json/{}.json"

# Rust IconSet variant -> Iconify prefix.
SETS = [
    ("Boxicons", "bx"),
    ("Iconoir", "iconoir"),
    ("Solar", "solar"),
    ("MynaUi", "mynaui"),
    ("Majesticons", "majesticons"),
    ("Pixelarticons", "pixelarticons"),
    ("DuoIcons", "duo-icons"),
]

# Numerals drawn as strokes in the bottom-right of a 24-box, laid over a
# shrunken heading glyph for the sets without heading-1 / heading-2.
NUMERALS = {
    "1": '<path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" d="M17.2 14.6l2.8-2.2v9.1"/>',
    "2": '<path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" d="M16.8 14.4a2.7 2.7 0 1 1 4.8 1.9l-4.6 5.2h5.2"/>',
}

# For every dock action: the icon name per set, or None to borrow Boxicons.
# ("heading", "1") means: that set's heading glyph plus a drawn numeral.
ICONS = {
    # variant: (Boxicons, Iconoir, Solar, MynaUi, Majesticons, Pixelarticons, DuoIcons)
    "Bold": ("bold", "bold", "text-bold-bold", "type-bold-solid", "bold", None, None),
    "Italic": ("italic", "italic", "text-italic-bold", "type-italic-solid", "italic", None, None),
    "Mark": ("highlight", None, None, None, None, None, None),
    "Code": ("code-alt", "code", "code-bold", "code-solid", "code", "code", None),
    "Table": ("table", "table", None, "table", "table", "table", None),
    "H1": (("heading", "1"), None, None, "heading-1-solid", None, "heading-1", None),
    "H2": (("heading", "2"), None, None, "heading-2-solid", None, "heading-2", None),
    "Bullet": ("list-ul", "list", "checklist-minimalistic-bold", "list-solid", "view-list-line", "bulletlist", None),
    "Todo": ("checkbox-checked", "check-square", "check-square-bold", "check-square-solid", "checkbox-list", "checkbox-on", None),
    "Quote": ("bxs-quote-alt-left", "quote", "quote-bold", None, None, None, None),
    "Link": ("link", "link", "link-bold", "link-solid", "link", "link", None),
    "Tag": ("hash", "hashtag", "hashtag-bold", "hash", "hashtag-line", "hash", None),
    "Rule": ("minus", "minus", "minus-bold", "minus-solid", "minus", "minus", None),
    "Plus": ("plus", "plus", "add-bold", "plus-solid", "plus", "plus", None),
    "Image": ("image-add", "media-image-plus", "gallery-add-bold", "image-solid", "image-plus", "image-plus", None),
    "Capture": ("cloud-download", "cloud-download", "cloud-download-bold", "cloud-download-solid", "cloud-download", "cloud-download", None),
}

KEYS = {
    "Bold": "bold", "Italic": "italic", "Mark": "mark", "Code": "code",
    "Table": "table", "H1": "h1", "H2": "h2", "Bullet": "bullet",
    "Todo": "todo", "Quote": "quote", "Link": "link", "Tag": "tag",
    "Rule": "rule", "Plus": "plus", "Image": "image",
    "Capture": "capture",
}


def load(prefix, cache):
    path = os.path.join(cache, prefix + ".json")
    if not os.path.exists(path):
        os.makedirs(cache, exist_ok=True)
        print("fetching", prefix, file=sys.stderr)
        req = urllib.request.Request(RAW.format(prefix), headers={"User-Agent": "attic-dock-icons"})
        with urllib.request.urlopen(req, timeout=120) as r, open(path, "wb") as f:
            f.write(r.read())
    with open(path) as f:
        return json.load(f)


def icon(coll, name):
    """(body, left, top, width, height) for `name`, following aliases."""
    icons = coll["icons"]
    aliases = coll.get("aliases", {})
    seen = set()
    while name not in icons:
        if name in seen or name not in aliases:
            raise KeyError(name)
        seen.add(name)
        name = aliases[name]["parent"]
    ic = icons[name]
    return (
        ic["body"],
        ic.get("left", coll.get("left", 0)),
        ic.get("top", coll.get("top", 0)),
        ic.get("width", coll.get("width", 16)),
        ic.get("height", coll.get("height", 16)),
    )


def composed_heading(coll, name, numeral):
    body, left, top, w, h = icon(coll, name)
    # Shrink the heading glyph to the left-middle and set the numeral beside it.
    s = 0.78 * 24 / max(w, h)
    g = f'<g transform="translate(0 2.6) scale({s:.4f}) translate({-left} {-top})">{body}</g>'
    return (g + NUMERALS[numeral], 0, 0, 24, 24)


def rust_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=os.path.expanduser("~/.cache/attic-iconify"))
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "..", "apps", "jjb-cosmic", "src", "dockicon.rs"))
    args = ap.parse_args()

    colls = {variant: load(prefix, args.cache) for variant, prefix in SETS}
    variants = list(ICONS)

    out = []
    out.append("// SPDX-License-Identifier: GPL-3.0-only\n")
    out.append("//! The dock's action icons, drawn in whichever folder icon set the user\n"
               "//! chose (Options → Icon) so the dock and the sidebar match. A set that\n"
               "//! lacks a drawing borrows the Boxicons one, like the tag icons do.\n"
               "//!\n"
               "//! GENERATED by `tools/dock_icons.py` from the Iconify collections — do not\n"
               "//! hand-edit the bodies; change the table in the script and regenerate.\n\n")
    out.append("use crate::glyph::IconSet;\nuse cosmic::iced::Color;\nuse cosmic::widget::svg;\n"
               "use std::collections::HashMap;\nuse std::sync::{Mutex, OnceLock};\n\n")
    out.append("/// Body, viewBox left, top, width, height.\ntype Body = (&'static str, i32, i32, u32, u32);\n\n")
    out.append("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\npub enum DockIcon {\n")
    for v in variants:
        out.append(f"    {v},\n")
    out.append("}\n\nimpl DockIcon {\n")
    out.append("    #[cfg_attr(not(test), allow(dead_code))]\n")
    out.append(f"    pub const ALL: [DockIcon; {len(variants)}] = [\n")
    for v in variants:
        out.append(f"        DockIcon::{v},\n")
    out.append("    ];\n\n")
    out.append("    #[cfg_attr(not(test), allow(dead_code))]\n")
    out.append("    pub fn key(self) -> &'static str {\n        match self {\n")
    for v in variants:
        out.append(f"            DockIcon::{v} => {rust_str(KEYS[v])},\n")
    out.append("        }\n    }\n\n")

    out.append("    /// The SVG body in `set`; an icon the set lacks falls back to Boxicons.\n"
               "    fn body(self, set: IconSet) -> Body {\n"
               "        let own = match set {\n")
    for si, (variant, _) in enumerate(SETS):
        fn = variant.lower()
        if si == 0:
            out.append(f"            IconSet::{variant} => Some(self.{fn}()),\n")
        else:
            out.append(f"            IconSet::{variant} => self.{fn}(),\n")
    out.append("        };\n        own.unwrap_or_else(|| self.boxicons())\n    }\n\n")

    missing = []
    for si, (variant, prefix) in enumerate(SETS):
        coll = colls[variant]
        fn = variant.lower()
        ret = "Body" if si == 0 else "Option<Body>"
        if si != 0 and not any(ICONS[v][si] for v in variants):
            out.append(f"    fn {fn}(self) -> {ret} {{\n        None\n    }}\n\n")
            continue
        out.append(f"    fn {fn}(self) -> {ret} {{\n        match self {{\n")
        for v in variants:
            spec = ICONS[v][si]
            if spec is None:
                if si == 0:
                    raise SystemExit(f"Boxicons must cover every icon: {v}")
                continue
            try:
                if isinstance(spec, tuple):
                    body, left, top, w, h = composed_heading(coll, spec[0], spec[1])
                else:
                    body, left, top, w, h = icon(coll, spec)
            except KeyError as e:
                missing.append((prefix, str(e)))
                continue
            tup = f"({rust_str(body)}, {left}, {top}, {w}, {h})"
            if si == 0:
                out.append(f"            DockIcon::{v} => {tup},\n")
            else:
                out.append(f"            DockIcon::{v} => Some({tup}),\n")
        if si != 0:
            out.append("            _ => None,\n")
        out.append("        }\n    }\n\n")

    out.append('''    /// An SVG of the icon in `color`.
    pub fn svg(self, set: IconSet, color: Color) -> String {
        let (body, left, top, w, h) = self.body(set);
        let fill = hex(color);
        format!(
            "<svg xmlns=\\"http://www.w3.org/2000/svg\\" viewBox=\\"{left} {top} {w} {h}\\">{}</svg>",
            body.replace("currentColor", &fill)
        )
    }

    /// A cached renderer handle for the icon in `set` and `color` — the
    /// same handle every frame, so the renderer keeps its rasterisation.
    pub fn handle(self, set: IconSet, color: Color) -> svg::Handle {
        static CACHE: OnceLock<Mutex<HashMap<(DockIcon, IconSet, [u8; 3]), svg::Handle>>> =
            OnceLock::new();
        let key = (self, set, rgb(color));
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache.lock().expect("dock icon cache");
        cache
            .entry(key)
            .or_insert_with(|| svg::Handle::from_memory(self.svg(set, color).into_bytes()))
            .clone()
    }
}

fn rgb(c: Color) -> [u8; 3] {
    [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    ]
}

fn hex(c: Color) -> String {
    let [r, g, b] = rgb(c);
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dock_icon_draws_in_every_set() {
        let mut keys = std::collections::HashSet::new();
        for icon in DockIcon::ALL {
            for set in IconSet::ALL {
                let (body, _, _, w, h) = icon.body(set);
                assert!(!body.is_empty(), "{icon:?} {set:?}");
                assert!(w > 0 && h > 0, "{icon:?} {set:?}");
            }
            assert!(keys.insert(icon.key()), "duplicate key {}", icon.key());
        }
        assert!(
            DockIcon::Bold
                .svg(IconSet::Iconoir, Color::WHITE)
                .contains("#ffffff")
        );
    }
}
''')

    if missing:
        for prefix, name in missing:
            print(f"missing: {prefix}:{name}", file=sys.stderr)
        raise SystemExit(1)

    with open(args.out, "w") as f:
        f.write("".join(out))
    # Keep the module rustfmt-clean when rustfmt is around.
    import shutil
    import subprocess
    if shutil.which("rustfmt"):
        subprocess.run(["rustfmt", "--edition", "2024", args.out], check=False)
    print("wrote", os.path.normpath(args.out), file=sys.stderr)


if __name__ == "__main__":
    main()
