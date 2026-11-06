# Name of the application's binary.
name := 'attic'
# The unique ID of the application.
appid := 'io.github.reddemo0110.Attic'

# Path to root file system, which defaults to `/`.
rootdir := ''
# The prefix for the `/usr` directory.
prefix := '/usr'
# The location of the cargo target directory.
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
# The COSMIC app's crate: its launcher files, icon and fonts live there.
app-dir := 'apps' / 'jjb-cosmic'

# Application's appstream metadata
appdata := appid + '.metainfo.xml'
# Application's desktop entry
desktop := appid + '.desktop'
# Application's icon.
icon-svg := appid + '.svg'
# GNOME Shell search provider: the shell reads the .ini, D-Bus activates the
# .service. Harmless on other desktops.
search-ini := appid + '.search-provider.ini'
search-service := appid + '.SearchProvider.service'
# COSMIC launcher plugin: pop-launcher reads `plugin.ron` and starts the
# script beside it (`attic --launcher-plugin`). Harmless on other desktops.
launcher-script := name + '-launcher'

# Install destinations
base-dir := absolute_path(clean(rootdir / prefix))
appdata-dst := base-dir / 'share' / 'appdata' / appdata
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / desktop
icons-dst := base-dir / 'share' / 'icons' / 'hicolor'
icon-svg-dst := icons-dst / 'scalable' / 'apps' / icon-svg
search-ini-dst := base-dir / 'share' / 'gnome-shell' / 'search-providers' / search-ini
search-service-dst := base-dir / 'share' / 'dbus-1' / 'services' / search-service
launcher-plugin-dst := base-dir / 'lib' / 'pop-launcher' / 'plugins' / name

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build --locked {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --workspace --all-features --locked {{args}} -- -W clippy::pedantic

# Proves the core builds for Windows (needs `rustup target add x86_64-pc-windows-msvc`
# and `cargo install cargo-xwin`; the first run downloads the Windows SDK).
check-windows:
    env CARGO_TARGET_DIR={{ cargo-target-dir / 'xwin' }} cargo xwin check -p jjb-core --target x86_64-pc-windows-msvc

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release --locked {{args}}

# Installs files
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{appdata-dst}}
    install -Dm0644 {{ app-dir / 'resources' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'icon.svg' }} {{icon-svg-dst}}
    install -Dm0644 {{ app-dir / 'resources' / 'search-provider.ini' }} {{search-ini-dst}}
    mkdir -p {{ parent_directory(search-service-dst) }}
    sed 's|^Exec=.*|Exec={{bin-dst}} --search-provider|' {{ app-dir / 'resources' / 'search-provider.service' }} > {{search-service-dst}}
    install -Dm0644 {{ app-dir / 'resources' / 'launcher-plugin.ron' }} {{ launcher-plugin-dst / 'plugin.ron' }}
    printf '#!/bin/sh\nexec "%s" --launcher-plugin\n' {{bin-dst}} > {{ launcher-plugin-dst / launcher-script }}
    chmod 0755 {{ launcher-plugin-dst / launcher-script }}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{desktop-dst}} {{icon-svg-dst}} {{search-ini-dst}} {{search-service-dst}}
    rm -rf {{launcher-plugin-dst}}

# Per-user install (no sudo): ~/.local/bin, launcher entry, icon.
# The desktop entry gets an absolute Exec so it works even if ~/.local/bin
# is not on PATH.
user-base := env('HOME') / '.local'
install-user: build-release
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{ user-base / 'bin' / name }}
    install -Dm0644 {{ app-dir / 'resources' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'icon.svg' }} {{ user-base / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / icon-svg }}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{ user-base / 'share' / 'metainfo' / appdata }}
    sed 's|^Exec=attic\(.*\)|Exec={{ user-base / 'bin' / name }}\1|' {{ 'target' / 'xdgen' / 'app.desktop' }} > {{ user-base / 'share' / 'applications' / desktop }}
    install -Dm0644 {{ app-dir / 'resources' / 'search-provider.ini' }} {{ user-base / 'share' / 'gnome-shell' / 'search-providers' / search-ini }}
    mkdir -p {{ user-base / 'share' / 'dbus-1' / 'services' }}
    sed 's|^Exec=.*|Exec={{ user-base / 'bin' / name }} --search-provider|' {{ app-dir / 'resources' / 'search-provider.service' }} > {{ user-base / 'share' / 'dbus-1' / 'services' / search-service }}
    install -Dm0644 {{ app-dir / 'resources' / 'launcher-plugin.ron' }} {{ user-base / 'share' / 'pop-launcher' / 'plugins' / name / 'plugin.ron' }}
    printf '#!/bin/sh\nexec "%s" --launcher-plugin\n' {{ user-base / 'bin' / name }} > {{ user-base / 'share' / 'pop-launcher' / 'plugins' / name / launcher-script }}
    chmod 0755 {{ user-base / 'share' / 'pop-launcher' / 'plugins' / name / launcher-script }}
    -update-desktop-database {{ user-base / 'share' / 'applications' }}
    -gtk-update-icon-cache -q -t -f {{ user-base / 'share' / 'icons' / 'hicolor' }}
    @echo "Installed. Find Attic in the app library; right-click it in the dock to pin."
    @echo "On GNOME, notes appear in the Activities overview search after the next login (or a shell restart)."
    @echo "On COSMIC, type 'note ' in the launcher (Super) to search your notes."

# Removes the per-user install
uninstall-user:
    rm -f {{ user-base / 'bin' / name }} {{ user-base / 'share' / 'applications' / desktop }} {{ user-base / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / icon-svg }} {{ user-base / 'share' / 'metainfo' / appdata }}
    rm -f {{ user-base / 'share' / 'gnome-shell' / 'search-providers' / search-ini }} {{ user-base / 'share' / 'dbus-1' / 'services' / search-service }}
    rm -rf {{ user-base / 'share' / 'pop-launcher' / 'plugins' / name }}
    -update-desktop-database {{ user-base / 'share' / 'applications' }}

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar pcf vendor.tar vendor
    rm -rf vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Bump cargo version, create git commit, and create tag
tag version:
    sed -i '0,/^version/s/^version.*/version = "{{version}}"/' Cargo.toml && git add Cargo.toml
    cargo check
    cargo clean
    git add Cargo.lock
    git commit -m 'release: {{version}}'
    git commit --amend
    git tag -a {{version}} -m ''


# Copy the example notes (and their photos) into the notes folder; never overwrites.
install-examples notes_dir=(env('HOME') / 'Documents' / 'Attic'):
    mkdir -p '{{notes_dir}}/assets'
    cp -n examples/notes/*.md '{{notes_dir}}/'
    cp -n examples/notes/assets/*.jpg '{{notes_dir}}/assets/'
    @echo "Examples installed into {{notes_dir}} — open Attic and look for #examples."
