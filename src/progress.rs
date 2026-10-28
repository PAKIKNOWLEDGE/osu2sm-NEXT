//! Optional progress/cancel hooks for the GUI.
//!
//! The CLI never installs a [`Progress`], so every function here is a cheap
//! no-op on that path. The GUI installs one `Arc<Progress>` per conversion run
//! and polls it from the UI thread while a worker thread drives the pipeline.
//!
//! A process-global sink is used instead of threading the handle through
//! `Opts` / the node graph so that `Opts` stays a pure serialisable config and
//! the individual nodes (`OsuLoad`, `SimfileWrite`, ...) do not need new
//! fields. The nodes only ever *report*; they never read anything back except
//! the cancel flag.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Live counters for a single conversion run.
#[derive(Debug, Default)]
pub struct Progress {
    /// Beatmapsets discovered so far. Grows while the input folder is scanned.
    total: AtomicUsize,
    /// Beatmapsets finished (successfully or not).
    done: AtomicUsize,
    /// `.sm` files written so far.
    files: AtomicUsize,
    /// Set by the UI when the user asks to stop.
    cancel: AtomicBool,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// A beatmapset was discovered and is about to be processed.
    pub fn add_total(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    /// A beatmapset finished, one way or another.
    pub fn finish_one(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// One simfile was written to disk.
    pub fn add_file(&self) {
        self.files.fetch_add(1, Ordering::Relaxed);
    }

    /// `(done, total, files)`.
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
            self.files.load(Ordering::Relaxed),
        )
    }

    /// Ask the running conversion to stop after the current beatmapset.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// The process-wide progress sink, if any.
///
/// `Mutex<Option<..>>` rather than `OnceLock` because the GUI installs a fresh
/// `Progress` for every run.
static CURRENT: Mutex<Option<Arc<Progress>>> = Mutex::new(None);

/// Install (or replace) the global progress sink.
pub fn install(progress: Arc<Progress>) {
    if let Ok(mut slot) = CURRENT.lock() {
        *slot = Some(progress);
    }
}

/// Remove the global progress sink.
pub fn uninstall() {
    if let Ok(mut slot) = CURRENT.lock() {
        *slot = None;
    }
}

fn with<R>(f: impl FnOnce(&Progress) -> R) -> Option<R> {
    match CURRENT.lock() {
        Ok(slot) => slot.as_ref().map(|progress| f(progress)),
        Err(_) => None,
    }
}

/// See [`Progress::add_total`].
pub fn add_total() {
    let _ = with(Progress::add_total);
}

/// See [`Progress::finish_one`].
pub fn finish_one() {
    let _ = with(Progress::finish_one);
}

/// See [`Progress::add_file`].
pub fn add_file() {
    let _ = with(Progress::add_file);
}

/// Whether the user asked the current run to stop. Always `false` when no
/// `Progress` is installed (i.e. the CLI).
pub fn is_cancelled() -> bool {
    with(Progress::is_cancelled).unwrap_or(false)
}
