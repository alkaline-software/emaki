//! emaki-core: everything that does not need a window.
//!
//! Copy first, render second. `archive` runs before anything else touches a
//! session so that if rendering fails the bytes are already safe. The model is
//! a pure function of the transcript, so the markdown is regenerable and the
//! app draws the same rounds the file gets.

pub mod adapters;
pub mod archive;
pub mod build;
pub mod config;
pub mod driver;
pub mod explain;
pub mod files;
pub mod find;
pub mod json;
pub mod limits;
pub mod model;
pub mod options;
pub mod paths;
pub mod peer;
pub mod pty;
pub mod redact;
pub mod render_md;
pub mod search;
pub mod statusline;
pub mod store;
pub mod terminal;
pub mod transcript;
pub mod update;
pub mod watcher;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
