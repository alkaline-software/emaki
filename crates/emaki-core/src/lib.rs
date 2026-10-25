//! emaki-core: everything that does not need a window.
//!
//! Copy first, render second. `archive` runs before anything else touches a
//! session so that if rendering fails the bytes are already safe. The model is
//! a pure function of the transcript, so the markdown is regenerable and the
//! app draws the same rounds the file gets.

pub mod adapters;
pub mod agents;
pub mod archive;
pub mod build;
pub mod check;
pub mod codex;
pub mod config;
pub mod driver;
pub mod explain;
pub mod files;
pub mod find;
pub mod format;
pub mod fsops;
pub mod git;
pub mod indent;
pub mod json;
pub mod latex;
pub mod limits;
pub mod math;
pub mod model;
pub mod office;
pub mod options;
pub mod outline;
pub mod pairs;
pub mod parquet_file;
pub mod paths;
pub mod peer;
pub mod process;
pub mod pty;
pub mod quarto;
pub mod redact;
pub mod render_md;
pub mod search;
pub mod statusline;
pub mod store;
pub mod terminal;
pub mod transcript;
pub mod trash;
pub mod typst_doc;
pub mod update;
pub mod watcher;
pub mod web;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `read` on a thread of its own with a stack deep enough for a document
/// nested as far as a parser will go: a LaTeX or Typst file's reading
/// recurses once a level, in the parser under it as well as in ours, and
/// Windows gives a thread a megabyte, where a file of a few hundred
/// levels overflowed it and took the app down. The same on every system.
pub(crate) fn on_deep_stack<T: Send>(read: impl FnOnce() -> T + Send) -> T {
    const STACK: usize = 64 << 20;
    std::thread::scope(|scope| match std::thread::Builder::new().name("emaki-read".into()).stack_size(STACK).spawn_scoped(scope, read) {
        Ok(reading) => reading.join().unwrap_or_else(|e| std::panic::resume_unwind(e)),
        Err(e) => panic!("a reading thread could not be started: {e}"),
    })
}
