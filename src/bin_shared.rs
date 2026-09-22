//! Shared CLI/GUI logic.
//!
//! Items here are used by both the CLI binary (`src/main.rs`) and the GUI
//! binary (`src/bin/gui.rs`). The plan calls for a clean split between the
//! conversion core (this crate) and the entry points (CLI / GUI), so this
//! module exposes the run-time configuration struct, the loader/saver for
//! `.config.txt` files, and the runner that invokes the node graph.

use crate::prelude::*;
use crate::node;
use crate::simfile::Simfile;

/// Top-level configuration shared between CLI and GUI.
///
/// `Opts::default()` produces a config equivalent to the upstream
/// `osu2sm` default, minus the now-deprecated `in_place` and osu!
/// autodetect defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Opts {
    /// A graph of nodes to load, transform and save simfiles.
    pub nodes: Vec<ConcreteNode>,
    /// Whether to carry out redundant sanity checks.
    /// (Will likely error on kinda-correct, mistimed and simultaneous-slider beatmaps).
    pub sanity_check: bool,
    /// A logspec string (see
    /// https://https://docs.rs/flexi_logger/0.16.1/flexi_logger/struct.LogSpecification.html).
    pub log: String,
    /// Whether to log to a file.
    pub log_file: bool,
    /// Enable logging to stderr.
    pub log_stderr: bool,
    /// Enable logging to stdout.
    pub log_stdout: bool,
}
impl Default for Opts {
    fn default() -> Opts {
        Opts {
            nodes: vec![
                node::osuload::OsuLoad {
                    input: "".to_string(),
                    standard: node::osuload::OsuStd {
                        keycount: 0,
                        ..default()
                    },
                    ..default()
                }
                .into(),
                node::rekey::Rekey {
                    gamemode: Gamemode::DanceSingle,
                    ..default()
                }
                .into(),
                node::rate::Rate { ..default() }.into(),
                node::select::Select { ..default() }.into(),
                node::simfilewrite::SimfileWrite {
                    output: "".to_string(),
                    ..default()
                }
                .into(),
            ],
            sanity_check: false,
            log: "info".to_string(),
            log_file: true,
            log_stderr: true,
            log_stdout: false,
        }
    }
}
impl Opts {
    /// Initialize `flexi_logger` according to the `log*` fields.
    pub fn apply_logging(&self) {
        let log_target = if self.log_file {
            flexi_logger::LogTarget::File
        } else {
            flexi_logger::LogTarget::DevNull
        };
        let log_stderr = if self.log_stderr {
            flexi_logger::Duplicate::All
        } else {
            flexi_logger::Duplicate::None
        };
        let log_stdout = if self.log_stdout {
            flexi_logger::Duplicate::All
        } else {
            flexi_logger::Duplicate::None
        };

        if let Err(err) = flexi_logger::Logger::with_str(&self.log)
            .log_target(log_target)
            .duplicate_to_stderr(log_stderr)
            .duplicate_to_stdout(log_stdout)
            .start()
        {
            eprintln!("error initializing logger: {:#}", err);
        }
    }
}

/// Runtime context used by `run_nodes`.
pub struct Ctx {
    pub sm_store: RefCell<SimfileStore>,
    pub nodes: Vec<Box<dyn Node>>,
    pub opts: Opts,
}

/// Run the resolved node graph. Each node's `entry()` is called exactly
/// once; per-song the downstream `apply()` chain is invoked.
pub fn run_nodes(ctx: &Ctx) -> Result<()> {
    let mut store = ctx.sm_store.borrow_mut();
    for (i, node) in ctx.nodes.iter().enumerate() {
        store.reset();
        node.entry(&mut *store, &mut |store| {
            for node in ctx.nodes.iter().skip(i + 1) {
                if ctx.opts.sanity_check {
                    store.check()?;
                }
                trace!("  applying node {:?}", node);
                node.apply(store)?;
            }
            if ctx.opts.sanity_check {
                store.check()?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Read a `.config.txt` (RON) from disk and decode it into `Opts`.
///
/// Windows path separator handling: single `\` is doubled (so `\` becomes
/// `\\` in the RON string); `\\` is collapsed to a single `\`. This lets
/// users paste Windows paths into RON without RON string-escape headaches.
pub fn load_cfg(path: &Path) -> Result<Opts> {
    //Replace all "\" for "\\", and all "\\" for "\", to allow for windows-style paths while still
    //allowing escapes for advanced users.
    let mut txt = fs::read_to_string(path)
        .with_context(|| anyhow!("failed to read config at \"{}\"", path.display()))?;
    let mut replacements = Vec::new();
    let mut skip_next_backslash = false;
    for (idx, _) in txt.match_indices('\\') {
        if skip_next_backslash {
            skip_next_backslash = false;
            continue;
        }
        if let Some(next_char) = txt.get(idx + 1..).and_then(|s| s.chars().next()) {
            if next_char == '\\' {
                //Convert double backslash to single backslash
                replacements.push((idx, ""));
                skip_next_backslash = true;
            } else {
                //Duplicate backslash
                replacements.push((idx, "\\\\"));
            }
        }
    }
    let mut added_bytes = 0;
    for (replace_idx, replace_by) in replacements {
        let replace_idx = (replace_idx as isize + added_bytes) as usize;
        txt.replace_range(replace_idx..replace_idx + 1, replace_by);
        added_bytes += replace_by.len() as isize - 1;
    }
    //Parse patched string
    ron::de::from_str(&txt)
        .with_context(|| anyhow!("failed to parse config at \"{}\"", path.display()))
}

/// Save `Opts` to a `.config.txt` (RON) on disk. Inverse of `load_cfg`.
pub fn save_cfg(path: &Path, opts: &Opts) -> Result<()> {
    ron::ser::to_writer_pretty(
        BufWriter::new(File::create(&path).with_context(|| anyhow!("failed to create file"))?),
        opts,
        default(),
    )
    .context("failed to serialize")?;
    Ok(())
}

/// Detect a known install directory by walking up from a known leaf path
/// and checking for the presence of expected sibling files.
///
/// Used by both `OsuLoad::fix_input` and `SimfileWrite::fix_output` to
/// auto-correct a user-supplied path that may have pointed at a file
/// inside the install rather than at the canonical sub-directory.
pub struct BaseDirFinder<'a> {
    pub base_files: &'a [&'a str],
    pub threshold: f64,
    pub default_main_path: &'a str,
}
impl BaseDirFinder<'_> {
    /// Returns a `(base, main)` path tuple.
    pub fn find_base(
        &self,
        main_path: &Path,
        should_exist: bool,
    ) -> Result<(PathBuf, PathBuf)> {
        let mut base_path = main_path.to_path_buf();
        let mut cur_depth = 0;
        loop {
            //Check whether this path is the base path
            let score = self
                .base_files
                .iter()
                .map(|filename| base_path.join(filename).exists() as u8 as f64)
                .sum::<f64>()
                / self.base_files.len() as f64;
            if score >= self.threshold {
                //Base path!
                break;
            } else {
                //Keep looking
                if !base_path.pop() {
                    //Ran out of ancestors
                    bail!("could not find installation base");
                }
                cur_depth += 1;
            }
        }
        //Fix up main folder if depth is not correct
        let default_main_path: &Path = self.default_main_path.as_ref();
        let main_depth = default_main_path.iter().count();
        let mut tmp_main = main_path.to_path_buf();
        if cur_depth < main_depth {
            //Dig deeper
            tmp_main.extend(default_main_path.iter().skip(cur_depth));
            if should_exist && !tmp_main.is_dir() {
                //Undo the work, this folder does not exist
                tmp_main = main_path.to_path_buf();
            }
        } else if cur_depth > main_depth {
            //Go higher
            for _ in main_depth..cur_depth {
                tmp_main.pop();
            }
        }
        Ok((base_path, tmp_main))
    }
}

/// Create a symlink at `dst` pointing to `src`. On Windows this requires
/// admin privileges (or Developer Mode).
pub fn symlink_file(src: &Path, dst: &Path) -> io::Result<()> {
    let result = {
        #[cfg(target_family = "windows")]
        {
            std::os::windows::fs::symlink_file(src, dst)
        }
        #[cfg(target_family = "unix")]
        {
            std::os::unix::fs::symlink(src, dst)
        }
    };
    if result.is_err() {
        if let Ok(link_src) = fs::read_link(dst) {
            if link_src.canonicalize().ok() == src.canonicalize().ok() {
                //Link already exists
                trace!(
                    "  link \"{}\" <- \"{}\" already exists",
                    src.display(),
                    dst.display()
                );
                return Ok(());
            }
        }
    }
    result
}

/// Create a symlink at `dst` pointing to the directory `src`. See
/// `symlink_file` for permission caveats.
pub fn symlink_dir(src: &Path, dst: &Path) -> io::Result<()> {
    let result = {
        #[cfg(target_family = "windows")]
        {
            std::os::windows::fs::symlink_dir(src, dst)
        }
        #[cfg(target_family = "unix")]
        {
            std::os::unix::fs::symlink(src, dst)
        }
    };
    if result.is_err() {
        if src.canonicalize().ok() == dst.canonicalize().ok() {
            //Paths are equivalent!
            debug!(
                "  link \"{}\" <- \"{}\" already exists (canonical paths are equivalent)",
                src.display(),
                dst.display()
            );
            return Ok(());
        }
        if src.canonicalize().ok() == fs::read_link(dst).and_then(|p| p.canonicalize()).ok() {
            //Link already exists
            debug!(
                "  link \"{}\" <- \"{}\" already exists",
                src.display(),
                dst.display()
            );
            return Ok(());
        }
    }
    result
}

/// Read a single line from stdin, optionally stripping a matching pair of
/// quote characters.
pub fn read_path_from_stdin() -> Result<String> {
    let mut path = String::new();
    io::stdin().read_line(&mut path).context("read stdin")?;
    let mut path = path.trim();
    if (path.starts_with('\'') && path.ends_with('\''))
        || (path.starts_with('"') && path.ends_with('"'))
    {
        path = path[1..path.len() - 1].trim();
    }
    Ok(path.to_string())
}

/// Build a deterministic RNG for a simfile + name pair. Used by nodes that
/// need reproducible randomness (e.g. `debug_allow_chance`).
pub fn simfile_rng(sm: &Simfile, name: &str) -> FastRng {
    let seed = fxhash::hash64(&(&sm.music, &sm.title_trans, &sm.desc, name));
    FastRng::seed_from_u64(seed)
}

/// Linear interpolation helper.
pub fn linear_map(
    in_min: f64,
    in_max: f64,
    out_min: f64,
    out_max: f64,
) -> impl Fn(f64) -> f64 {
    let m = (out_max - out_min) / (in_max - in_min);
    move |input| (input - in_min) * m + out_min
}