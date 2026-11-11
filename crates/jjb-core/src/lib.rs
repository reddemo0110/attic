// SPDX-License-Identifier: GPL-3.0-only

//! Attic's core: everything that is true of a note regardless of which
//! window shows it.
//!
//! Files on disk are the source of truth; `index.db` is derived and
//! disposable. Nothing in here knows about a toolkit — the COSMIC app and
//! the Tauri app both sit on top of this crate.
//!
//! The pure modules (`markdown`, `table`) build everywhere, including for
//! the browser; the rest needs the `full` feature (on by default).

pub mod markdown;
pub mod table;

#[cfg(feature = "full")]
pub mod images;
#[cfg(feature = "full")]
pub mod links;
#[cfg(feature = "full")]
pub mod note;
#[cfg(feature = "full")]
pub mod platform;
#[cfg(feature = "full")]
pub mod proof;
#[cfg(feature = "full")]
pub mod store;
#[cfg(feature = "full")]
pub mod sync;
