//! `osu2sm_core` — the conversion library shared by the CLI and GUI
//! binaries.
//!
//! The CLI is at `src/main.rs`, the GUI is at `src/bin/gui.rs`.

pub mod node;
pub mod osufile;
pub mod simfile;
pub mod bin_shared;
pub mod progress;

/// Common imports used by every module in this crate.
pub mod prelude {
    pub(crate) use crate::{
        node::{ConcreteNode, Node, SimfileStore},
        osufile::{self, Beatmap, TimingPoint},
        simfile::{BeatPos, ControlPoint, Difficulty, DisplayBpm, Gamemode, Note, Simfile, ToTime},
        bin_shared::simfile_rng,
        bin_shared::symlink_dir,
        bin_shared::symlink_file,
        bin_shared::BaseDirFinder,
    };
    pub use anyhow::{anyhow, bail, ensure, Context, Error, Result};
    pub use fxhash::{FxHashMap as HashMap, FxHashSet as HashSet};
    pub use log::{debug, error, info, trace, warn};
    pub use rand::{
        seq::{IteratorRandom, SliceRandom},
        Rng, RngCore, SeedableRng,
    };
    pub use rand_xoshiro::Xoshiro256Plus as FastRng;
    pub use serde::{Deserialize, Serialize};
    pub use std::{
        borrow::Cow,
        cell::{Cell, RefCell},
        cmp,
        convert::{TryFrom, TryInto},
        ffi::{OsStr, OsString},
        fmt::{self, Write as _},
        fs::{self, File},
        io::{self, BufRead, BufReader, BufWriter, Read, Write},
        iter, mem, ops,
        path::{Path, PathBuf},
        time::Instant,
    };
    pub use walkdir::WalkDir;
    pub fn default<T: Default>() -> T {
        T::default()
    }
    #[derive(Debug, Clone, Copy)]
    pub struct SortableFloat(pub f64);
    impl Ord for SortableFloat {
        fn cmp(&self, rhs: &Self) -> cmp::Ordering {
            self.0.partial_cmp(&rhs.0).unwrap_or_else(|| {
                if self.0.is_nan() == rhs.0.is_nan() {
                    cmp::Ordering::Equal
                } else if self.0.is_nan() {
                    cmp::Ordering::Less
                } else {
                    cmp::Ordering::Greater
                }
            })
        }
    }
    impl PartialOrd for SortableFloat {
        fn partial_cmp(&self, rhs: &Self) -> Option<cmp::Ordering> {
            Some(self.cmp(rhs))
        }
    }
    impl PartialEq for SortableFloat {
        fn eq(&self, rhs: &Self) -> bool {
            self.cmp(rhs) == cmp::Ordering::Equal
        }
    }
    impl Eq for SortableFloat {}
}

/// Convenience re-exports for the GUI / library consumers.
pub use bin_shared::{load_cfg as load_opts_from_path, save_cfg as save_opts_to_path, Opts};
pub use bin_shared::Opts as CoreOpts;
pub use bin_shared::run_nodes as run_nodes;

/// Run the full conversion pipeline from an `Opts` value.
///
/// On error, the function returns the underlying `anyhow::Error`. Already-
/// written simfiles are not rolled back.
pub fn run_pipeline(opts: Opts) -> anyhow::Result<()> {
    // `log` accepts exactly one global backend. The GUI installs its own and
    // raises the max level before calling this, so `flexi_logger` could only
    // lose the race — and its failure path prints to a stderr that a windowed
    // build does not have. Fall back to it only when nothing logs yet (CLI).
    if log::max_level() == log::LevelFilter::Off {
        opts.apply_logging();
    }
    let ctx = bin_shared::Ctx {
        sm_store: RefCell::new(Default::default()),
        nodes: node::resolve_buckets(&opts.nodes).context("failed to resolve nodes")?,
        opts,
    };
    bin_shared::run_nodes(&ctx)?;
    Ok(())
}

use anyhow::Context as _;
use std::cell::RefCell;

/// Build the default `Opts` value used when no config file is present.
///
/// This is the config-file-compatible default (every supported mania
/// keycount, every default node). The GUI does **not** use it: see
/// [`pipeline_4k`].
pub fn default_opts() -> Opts {
    Opts::default()
}

/// Knobs for [`pipeline_4k`] that the GUI exposes as checkboxes.
#[derive(Clone, Debug)]
pub struct FourKeyOptions {
    /// Compute output paths and log them without touching the filesystem.
    pub dry_run: bool,
    /// Leave an existing `.sm` untouched instead of overwriting it.
    pub skip_existing: bool,
    /// Copy the audio/background next to the written simfile.
    pub copy_deps: bool,
}

impl Default for FourKeyOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            skip_existing: false,
            copy_deps: true,
        }
    }
}

/// The fork's primary pipeline: osu! **4K mania** → `dance-single`.
///
/// * `OsuLoad` is restricted to 4K so that 7K/9K charts are skipped rather than
///   being force-remapped by `Rekey` (which loses notes when every output key
///   is already held).
/// * `standard` parsing stays disabled — a 4K-only workflow never needs it.
/// * `cleanup` and `in_place` stay off: both are destructive and a GUI user has
///   no config file to review them in.
///
/// Everything else is the same graph the CLI runs (`Rekey` → `Rate` → `Select`
/// → `SimfileWrite`), so testing the GUI also tests the CLI path.
pub fn pipeline_4k(input: &str, output: &str, opts: &FourKeyOptions) -> Opts {
    let mut base = Opts::default();
    for each_node in base.nodes.iter_mut() {
        match each_node {
            node::ConcreteNode::OsuLoad(n) => {
                n.input = input.to_string();
                // 4K mania maps to `dance-single`; other keycounts are not
                // requested, so incompatible charts are skipped with a log
                // line instead of being re-keyed.
                n.gamemodes = vec![simfile::Gamemode::DanceSingle];
                n.standard.keycount = 0;
                n.unicode = true;
            }
            node::ConcreteNode::SimfileWrite(n) => {
                n.output = output.to_string();
                n.dry_run = opts.dry_run;
                n.overwrite = !opts.skip_existing;
                n.cleanup = false;
                n.in_place = false;
                n.copy = if opts.copy_deps {
                    vec![node::simfilewrite::CopyMethod::Copy]
                } else {
                    Vec::new()
                };
            }
            _ => {}
        }
    }
    // The GUI installs its own `log` backend, so `run_pipeline` skips
    // `apply_logging` for it; these stay off so a CLI-shaped run of this same
    // config would not write a log file either.
    base.log_file = false;
    base.log_stderr = false;
    base.log_stdout = false;
    base
}
