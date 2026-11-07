# Attic

Retro-flavoured markdown notes for the COSMIC desktop — and for GNOME.

## Installation

The quick way — clone and run the one-shot installer (per-user, into
`~/.local`; it fetches Rust and build tools if the machine lacks them):

```sh
git clone https://github.com/reddemo0110/attic.git
cd attic && ./install.sh
```

Re-run it after a `git pull` to update. Your notes live in
`~/Documents/Attic` as plain markdown files (Options → Notes folder moves
them; a Dropbox folder chosen on every machine keeps them in step).

A [justfile](./justfile) is included by default for the [casey/just][just] command runner.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the application
- `just install` installs the project into the system
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies from that tarball
- `just check` runs clippy on the project to check for linter warnings
- `just check-json` can be used by IDEs that support LSP

## On COSMIC and GNOME

The app works out which desktop launched it (from the session, or by
asking the session bus when there is none) and behaves like a native app
on each.

On COSMIC the chrome is stock libcosmic: fonts, icon theme, light/dark and
accent come from COSMIC Settings. Type `note ` in the launcher (Super) to
search your notes and open one; `attic ` works too. The installer puts the
launcher plugin under `~/.local/share/pop-launcher/plugins/attic`.

On GNOME (Wayland or Xorg) the same build borrows what the desktop already
knows:

- Light/dark mode and the accent colour follow the system setting.
- Interface and monospace fonts, the icon theme, and which title-bar
  buttons are shown come from GNOME's settings, and change with them:
  pick a different font in Settings → Appearance and the window follows
  without a restart.
- Notes turn up in the Activities overview search; pick one to open it,
  or press the arrow to search inside the app. (Log in again after
  installing for the shell to notice the provider.)
- "Open With → Attic" on a `.md` file in Files imports a copy into
  your notes; a file already in the notes folder just opens.
- The launcher's right-click menu has "New note"; launching the app again
  raises the window instead of opening a second one.

## Sync between machines

Notes — with their pictures, attached files and folders — can follow you
between machines through a server you run yourself:
a single [PocketBase](https://pocketbase.io) binary. Set it up in five
minutes with [server/README.md](./server/README.md), then open
Options → Sync in the app, enter the address, an email and a password,
and press **Create account**. Every other machine signs in with the same
details. The server you run holds the note text (over HTTPS, so keep it
yours); the only metadata it indexes is a note's id, a revision counter,
a timestamp and which device wrote it. If two machines change the same
note between syncs, both versions are kept and one is marked
"(conflict, *machine*)". Any folder-sync tool works too, since the notes
are plain files: Syncthing between two machines is the simplest.

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./apps/jjb-cosmic/i18n). New translations may copy the [English (en) localization](./apps/jjb-cosmic/i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Packaging

If packaging for a Linux distribution, vendor dependencies locally with the `vendor` rule, and build with the vendored sources using the `build-vendored` rule. When installing files, use the `rootdir` and `prefix` variables to change installation paths.

```sh
just vendor
just build-vendored
just rootdir=debian/attic prefix=/usr install
```

It is recommended to build a source tarball with the vendored dependencies, which can typically be done by running `just vendor` on the host system before it enters the build environment.

## Developers

Developers should install [rustup][rustup] and configure their editor to use [rust-analyzer][rust-analyzer]. To improve compilation times, disable LTO in the release profile, install the [mold][mold] linker, and configure [sccache][sccache] for use with Rust. The [mold][mold] linker will only improve link times if LTO is disabled.

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[mold]: https://github.com/rui314/mold
[sccache]: https://github.com/mozilla/sccache

## Credits

Folder icons come from [Boxicons](https://github.com/box-icons/boxicons) (MIT), [Iconoir](https://iconoir.com) (MIT), [Solar](https://www.figma.com/community/file/1166831539721848736) by 480 Design (CC BY 4.0), [Myna UI Icons](https://github.com/praveenjuge/mynaui-icons) (MIT), [Majesticons](https://github.com/halfmage/majesticons) (MIT), [Pixelarticons](https://github.com/halfmage/pixelarticons) (MIT) and [Duoicons](https://github.com/fernandcf/duoicons) (MIT), bundled via Iconify. Bundled fonts are under the OFL/UFL, see `apps/jjb-cosmic/resources/fonts`.
