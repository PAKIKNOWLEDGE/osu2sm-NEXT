//! `osu2sm_core` — the conversion library shared by the CLI and GUI
//! binaries.
//!
//! The CLI is at `src/main.rs`, the GUI is at `src/bin/gui.rs`.

pub mod node;
pub mod osufile;
pub mod simfile;
pub mod bin_shared;

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
    opts.apply_logging();
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
pub fn default_opts() -> Opts {
    Opts::default()
}