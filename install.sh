#!/usr/bin/env bash
# One-shot installer: clone the repo, run this, find Attic in the
# app library. Installs per-user into ~/.local — no root needed except
# (optionally) to fetch build tools from your distro.
#
#   git clone https://github.com/reddemo0110/attic.git
#   cd attic && ./install.sh
#
# Re-running after a `git pull` updates the install.
set -euo pipefail
cd "$(dirname "$0")"

NAME=attic
APPID=io.github.reddemo0110.Attic
BASE="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN_DIR="$HOME/.local/bin"
TARGET="${CARGO_TARGET_DIR:-target}"

say() { printf '\033[1m== %s\033[0m\n' "$*"; }

# --- build tools -----------------------------------------------------------
missing=()
command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || missing+=(compiler)
command -v pkg-config >/dev/null 2>&1 || missing+=(pkg-config)
pkg-config --exists xkbcommon 2>/dev/null || missing+=(libxkbcommon)

if [ "${#missing[@]}" -gt 0 ]; then
    say "Missing build tools: ${missing[*]}"
    if command -v dnf >/dev/null 2>&1; then
        say "Installing with dnf (sudo will ask for your password)"
        sudo dnf install -y gcc pkg-config libxkbcommon-devel
    elif command -v pacman >/dev/null 2>&1; then
        say "Installing with pacman (sudo will ask for your password)"
        sudo pacman -S --needed --noconfirm base-devel libxkbcommon
    elif command -v apt-get >/dev/null 2>&1; then
        say "Installing with apt (sudo will ask for your password)"
        sudo apt-get install -y build-essential pkg-config libxkbcommon-dev
    elif command -v zypper >/dev/null 2>&1; then
        say "Installing with zypper (sudo will ask for your password)"
        sudo zypper install -y gcc pkg-config libxkbcommon-devel
    else
        echo "Please install a C compiler, pkg-config and libxkbcommon-devel, then re-run." >&2
        exit 1
    fi
fi

# --- rust ------------------------------------------------------------------
if ! command -v cargo >/dev/null 2>&1; then
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        export PATH="$HOME/.cargo/bin:$PATH"
    else
        say "Installing Rust via rustup (per-user, no root)"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        export PATH="$HOME/.cargo/bin:$PATH"
    fi
fi

# --- build -----------------------------------------------------------------
say "Building (release — the first time takes a few minutes)"
cargo build --release

# --- install into ~/.local -------------------------------------------------
say "Installing into ~/.local"
install -Dm0755 "$TARGET/release/$NAME" "$BIN_DIR/$NAME"
install -Dm0644 apps/jjb-cosmic/resources/icons/hicolor/scalable/apps/icon.svg \
    "$BASE/icons/hicolor/scalable/apps/$APPID.svg"
install -Dm0644 "$TARGET/xdgen/app.metainfo.xml" "$BASE/metainfo/$APPID.metainfo.xml"
mkdir -p "$BASE/applications"
# Every Exec line (the app and its "New note" action) gets the absolute path.
sed "s|^Exec=attic\(.*\)|Exec=$BIN_DIR/$NAME\1|" "$TARGET/xdgen/app.desktop" \
    > "$BASE/applications/$APPID.desktop"
# GNOME Shell search provider: the shell reads the .ini and D-Bus activates
# the .service on demand. Harmless on other desktops.
install -Dm0644 apps/jjb-cosmic/resources/search-provider.ini \
    "$BASE/gnome-shell/search-providers/$APPID.search-provider.ini"
mkdir -p "$BASE/dbus-1/services"
sed "s|^Exec=.*|Exec=$BIN_DIR/$NAME --search-provider|" apps/jjb-cosmic/resources/search-provider.service \
    > "$BASE/dbus-1/services/$APPID.SearchProvider.service"
# COSMIC launcher plugin: `note <words>` in the launcher lists notes.
# pop-launcher reads ~/.local/share/pop-launcher/plugins; harmless elsewhere.
PLUGIN_DIR="$BASE/pop-launcher/plugins/$NAME"
install -Dm0644 apps/jjb-cosmic/resources/launcher-plugin.ron "$PLUGIN_DIR/plugin.ron"
printf '#!/bin/sh\nexec "%s" --launcher-plugin\n' "$BIN_DIR/$NAME" > "$PLUGIN_DIR/$NAME-launcher"
chmod 0755 "$PLUGIN_DIR/$NAME-launcher"
update-desktop-database "$BASE/applications" 2>/dev/null || true
gtk-update-icon-cache -q -t -f "$BASE/icons/hicolor" 2>/dev/null || true

say "Installed. Find Attic in the app library; right-click it in the dock to pin."
case ":${XDG_CURRENT_DESKTOP:-}:" in
    *GNOME*) echo "GNOME: notes show up in the Activities overview search after your next login (or Alt+F2, r, Enter on X11)." ;;
    *COSMIC*) echo "COSMIC: type 'note ' in the launcher (Super) to search your notes." ;;
esac
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "Note: $BIN_DIR is not on your PATH — the launcher entry works anyway." ;;
esac
