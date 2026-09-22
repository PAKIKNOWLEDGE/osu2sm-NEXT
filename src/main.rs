//! CLI entry point for osu2sm.
//!
//! The shared logic lives in `osu2sm::bin_shared`; this file only deals
//! with argument parsing, the bootstrap window, and the `flexi_logger`
//! startup. The GUI entry point is `src/bin/gui.rs`.

use osu2sm::bin_shared::{self, Ctx, Opts};
use osu2sm::node;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;

fn run() -> anyhow::Result<()> {
    let load_cfg_from = std::env::args_os()
        .skip(1)
        .next()
        .map(|path| PathBuf::from(path));
    let opts = if let Some(cfg_path) = load_cfg_from {
        //Load from here
        let opts = bin_shared::load_cfg(&cfg_path)?;
        opts.apply_logging();
        log::info!("loaded config from \"{}\"", cfg_path.display());
        opts
    } else {
        //Load/save config from default path
        let mut cfg_path: PathBuf = std::env::current_exe()
            .unwrap_or_default()
            .file_name()
            .unwrap_or_default()
            .into();
        cfg_path.set_extension("config.txt");
        match bin_shared::load_cfg(&cfg_path) {
            Ok(opts) => {
                opts.apply_logging();
                log::info!("loaded config from \"{}\"", cfg_path.display());
                opts
            }
            Err(err) => {
                let opts = Opts::default();
                opts.apply_logging();
                log::info!("failed to load config from default path: {:#}", err);
                if cfg_path.exists() {
                    log::info!("using default config");
                } else {
                    match bin_shared::save_cfg(&cfg_path, &opts) {
                        Ok(()) => {
                            log::info!("saved default config file");
                        }
                        Err(err) => {
                            log::warn!("failed to save default config: {:#}", err);
                        }
                    }
                }
                opts
            }
        }
    };
    let ctx = Ctx {
        sm_store: RefCell::new(Default::default()),
        nodes: node::resolve_buckets(&opts.nodes).context("failed to resolve nodes")?,
        opts,
    };
    bin_shared::run_nodes(&ctx)?;
    Ok(())
}

fn main() {
    let start = Instant::now();
    match run() {
        Ok(()) => {
            log::info!(
                "finished in {}s",
                start.elapsed().as_millis() as f64 / 1000.
            );
        }
        Err(err) => {
            log::error!("fatal error: {:#}", err);
        }
    }
    eprintln!("hit enter to close this window");
    let _ = std::io::stdin().read_line(&mut String::new());
}

// `anyhow::Context` is re-exported so we can call `.context()` above.
use anyhow::Context;