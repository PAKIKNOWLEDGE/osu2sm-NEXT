//! Load simfiles from a directory of existing `.sm` files.
//!
//! Mirrors `OsuLoad`'s shape so that downstream nodes (`Rekey`, `Rate`,
//! `Select`, ...) can process both osu! and pre-existing SM sources
//! interchangeably.

use crate::node::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SmLoad {
    /// The input folder. May point to:
    /// - A single song folder (containing one or more `.sm` files), or
    /// - A StepMania-style group folder (containing several song folders).
    pub input: String,
    /// Recurse into subdirectories. Defaults to `true` so a group folder is
    /// scanned in one go.
    pub recursive: bool,
    /// If non-empty, only load `.sm` files whose path contains any of these
    /// substrings (lowercase). Useful for selective re-runs.
    pub filter: Vec<String>,
    /// Output bucket id.
    pub into: BucketId,
}

impl Default for SmLoad {
    fn default() -> Self {
        Self {
            input: "".into(),
            recursive: true,
            filter: Vec::new(),
            into: default(),
        }
    }
}

impl Node for SmLoad {
    fn prepare(&mut self) -> Result<()> {
        if self.input.is_empty() {
            eprintln!();
            eprintln!(
                "drag and drop a folder of .sm files (or a StepMania Songs/<group> folder) into this window, then press enter"
            );
            self.input = crate::bin_shared::read_path_from_stdin()?;
        }
        info!("scanning for .sm files in \"{}\"", self.input);
        Ok(())
    }
    fn apply(&self, _store: &mut SimfileStore) -> Result<()> {
        Ok(())
    }
    fn entry(
        &self,
        store: &mut SimfileStore,
        on_song: &mut dyn FnMut(&mut SimfileStore) -> Result<()>,
    ) -> Result<()> {
        scan_input(self, store, on_song)
    }
    fn buckets_mut<'a>(&'a mut self) -> BucketIter<'a> {
        Box::new(iter::once((BucketKind::Output, &mut self.into)))
    }
}

fn scan_input(
    conf: &SmLoad,
    store: &mut SimfileStore,
    on_song: &mut dyn FnMut(&mut SimfileStore) -> Result<()>,
) -> Result<()> {
    let root = PathBuf::from(&conf.input);
    let walker = if conf.recursive {
        WalkDir::new(&root).follow_links(false)
    } else {
        WalkDir::new(&root).max_depth(1).follow_links(false)
    };
    //Group `.sm` files by their immediate parent directory (the song folder).
    let mut by_song: HashMap<PathBuf, Vec<PathBuf>> = HashMap::default();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!("failed to scan input: {:#}", err);
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if path.extension().and_then(|s| s.to_str()) != Some("sm") {
            continue;
        }
        if !conf.filter.is_empty() {
            let path_lower = path.to_string_lossy().to_lowercase();
            if !conf.filter.iter().any(|f| path_lower.contains(f)) {
                continue;
            }
        }
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        by_song.entry(parent).or_default().push(path);
    }
    if by_song.is_empty() {
        warn!("no .sm files found under \"{}\"", conf.input);
        return Ok(());
    }
    //Set up globals (used downstream by `SimfileWrite`)
    store.reset();
    store.global_set("root", conf.input.clone());
    let mut total = 0usize;
    for (song_dir, sm_paths) in &by_song {
        store.global_set("base", song_dir.to_string_lossy().to_string());
        for sm_path in sm_paths {
            match crate::simfile::Simfile::load(sm_path) {
                Ok(simfiles) => {
                    let boxed: Vec<Box<Simfile>> =
                        simfiles.into_iter().map(Box::new).collect();
                    total += boxed.len();
                    store.put(&conf.into, boxed);
                }
                Err(err) => {
                    warn!(
                        "  failed to load \"{}\": {:#}",
                        sm_path.display(),
                        err
                    );
                }
            }
        }
        on_song(store)?;
        store.reset();
        store.global_set("root", conf.input.clone());
    }
    info!("loaded {} simfiles from {} song folder(s)", total, by_song.len());
    Ok(())
}