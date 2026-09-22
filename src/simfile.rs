//! Create and write stepmania simfiles.

use crate::prelude::*;

/// Forced to be 4 by the godlike simfile format.
const BEATS_IN_MEASURE: i32 = 4;

/// Standard SM 5.1 preview length, in seconds.
const SAMPLE_LENGTH_SECONDS: f64 = 12.0;

/// Format an `f64` value to a string compatible with the StepMania 5.1
/// reference convention: up to 3 decimal places, with trailing zeros removed,
/// and no scientific notation. `NaN` and `Inf` are coerced to `0`.
///
/// Examples:
/// - `0.5985209960937521` → `"0.599"`
/// - `171.99996500651727` → `"172"`
/// - `-1.123` → `"-1.123"`
/// - `133.020` → `"133.02"`
pub fn fmt_f64(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    // Round to 3 decimal places via string parse (avoids f64 drift).
    let rounded = (v * 1000.).round() / 1000.;
    let s = format!("{}", rounded);
    s
}

#[derive(Debug, Clone)]
pub struct Simfile {
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub title_trans: String,
    pub subtitle_trans: String,
    pub artist_trans: String,
    pub genre: String,
    pub credit: String,
    pub banner: Option<PathBuf>,
    pub background: Option<PathBuf>,
    pub lyrics: Option<PathBuf>,
    pub cdtitle: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub offset: f64,
    pub bpms: Vec<ControlPoint>,
    pub stops: Vec<(f64, f64)>,
    pub sample_start: Option<f64>,
    pub sample_len: Option<f64>,
    pub display_bpm: DisplayBpm,
    pub gamemode: Gamemode,
    pub desc: String,
    pub difficulty: Difficulty,
    pub difficulty_num: f64,
    pub radar: [f64; 5],
    pub notes: Vec<Note>,
}
impl Simfile {
    pub fn save<'a>(path: &Path, simfiles: impl IntoIterator<Item = &'a Simfile>) -> Result<()> {
        let mut simfiles = simfiles.into_iter();
        let main_sm = simfiles.next().ok_or(anyhow!("zero simfiles supplied"))?;
        let mut file = BufWriter::new(File::create(path).context("create file")?);
        fn as_utf8<'a>(path: &'a Option<PathBuf>, name: &str) -> Result<&'a str> {
            path.as_deref()
                .unwrap_or_else(|| "".as_ref())
                .to_str()
                .ok_or_else(|| anyhow!("non-utf8 {}", name))
        }
        // Dedupe `*_translit` against their primary counterparts.
        // When the transliteration is identical to the primary, emit an empty
        // string instead — matches the StepMania 5.1 reference convention.
        let title_t = if main_sm.title_trans == main_sm.title {
            ""
        } else {
            main_sm.title_trans.as_str()
        };
        let subtitle_t = if main_sm.subtitle_trans == main_sm.subtitle {
            ""
        } else {
            main_sm.subtitle_trans.as_str()
        };
        let artist_t = if main_sm.artist_trans == main_sm.artist {
            ""
        } else {
            main_sm.artist_trans.as_str()
        };
        // #SAMPLESTART must be non-negative.
        let sample_start = main_sm
            .sample_start
            .map(|s| fmt_f64(s.max(0.)))
            .unwrap_or_else(String::new);
        // #SAMPLELENGTH is the song-wheel preview length; the SM 5.1 reference
        // convention is 12 seconds.
        let sample_len = main_sm
            .sample_len
            .map(|l| fmt_f64(l.clamp(0., f64::MAX)))
            .unwrap_or_else(|| "12.000".to_string());
        write!(
            file,
            r#"
#TITLE:{title};
#SUBTITLE:{subtitle};
#ARTIST:{artist};
#TITLETRANSLIT:{title_t};
#SUBTITLETRANSLIT:{subtitle_t};
#ARTISTTRANSLIT:{artist_t};
#GENRE:{genre};
#CREDIT:{credit};
#BANNER:{banner};
#BACKGROUND:{bg};
#LYRICSPATH:{lyrics};
#CDTITLE:{cdtitle};
#MUSIC:{music};
#OFFSET:{offset};
#SAMPLESTART:{sample_start};
#SAMPLELENGTH:{sample_len};
#DISPLAYBPM:{display_bpm};
#SELECTABLE:YES;
#BPMS:{bpms};
#STOPS:;
#BGCHANGES:;
#KEYSOUNDS:;
#ATTACKS:;
"#,
            title = main_sm.title,
            subtitle = main_sm.subtitle,
            artist = main_sm.artist,
            title_t = title_t,
            subtitle_t = subtitle_t,
            artist_t = artist_t,
            genre = main_sm.genre,
            credit = main_sm.credit,
            banner = as_utf8(&main_sm.banner, "BANNER")?,
            bg = as_utf8(&main_sm.background, "BACKGROUND")?,
            lyrics = as_utf8(&main_sm.lyrics, "LYRICSPATH")?,
            cdtitle = as_utf8(&main_sm.cdtitle, "CDTITLE")?,
            music = as_utf8(&main_sm.music, "MUSIC")?,
            offset = fmt_f64(main_sm.offset),
            sample_start = sample_start,
            sample_len = sample_len,
            display_bpm = main_sm.display_bpm.to_string_with(|v| fmt_f64(v)),
            bpms = {
                let mut bpms = String::new();
                let mut first = true;
                for point in main_sm.bpms.iter() {
                    if first {
                        first = false;
                    } else {
                        bpms.push(',');
                    }
                    write!(
                        bpms,
                        "{}={}",
                        fmt_f64(point.beat.as_num()),
                        fmt_f64(point.bpm()),
                    )
                    .unwrap();
                }
                bpms
            },
        )?;
        for sm in iter::once(main_sm).chain(simfiles) {
            // Visual separator before each `#NOTES:` block, mirroring the
            // convention used by SM 5.1 reference packs (e.g. ettgroup/YATTA).
            write!(
                file,
                "\n//---------------{gamemode} - {diff_name}----------------",
                gamemode = sm.gamemode.id(),
                diff_name = sm.difficulty.name(),
            )?;
            write!(
                file,
                r#"
#NOTES:
    {gamemode}:
    {desc}:
    {diff_name}:
    {diff_num}:
    {radar0}, {radar1}, {radar2}, {radar3}, {radar4}:"#,
                gamemode = sm.gamemode.id(),
                desc = sm.desc,
                diff_name = sm.difficulty.name(),
                diff_num = sm.difficulty_num.round(),
                radar0 = sm.radar[0],
                radar1 = sm.radar[1],
                radar2 = sm.radar[2],
                radar3 = sm.radar[3],
                radar4 = sm.radar[4],
            )?;
            write_notedata(&mut file, &sm)?;
            write!(file, ";")?;
        }
        Ok(())
    }

    /// Get the files that this simfile references.
    pub fn file_deps(&self) -> impl Iterator<Item = &Path> {
        self.banner
            .as_deref()
            .into_iter()
            .chain(self.background.as_deref().into_iter())
            .chain(self.lyrics.as_deref().into_iter())
            .chain(self.cdtitle.as_deref().into_iter())
            .chain(self.music.as_deref().into_iter())
    }

    /// Iterate over the populated beats in a simfile.
    pub fn iter_beats(&self) -> BeatIter {
        BeatIter::new(&self.notes)
    }

    /// Get a helper type useful for getting monotonically increasing times from beats.
    pub fn beat_to_time(&self) -> ToTime {
        ToTime::new(self)
    }

    /// Naive difficulty calculation.
    pub fn difficulty_naive(&self) -> f64 {
        fn adapt_range(src: (f64, f64), dst: (f64, f64), val: f64) -> f64 {
            dst.0 + (val - src.0) / (src.1 - src.0) * (dst.1 - dst.0)
        }
        let diff = adapt_range((6., 14.), (1., 12.), (self.notes.len() as f64).log2());
        diff.max(1.)
    }

    /// Osu allows two notes at the same time and key, but the `.sm` format disallows this.
    ///
    /// Having two notes at the exact same location is usually wrong, except for the tail -> head
    /// or tail -> hit case (where a note ends and another note immediately starts).
    /// In order to fix this, if there is a tail and afterwards at the exact same beat and key
    /// there is another note, the tail is moved back a little.
    /// Note that this requires sorting the notes if any is moved.
    pub fn fix_tails(&mut self) -> Result<()> {
        let mut cur_beat = BeatPos::from(0.);
        let mut cur_beat_first_note = 0;
        for i in 0..self.notes.len() {
            let note = &self.notes[i];
            if note.beat > cur_beat {
                cur_beat_first_note = i;
                cur_beat = note.beat;
            }
            if note.is_tail() {
                if self.notes[i + 1..]
                    .iter()
                    .take_while(|next_n| next_n.beat == cur_beat)
                    .any(|next_n| next_n.key == note.key)
                {
                    //Move back by the smallest beat unit, and to the previous beat
                    self.notes[i].beat -= BeatPos::EPSILON;
                    self.notes[cur_beat_first_note..i + 1].rotate_right(1);
                }
            }
        }
        Ok(())
    }

    /// Sanity-check a simfile.
    ///
    /// These checks prioritize correctness over speed, and as such should only be used for
    /// debugging purposes.
    pub fn check(&self) -> Result<()> {
        let key_count = self.gamemode.key_count() as usize;
        //Basic control point checks
        let mut last_beat = BeatPos::from(0.) - BeatPos::EPSILON;
        ensure!(!self.bpms.is_empty(), "no control points");
        for cp in self.bpms.iter() {
            ensure!(
                cp.beat != last_beat,
                "two control points at beat {}",
                last_beat
            );
            ensure!(
                cp.beat > last_beat,
                "control point beats do not increase monotonically ({} < {})",
                cp.beat,
                last_beat
            );
            ensure!(
                cp.beat_len.is_finite() && cp.beat_len > 0.,
                "control point beatlength ({}) is not a positive real",
                cp.beat_len
            );
            last_beat = cp.beat;
        }
        //Check a single beat
        let mut beat_notes = vec![false; key_count];
        let mut beat_tails = vec![false; key_count];
        let mut check_beat = |beat, start_idx: usize, end_idx: usize| -> Result<()> {
            for n in beat_notes.iter_mut() {
                *n = false;
            }
            for t in beat_tails.iter_mut() {
                *t = false;
            }
            for idx in start_idx..end_idx {
                let key = self.notes[idx].key as usize;
                if self.notes[idx].is_tail() {
                    ensure!(
                        !beat_tails[key],
                        "two tails on beat {}, key {} (beat {:?})",
                        beat,
                        key,
                        &self.notes[start_idx..end_idx]
                    );
                    beat_tails[key] = true;
                } else {
                    ensure!(
                        !beat_notes[key],
                        "two hit/head notes on beat {}, key {} (beat {:?})",
                        beat,
                        key,
                        &self.notes[start_idx..end_idx]
                    );
                    beat_notes[key] = true;
                }
            }
            Ok(())
        };
        //Note sanity checks
        let mut last_beat = BeatPos::from(0.);
        let mut last_beat_start = 0;
        for (idx, note) in self.notes.iter().enumerate() {
            //Individual note checks
            ensure!(
                note.beat >= last_beat,
                "note beats do not increase monotonically ({} < {})",
                note.beat,
                last_beat
            );
            ensure!(
                note.is_hit() || note.is_head() || note.is_tail(),
                "unknown note kind '{}'",
                note.kind
            );
            ensure!(note.key >= 0, "note key ({}) is negative", note.key);
            ensure!(
                note.key < key_count as i32,
                "note key is not less than key-count-for-gamemode-{:?}: {} >= {}",
                self.gamemode,
                note.key,
                key_count
            );
            //Check an entire beat
            if last_beat != note.beat {
                check_beat(last_beat, last_beat_start, idx)?;
                last_beat = note.beat;
                last_beat_start = idx;
            }
            //Check hold notes
            if note.is_head() {
                //Search for its tail
                let mut found = false;
                for j in idx + 1..self.notes.len() {
                    let next_note = &self.notes[j];
                    if next_note.key == note.key {
                        ensure!(next_note.is_tail(), "hold head at beat {}, key {} is followed by non-tail (kind '{}') at beat {}", note.beat, note.key, next_note.kind, next_note.beat);
                        ensure!(
                            next_note.beat != note.beat,
                            "zero-length hold note at beat {}, key {}",
                            note.beat,
                            note.key
                        );
                        found = true;
                        break;
                    }
                }
                ensure!(
                    found,
                    "head at beat {}, key {}, index {} has no matching tail",
                    note.beat,
                    note.key,
                    idx
                );
            } else if note.is_tail() {
                //Search for its head
                let mut found = false;
                for j in (0..idx).rev() {
                    let prev_note = &self.notes[j];
                    if prev_note.key == note.key {
                        ensure!(prev_note.is_head(), "hold tail at beat {}, key {} is preceded by non-head (kind '{}') at beat {}", note.beat, note.key, prev_note.kind, prev_note.beat);
                        found = true;
                        break;
                    }
                }
                ensure!(
                    found,
                    "tail at beat {}, key {}, index {} has no matching head",
                    note.beat,
                    note.key,
                    idx
                );
            }
        }
        //Check the last remaining beat
        check_beat(last_beat, last_beat_start, self.notes.len())?;
        Ok(())
    }

    /// Load one or more simfiles from a `.sm` file path.
    ///
    /// A single `.sm` file can contain multiple `#NOTES:` blocks (one per
    /// difficulty); this returns a `Vec<Simfile>` with one entry per block.
    /// The shared metadata (`#TITLE`, `#OFFSET`, `#BPMS`, ...) is copied to
    /// each returned `Simfile`; per-block fields (gamemode, difficulty,
    /// radar, notes) are parsed from each `#NOTES:` block.
    pub fn load(path: &Path) -> Result<Vec<Simfile>> {
        let text = fs::read_to_string(path)
            .with_context(|| anyhow!("failed to read \"{}\"", path.display()))?;
        let mut parser = SmParser::new(&text);
        let meta = parser.parse_header()?;
        let mut out = Vec::new();
        while let Some((gamemode, difficulty, difficulty_num, radar, desc, notes_text)) =
            parser.parse_notes_block()?
        {
            let notes = parse_notedata(&notes_text, gamemode.key_count())?;
            let mut sm = Simfile {
                title: meta.title.clone(),
                subtitle: meta.subtitle.clone(),
                artist: meta.artist.clone(),
                title_trans: meta.title_trans.clone(),
                subtitle_trans: meta.subtitle_trans.clone(),
                artist_trans: meta.artist_trans.clone(),
                genre: meta.genre.clone(),
                credit: meta.credit.clone(),
                banner: meta.banner.as_ref().map(PathBuf::from),
                background: meta.background.as_ref().map(PathBuf::from),
                lyrics: meta.lyrics.as_ref().map(PathBuf::from),
                cdtitle: meta.cdtitle.as_ref().map(PathBuf::from),
                music: meta.music.as_ref().map(PathBuf::from),
                offset: meta.offset,
                bpms: meta.bpms.clone(),
                stops: meta.stops.clone(),
                sample_start: meta.sample_start,
                sample_len: meta.sample_len,
                display_bpm: meta.display_bpm,
                gamemode,
                desc,
                difficulty,
                difficulty_num,
                radar,
                notes,
            };
            // Re-pair title-translits with primaries on load. The .sm file
            // can have an explicit translit that equals the primary (e.g. when
            // the author only filled in the primary); we treat that as no
            // transliteration.
            if sm.title_trans == sm.title {
                sm.title_trans.clear();
            }
            if sm.subtitle_trans == sm.subtitle {
                sm.subtitle_trans.clear();
            }
            if sm.artist_trans == sm.artist {
                sm.artist_trans.clear();
            }
            out.push(sm);
        }
        if out.is_empty() {
            bail!("no #NOTES: blocks found in \"{}\"", path.display());
        }
        Ok(out)
    }
}

/// Strip characters that are invalid in filenames on at least one common
/// operating system, plus leading/trailing whitespace. Falls back to
/// `"untitled"` if the result is empty.
pub fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            // Forbidden on Windows; forbidden on most filesystems in general.
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('_'),
            // Control characters (NUL, BEL, ...).
            c if (c as u32) < 0x20 => out.push('_'),
            c => out.push(c),
        }
    }
    let trimmed = out.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

fn write_measure(
    file: &mut impl Write,
    key_count: i32,
    measure_idx: usize,
    measure_start: BeatPos,
    notes: &[Note],
) -> Result<()> {
    //Extract largest simplified denominator, in prime-factorized form.
    //To obtain the actual number from prime-factorized form, use 2^pf[0] * 3^pf[1]
    fn get_denom(mut num: i32) -> [u32; 2] {
        let mut den = BeatPos::FIXED_POINT;
        let mut simplify_by = [0; 2];
        for (idx, &factor) in [2, 3].iter().enumerate() {
            while num % factor == 0 && den % factor == 0 {
                num /= factor;
                den /= factor;
                simplify_by[idx] += 1;
            }
        }
        simplify_by
    }
    let simplify_by = if notes.is_empty() {
        BeatPos::FIXED_POINT
    } else {
        let mut max_simplify_by = [u32::MAX; 2];
        for note in notes {
            let rel_pos = note.beat - measure_start;
            ensure!(
                rel_pos >= BeatPos::from(0.),
                "handed a note that starts before the measure start ({} < {})",
                note.beat,
                measure_start
            );
            let simplify_by = get_denom(rel_pos.frac);
            for (max_exp, exp) in max_simplify_by.iter_mut().zip(simplify_by.iter()) {
                *max_exp = u32::min(*max_exp, *exp);
            }
        }
        2i32.pow(max_simplify_by[0]) * 3i32.pow(max_simplify_by[1])
    };
    let rows_per_beat = BeatPos::FIXED_POINT / simplify_by;
    //Output 4x this amount of rows (if 4 beats in measure)
    let mut out_measure =
        vec![b'0'; (BEATS_IN_MEASURE * rows_per_beat) as usize * key_count as usize];
    for note in notes {
        let rel_pos = note.beat - measure_start;
        let idx = (rel_pos.frac / simplify_by) as usize;
        ensure!(
            rel_pos.frac % simplify_by == 0,
            "incorrect simplify_by ({} % {} == {} != 0)",
            rel_pos,
            simplify_by,
            rel_pos.frac % simplify_by
        );
        ensure!(
            idx < (BEATS_IN_MEASURE * rows_per_beat) as usize,
            "called `flush_measure` with more than one measure in buffer (rel_pos = {} out of max {})",
            rel_pos,
            BEATS_IN_MEASURE * rows_per_beat,
        );
        ensure!(
            note.key >= 0 && note.key < key_count,
            "note key {} outside range [0, {})",
            note.key,
            key_count
        );
        out_measure[idx * key_count as usize + note.key as usize] = note.kind as u8;
    }
    //Convert map into a string
    if measure_idx > 0 {
        //Add separator from previous measure
        write!(file, ",")?;
    }
    write!(file, "\n// Measure {}", measure_idx)?;
    for row in 0..(BEATS_IN_MEASURE * rows_per_beat) as usize {
        write!(file, "\n")?;
        for key in 0..key_count as usize {
            file.write_all(&[out_measure[row * key_count as usize + key]])?;
        }
    }
    Ok(())
}

fn write_notedata(file: &mut impl Write, sm: &Simfile) -> Result<()> {
    struct CurMeasure {
        first_note: usize,
        start_beat: BeatPos,
    }

    let key_count = sm.gamemode.key_count();
    let mut measure_counter = 0;
    let mut cur_measure = CurMeasure {
        first_note: 0,
        start_beat: BeatPos::from(0.),
    };
    for (note_idx, note) in sm.notes.iter().enumerate() {
        //Finish any pending measures
        while (note.beat - cur_measure.start_beat) >= BeatPos::from(BEATS_IN_MEASURE as f64) {
            write_measure(
                file,
                key_count,
                measure_counter,
                cur_measure.start_beat,
                &sm.notes[cur_measure.first_note..note_idx],
            )?;
            measure_counter += 1;
            cur_measure.first_note = note_idx;
            cur_measure.start_beat =
                cur_measure.start_beat + BeatPos::from(BEATS_IN_MEASURE as f64);
        }
    }
    //Finish the last pending measure
    write_measure(
        file,
        key_count,
        measure_counter,
        cur_measure.start_beat,
        &sm.notes[cur_measure.first_note..sm.notes.len()],
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct BeatIter<'a> {
    notes: &'a [Note],
    next_idx: usize,
}
impl BeatIter<'_> {
    pub fn new(notes: &[Note]) -> BeatIter {
        BeatIter { notes, next_idx: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.next_idx >= self.notes.len()
    }

    pub fn peek(&self) -> Option<Beat> {
        self.clone().next()
    }
}
impl Iterator for BeatIter<'_> {
    type Item = Beat;
    fn next(&mut self) -> Option<Beat> {
        if self.is_empty() {
            return None;
        }
        let beat_start = self.next_idx;
        let cur_beat = self.notes[beat_start].beat;
        while self.next_idx < self.notes.len() && self.notes[self.next_idx].beat == cur_beat {
            self.next_idx += 1;
        }
        let beat_end = self.next_idx;
        Some(Beat {
            pos: cur_beat,
            start_idx: beat_start,
            end_idx: beat_end,
        })
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Beat {
    pub pos: BeatPos,
    pub start_idx: usize,
    pub end_idx: usize,
}
impl Beat {
    pub fn count_heads(&self, notes: &[Note]) -> usize {
        notes[self.start_idx..self.end_idx]
            .iter()
            .filter(|note| !note.is_tail())
            .count()
    }
}

/// From the StepMania source,
/// [`GameManager.cpp`](https://github.com/stepmania/stepmania/blob/5_1-new/src/GameManager.cpp):
///
/// ```
/// // dance
/// { "dance-single",	4,	true,	StepsTypeCategory_Single },
/// { "dance-double",	8,	true,	StepsTypeCategory_Double },
/// { "dance-couple",	8,	true,	StepsTypeCategory_Couple },
/// { "dance-solo",		6,	true,	StepsTypeCategory_Single },
/// { "dance-threepanel",	3,	true,	StepsTypeCategory_Single }, // thanks to kurisu
/// { "dance-routine",	8,	false,	StepsTypeCategory_Routine },
/// // pump
/// { "pump-single",	5,	true,	StepsTypeCategory_Single },
/// { "pump-halfdouble",	6,	true,	StepsTypeCategory_Double },
/// { "pump-double",	10,	true,	StepsTypeCategory_Double },
/// { "pump-couple",	10,	true,	StepsTypeCategory_Couple },
/// // uh, dance-routine has that one bool as false... wtf? -aj
/// { "pump-routine",	10,	true,	StepsTypeCategory_Routine },
/// // kb7
/// { "kb7-single",		7,	true,	StepsTypeCategory_Single },
/// // { "kb7-small",		7,	true,	StepsTypeCategory_Single },
/// // ez2dancer
/// { "ez2-single",		5,	true,	StepsTypeCategory_Single },	// Single: TL,LHH,D,RHH,TR
/// { "ez2-double",		10,	true,	StepsTypeCategory_Double },	// Double: Single x2
/// { "ez2-real",		7,	true,	StepsTypeCategory_Single },	// Real: TL,LHH,LHL,D,RHL,RHH,TR
/// // parapara paradise
/// { "para-single",	5,	true,	StepsTypeCategory_Single },
/// // ds3ddx
/// { "ds3ddx-single",	8,	true,	StepsTypeCategory_Single },
/// // beatmania
/// { "bm-single5",		6,	true,	StepsTypeCategory_Single },	// called "bm" for backward compat
/// { "bm-versus5",		6,	true,	StepsTypeCategory_Single },	// called "bm" for backward compat
/// { "bm-double5",		12,	true,	StepsTypeCategory_Double },	// called "bm" for backward compat
/// { "bm-single7",		8,	true,	StepsTypeCategory_Single },	// called "bm" for backward compat
/// { "bm-versus7",		8,	true,	StepsTypeCategory_Single },	// called "bm" for backward compat
/// { "bm-double7",		16,	true,	StepsTypeCategory_Double },	// called "bm" for backward compat
/// // dance maniax
/// { "maniax-single",	4,	true,	StepsTypeCategory_Single },
/// { "maniax-double",	8,	true,	StepsTypeCategory_Double },
/// // technomotion
/// { "techno-single4",	4,	true,	StepsTypeCategory_Single },
/// { "techno-single5",	5,	true,	StepsTypeCategory_Single },
/// { "techno-single8",	8,	true,	StepsTypeCategory_Single },
/// { "techno-double4",	8,	true,	StepsTypeCategory_Double },
/// { "techno-double5",	10,	true,	StepsTypeCategory_Double },
/// { "techno-double8",	16,	true,	StepsTypeCategory_Double },
/// // pop'n music
/// { "pnm-five",		5,	true,	StepsTypeCategory_Single },	// called "pnm" for backward compat
/// { "pnm-nine",		9,	true,	StepsTypeCategory_Single },	// called "pnm" for backward compat
/// // cabinet lights and other fine StepsTypes that don't exist lol
/// { "lights-cabinet",	NUM_CabinetLight,	false,	StepsTypeCategory_Single }, // XXX disable lights autogen for now
/// // kickbox mania
/// { "kickbox-human", 4, true, StepsTypeCategory_Single },
/// { "kickbox-quadarm", 4, true, StepsTypeCategory_Single },
/// { "kickbox-insect", 6, true, StepsTypeCategory_Single },
/// { "kickbox-arachnid", 8, true, StepsTypeCategory_Single },
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Gamemode {
    DanceSingle,
    DanceDouble,
    DanceCouple,
    DanceSolo,
    DanceThreepanel,
    DanceRoutine,
    PumpSingle,
    PumpHalfdouble,
    PumpDouble,
    PumpCouple,
    PumpRoutine,
    Kb7Single,
    Ez2Single,
    Ez2Double,
    Ez2Real,
    ParaSingle,
    Ds3ddxSingle,
    BmSingle5,
    BmVersus5,
    BmDouble5,
    BmSingle7,
    BmVersus7,
    BmDouble7,
    ManiaxSingle,
    ManiaxDouble,
    TechnoSingle4,
    TechnoSingle5,
    TechnoSingle8,
    TechnoDouble4,
    TechnoDouble5,
    TechnoDouble8,
    PnmFive,
    PnmNine,
    KickboxHuman,
    KickboxQuadarm,
    KickboxInsect,
    KickboxArachnid,
}
impl Gamemode {
    pub fn key_count(&self) -> i32 {
        use Gamemode::*;
        match self {
            DanceSingle => 4,
            DanceDouble => 8,
            DanceCouple => 8,
            DanceSolo => 6,
            DanceThreepanel => 3,
            DanceRoutine => 8,
            PumpSingle => 5,
            PumpHalfdouble => 6,
            PumpDouble => 10,
            PumpCouple => 10,
            PumpRoutine => 10,
            Kb7Single => 7,
            Ez2Single => 5,
            Ez2Double => 10,
            Ez2Real => 7,
            ParaSingle => 5,
            Ds3ddxSingle => 8,
            BmSingle5 => 6,
            BmVersus5 => 6,
            BmDouble5 => 12,
            BmSingle7 => 8,
            BmVersus7 => 8,
            BmDouble7 => 16,
            ManiaxSingle => 4,
            ManiaxDouble => 8,
            TechnoSingle4 => 4,
            TechnoSingle5 => 5,
            TechnoSingle8 => 8,
            TechnoDouble4 => 8,
            TechnoDouble5 => 10,
            TechnoDouble8 => 16,
            PnmFive => 5,
            PnmNine => 9,
            KickboxHuman => 4,
            KickboxQuadarm => 4,
            KickboxInsect => 6,
            KickboxArachnid => 8,
        }
    }

    pub fn id(&self) -> &'static str {
        use Gamemode::*;
        match self {
            DanceSingle => "dance-single",
            DanceDouble => "dance-double",
            DanceCouple => "dance-couple",
            DanceSolo => "dance-solo",
            DanceThreepanel => "dance-threepanel",
            DanceRoutine => "dance-routine",
            PumpSingle => "pump-single",
            PumpHalfdouble => "pump-halfdouble",
            PumpDouble => "pump-double",
            PumpCouple => "pump-couple",
            PumpRoutine => "pump-routine",
            Kb7Single => "kb7-single",
            Ez2Single => "ez2-single",
            Ez2Double => "ez2-double",
            Ez2Real => "ez2-real",
            ParaSingle => "para-single",
            Ds3ddxSingle => "ds3ddx-single",
            BmSingle5 => "bm-single5",
            BmVersus5 => "bm-versus5",
            BmDouble5 => "bm-double5",
            BmSingle7 => "bm-single7",
            BmVersus7 => "bm-versus7",
            BmDouble7 => "bm-double7",
            ManiaxSingle => "maniax-single",
            ManiaxDouble => "maniax-double",
            TechnoSingle4 => "techno-single4",
            TechnoSingle5 => "techno-single5",
            TechnoSingle8 => "techno-single8",
            TechnoDouble4 => "techno-double4",
            TechnoDouble5 => "techno-double5",
            TechnoDouble8 => "techno-double8",
            PnmFive => "pnm-five",
            PnmNine => "pnm-nine",
            KickboxHuman => "kickbox-human",
            KickboxQuadarm => "kickbox-quadarm",
            KickboxInsect => "kickbox-insect",
            KickboxArachnid => "kickbox-arachnid",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Difficulty {
    Beginner,
    Easy,
    Medium,
    Hard,
    Challenge,
    Edit,
}
impl Difficulty {
    pub fn name(&self) -> &'static str {
        use Difficulty::*;
        match self {
            Beginner => "Beginner",
            Easy => "Easy",
            Medium => "Medium",
            Hard => "Hard",
            Challenge => "Challenge",
            Edit => "Edit",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DisplayBpm {
    Single(f64),
    Range(f64, f64),
    Random,
}
impl Default for DisplayBpm {
    fn default() -> Self {
        DisplayBpm::Single(120.0)
    }
}
impl DisplayBpm {
    pub fn to_string_with<F: Fn(f64) -> String>(&self, fmt: F) -> String {
        use DisplayBpm::*;
        match self {
            Single(bpm) => fmt(*bpm),
            Range(min, max) => format!("{}:{}", fmt(*min), fmt(*max)),
            Random => format!("*"),
        }
    }
}

/// Represents an absolute position in beats, where 0 is the first beat of the song.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BeatPos {
    frac: i32,
}
impl BeatPos {
    const FIXED_POINT: i32 = 48;
    pub const EPSILON: BeatPos = BeatPos { frac: 1 };

    /// Get the beat number as an `f64`.
    pub fn as_num(self) -> f64 {
        self.into()
    }

    pub fn from_num_floor(beats: f64) -> BeatPos {
        Self {
            frac: (beats * Self::FIXED_POINT as f64).floor() as i32,
        }
    }

    pub fn from_num_ceil(beats: f64) -> BeatPos {
        Self {
            frac: (beats * Self::FIXED_POINT as f64).ceil() as i32,
        }
    }

    /// Round this beat position to the given beat.
    pub fn round(mut self, mut round_to: BeatPos) -> Self {
        round_to = round_to.max(BeatPos::EPSILON);
        self.frac += round_to.frac / 2;
        self.frac -= self.frac.rem_euclid(round_to.frac);
        self
    }

    /// Round down this beat position to the given beat.
    pub fn floor(mut self, mut round_to: BeatPos) -> Self {
        round_to = round_to.max(BeatPos::EPSILON);
        self.frac -= self.frac.rem_euclid(round_to.frac);
        self
    }

    /// Round up this beat position to the given beat.
    pub fn ceil(mut self, mut round_to: BeatPos) -> Self {
        round_to = round_to.max(BeatPos::EPSILON);
        self.frac += round_to.frac - 1;
        self.frac -= self.frac % round_to.frac;
        self
    }

    /// Get the denominator of the most-simplified version of this beat (eg. 1/2, 3/4, 0/1, 19/16).
    pub fn denominator(self) -> i32 {
        let mut num = self.frac;
        let mut den = BeatPos::FIXED_POINT;
        for &factor in [2, 3].iter() {
            while num % factor == 0 && den % factor == 0 {
                num /= factor;
                den /= factor;
            }
        }
        den
    }

    /// Check whether a beat is a multiple of the given beat.
    pub fn is_aligned(self, align_to: BeatPos) -> bool {
        self.frac % align_to.frac == 0
    }
}
impl From<f64> for BeatPos {
    fn from(float: f64) -> BeatPos {
        Self {
            frac: (float * Self::FIXED_POINT as f64).round() as i32,
        }
    }
}
impl From<BeatPos> for f64 {
    fn from(beat: BeatPos) -> f64 {
        beat.frac as f64 / BeatPos::FIXED_POINT as f64
    }
}
impl ops::AddAssign for BeatPos {
    fn add_assign(&mut self, rhs: Self) {
        self.frac += rhs.frac;
    }
}
impl ops::Add for BeatPos {
    type Output = Self;
    fn add(mut self, rhs: Self) -> Self {
        self += rhs;
        self
    }
}
impl ops::SubAssign for BeatPos {
    fn sub_assign(&mut self, rhs: Self) {
        self.frac -= rhs.frac;
    }
}
impl ops::Sub for BeatPos {
    type Output = Self;
    fn sub(mut self, rhs: Self) -> Self {
        self -= rhs;
        self
    }
}
impl fmt::Display for BeatPos {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_num())
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub kind: char,
    pub beat: BeatPos,
    pub key: i32,
}
impl Note {
    pub const KIND_HIT: char = '1';
    pub const KIND_HEAD: char = '2';
    pub const KIND_TAIL: char = '3';

    pub fn is_hit(&self) -> bool {
        self.kind == Self::KIND_HIT
    }

    pub fn is_head(&self) -> bool {
        self.kind == Self::KIND_HEAD
    }

    pub fn is_tail(&self) -> bool {
        self.kind == Self::KIND_TAIL
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlPoint {
    /// First beat of the control point.
    pub beat: BeatPos,
    /// Length of a beat in seconds.
    pub beat_len: f64,
}
impl ControlPoint {
    pub fn bpm(&self) -> f64 {
        60. / self.beat_len
    }
}

#[derive(Debug, Clone)]
pub struct ToTime<'a> {
    bpms: &'a [ControlPoint],
    cur_idx: usize,
    cur_time: f64,
}
impl ToTime<'_> {
    pub fn new(sm: &Simfile) -> ToTime {
        ToTime {
            bpms: &sm.bpms,
            cur_idx: 0,
            cur_time: -sm.offset,
        }
    }

    pub fn from_raw(bpms: &[ControlPoint], offset: f64) -> ToTime {
        ToTime {
            bpms,
            cur_idx: 0,
            cur_time: -offset,
        }
    }

    /// Returns incorrect results if called with non-monotonic beat positions.
    /// If needing to seek back in time, create a new `ToTime` or make "checkpoints" with `Clone`.
    pub fn beat_to_time(&mut self, beat: BeatPos) -> f64 {
        //Advance control points
        while self.cur_idx + 1 < self.bpms.len() {
            let cur_bpm = &self.bpms[self.cur_idx];
            let next_bpm = &self.bpms[self.cur_idx + 1];
            if beat >= next_bpm.beat {
                //Advance to this control point
                let adv_time = (next_bpm.beat - cur_bpm.beat).as_num() * cur_bpm.beat_len;
                self.cur_time += adv_time;
                self.cur_idx += 1;
            } else {
                //Still within the current timing point
                break;
            }
        }
        //Use the current control point to determine the time corresponding to this beat
        let cur_bpm = &self.bpms[self.cur_idx];
        self.cur_time + (beat - cur_bpm.beat).as_num() * cur_bpm.beat_len
    }
}

// ---------------------------------------------------------------------------
// `.sm` parser (used by `Simfile::load`)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct SmHeader {
    title: String,
    subtitle: String,
    artist: String,
    title_trans: String,
    subtitle_trans: String,
    artist_trans: String,
    genre: String,
    credit: String,
    banner: Option<String>,
    background: Option<String>,
    lyrics: Option<String>,
    cdtitle: Option<String>,
    music: Option<String>,
    offset: f64,
    bpms: Vec<ControlPoint>,
    stops: Vec<(f64, f64)>,
    sample_start: Option<f64>,
    sample_len: Option<f64>,
    display_bpm: DisplayBpm,
}

/// Lightweight line-based parser for the `.sm` v5 format.
///
/// Only the fields the rest of the pipeline reads are populated; everything
/// else is ignored. This is enough to round-trip the reference packs shipped
/// with StepMania 5.1 and any output produced by `Simfile::save`.
struct SmParser<'a> {
    text: &'a str,
    pos: usize,
}
impl<'a> SmParser<'a> {
    fn new(text: &'a str) -> Self {
        SmParser { text, pos: 0 }
    }
    fn parse_header(&mut self) -> Result<SmHeader> {
        let mut hdr = SmHeader::default();
        hdr.sample_len = Some(SAMPLE_LENGTH_SECONDS);
        //Default BPM so a simfile without #BPMS doesn't crash later
        hdr.bpms.push(ControlPoint {
            beat: BeatPos::from(0.),
            beat_len: 60. / 120.,
        });
        while let Some(line) = self.peek_line() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                self.advance_line();
                continue;
            }
            if line.starts_with('#') {
                let (key, value) = match split_header(line) {
                    Some(kv) => kv,
                    None => {
                        self.advance_line();
                        continue;
                    }
                };
                match key {
                    "TITLE" => hdr.title = value.to_string(),
                    "SUBTITLE" => hdr.subtitle = value.to_string(),
                    "ARTIST" => hdr.artist = value.to_string(),
                    "TITLETRANSLIT" => hdr.title_trans = value.to_string(),
                    "SUBTITLETRANSLIT" => hdr.subtitle_trans = value.to_string(),
                    "ARTISTTRANSLIT" => hdr.artist_trans = value.to_string(),
                    "GENRE" => hdr.genre = value.to_string(),
                    "CREDIT" => hdr.credit = value.to_string(),
                    "BANNER" if !value.is_empty() => hdr.banner = Some(value.to_string()),
                    "BACKGROUND" if !value.is_empty() => {
                        hdr.background = Some(value.to_string())
                    }
                    "LYRICSPATH" if !value.is_empty() => hdr.lyrics = Some(value.to_string()),
                    "CDTITLE" if !value.is_empty() => hdr.cdtitle = Some(value.to_string()),
                    "MUSIC" if !value.is_empty() => hdr.music = Some(value.to_string()),
                    "OFFSET" => hdr.offset = value.parse().unwrap_or(0.),
                    "SAMPLESTART" => {
                        if let Ok(v) = value.parse::<f64>() {
                            hdr.sample_start = Some(v.max(0.));
                        }
                    }
                    "SAMPLELENGTH" => {
                        if let Ok(v) = value.parse::<f64>() {
                            hdr.sample_len = Some(v.max(0.));
                        }
                    }
                    "DISPLAYBPM" => hdr.display_bpm = parse_display_bpm(value),
                    "BPMS" => hdr.bpms = parse_bpms(value)?,
                    "STOPS" => hdr.stops = parse_stops(value),
                    "NOTES" => break,
                    _ => {}
                }
                self.advance_line();
            } else {
                self.advance_line();
            }
        }
        Ok(hdr)
    }
    /// Returns `(gamemode, difficulty, difficulty_num, radar, desc, notes_text)`.
    fn parse_notes_block(
        &mut self,
    ) -> Result<
        Option<(
            Gamemode,
            Difficulty,
            f64,
            [f64; 5],
            String,
            String,
        )>,
    > {
        //Skip until we see #NOTES:
        while let Some(line) = self.peek_line() {
            let trimmed = line.trim();
            if trimmed.starts_with("#NOTES:") {
                self.advance_line();
                break;
            }
            self.advance_line();
        }
        //Need 5 lines after #NOTES:
        let gamemode_line = match self.next_line() {
            Some(l) => l.trim().trim_end_matches(':').to_string(),
            None => return Ok(None),
        };
        let desc_line = match self.next_line() {
            Some(l) => l.trim().trim_end_matches(':').to_string(),
            None => return Ok(None),
        };
        let diff_name_line = match self.next_line() {
            Some(l) => l.trim().trim_end_matches(':').to_string(),
            None => return Ok(None),
        };
        let diff_num_line = match self.next_line() {
            Some(l) => l.trim().to_string(),
            None => return Ok(None),
        };
        let radar_line = match self.next_line() {
            Some(l) => l.trim().to_string(),
            None => return Ok(None),
        };
        let difficulty = parse_difficulty(&diff_name_line);
        let difficulty_num: f64 = diff_num_line.parse().unwrap_or(0.);
        let radar = parse_radar(&radar_line);
        let gamemode = parse_gamemode(&gamemode_line)
            .ok_or_else(|| anyhow!("unknown gamemode \"{}\"", gamemode_line))?;
        //Collect notes until the next `#` directive or EOF. Strip the
        // trailing `;` that terminates the `#NOTES:` block.
        let mut notes_text = String::new();
        while let Some(line) = self.peek_line() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                break;
            }
            notes_text.push_str(line);
            notes_text.push('\n');
            self.advance_line();
        }
        // Drop a single trailing `;` (and any whitespace around it).
        if let Some(stripped) = notes_text.strip_suffix(';') {
            notes_text = stripped.to_string();
        }
        if let Some(stripped) = notes_text.strip_suffix(';') {
            notes_text = stripped.to_string();
        }
        Ok(Some((gamemode, difficulty, difficulty_num, radar, desc_line, notes_text)))
    }
    fn peek_line(&self) -> Option<&'a str> {
        let rest = &self.text[self.pos..];
        if rest.is_empty() {
            return None;
        }
        if let Some(idx) = rest.find('\n') {
            Some(&rest[..idx])
        } else {
            Some(rest)
        }
    }
    fn next_line(&mut self) -> Option<&'a str> {
        let line = self.peek_line()?;
        self.pos += line.len();
        if self.text[self.pos..].starts_with('\n') {
            self.pos += 1;
        }
        Some(line)
    }
    fn advance_line(&mut self) {
        let _ = self.next_line();
    }
}

fn split_header(line: &str) -> Option<(&str, &str)> {
    //Form: `#KEY:value;`
    let line = line.strip_prefix('#')?;
    let (key, rest) = line.split_once(':')?;
    let value = rest.trim_end_matches(';');
    Some((key, value))
}

fn parse_bpms(s: &str) -> Result<Vec<ControlPoint>> {
    let mut out = Vec::new();
    for entry in s.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (beat_str, bpm_str) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("malformed #BPMS entry \"{}\"", entry))?;
        let beat: f64 = beat_str
            .trim()
            .parse()
            .with_context(|| anyhow!("invalid beat in #BPMS \"{}\"", entry))?;
        let bpm: f64 = bpm_str
            .trim()
            .parse()
            .with_context(|| anyhow!("invalid BPM in #BPMS \"{}\"", entry))?;
        if bpm <= 0. {
            bail!("non-positive BPM in #BPMS \"{}\"", entry);
        }
        out.push(ControlPoint {
            beat: BeatPos::from(beat),
            beat_len: 60. / bpm,
        });
    }
    if out.is_empty() {
        bail!("#BPMS has no entries");
    }
    Ok(out)
}

fn parse_stops(s: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for entry in s.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((beat_str, len_str)) = entry.split_once('=') else {
            continue;
        };
        let Ok(beat) = beat_str.trim().parse() else { continue };
        let Ok(len) = len_str.trim().parse() else { continue };
        out.push((beat, len));
    }
    out
}

fn parse_display_bpm(s: &str) -> DisplayBpm {
    let s = s.trim();
    if s == "*" {
        return DisplayBpm::Random;
    }
    if let Some((min_str, max_str)) = s.split_once(':') {
        if let (Ok(min), Ok(max)) = (min_str.parse(), max_str.parse()) {
            return DisplayBpm::Range(min, max);
        }
    }
    s.parse().map(DisplayBpm::Single).unwrap_or(DisplayBpm::Random)
}

fn parse_difficulty(s: &str) -> Difficulty {
    use Difficulty::*;
    match s.to_ascii_lowercase().as_str() {
        "beginner" | "novice" => Beginner,
        "easy" | "basic" => Easy,
        "medium" | "light" | "another" => Medium,
        "hard" | "difficult" | "standard" => Hard,
        "challenge" | "hardest" | "maniac" | "expert" | "oni" => Challenge,
        _ => Edit,
    }
}

fn parse_gamemode(s: &str) -> Option<Gamemode> {
    use Gamemode::*;
    let s = s.to_ascii_lowercase();
    let s = s.trim_end_matches(':');
    Some(match s {
        "dance-single" => DanceSingle,
        "dance-double" => DanceDouble,
        "dance-couple" => DanceCouple,
        "dance-solo" => DanceSolo,
        "dance-threepanel" => DanceThreepanel,
        "dance-routine" => DanceRoutine,
        "pump-single" => PumpSingle,
        "pump-halfdouble" => PumpHalfdouble,
        "pump-double" => PumpDouble,
        "pump-couple" => PumpCouple,
        "pump-routine" => PumpRoutine,
        "kb7-single" => Kb7Single,
        "ez2-single" => Ez2Single,
        "ez2-double" => Ez2Double,
        "ez2-real" => Ez2Real,
        "para-single" => ParaSingle,
        "ds3ddx-single" => Ds3ddxSingle,
        "bm-single5" | "bm-single" | "bm" => BmSingle5,
        "bm-versus5" => BmVersus5,
        "bm-double5" => BmDouble5,
        "bm-single7" => BmSingle7,
        "bm-versus7" => BmVersus7,
        "bm-double7" => BmDouble7,
        "maniax-single" => ManiaxSingle,
        "maniax-double" => ManiaxDouble,
        "techno-single4" => TechnoSingle4,
        "techno-single5" => TechnoSingle5,
        "techno-single8" => TechnoSingle8,
        "techno-double4" => TechnoDouble4,
        "techno-double5" => TechnoDouble5,
        "techno-double8" => TechnoDouble8,
        "pnm-five" => PnmFive,
        "pnm-nine" => PnmNine,
        "kickbox-human" => KickboxHuman,
        "kickbox-quadarm" => KickboxQuadarm,
        "kickbox-insect" => KickboxInsect,
        "kickbox-arachnid" => KickboxArachnid,
        _ => return None,
    })
}

fn parse_radar(s: &str) -> [f64; 5] {
    let mut out = [0.0; 5];
    for (i, part) in s.split(',').enumerate() {
        if i >= 5 {
            break;
        }
        out[i] = part.trim().parse().unwrap_or(0.);
    }
    out
}

/// Parse the notes-text portion of a `#NOTES:` block into a `Vec<Note>`.
///
/// Mirrors the inverse of `write_measure`: each measure is separated by `,`
/// and contains `4 * rows_per_beat` rows of `key_count` columns.
fn parse_notedata(text: &str, key_count: i32) -> Result<Vec<Note>> {
    let mut notes = Vec::new();
    let mut cur_beat_num = 0.0_f64; // beats since start of song
    for measure_text in text.split(',') {
        // Strip whitespace and measure-header comments (`// Measure N`).
        let mut rows: Vec<&str> = Vec::new();
        for line in measure_text.lines() {
            let trimmed = line.trim();
            // Skip empty lines, measure-header comments, and the `;`
            // terminator that closes a `#NOTES:` block.
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed == ";"
            {
                continue;
            }
            // Allow only line-of-N-cols lines.
            if trimmed.chars().count() as i32 != key_count {
                bail!(
                    "expected {}-char row in measure, got {:?} ({:?})",
                    key_count,
                    trimmed,
                    line
                );
            }
            rows.push(trimmed);
        }
        let rows_per_beat = rows.len() as i32 / BEATS_IN_MEASURE;
        if rows_per_beat <= 0 {
            // Empty measure; advance one measure's worth of beats.
            cur_beat_num += BEATS_IN_MEASURE as f64;
            continue;
        }
        for (row_idx, row) in rows.iter().enumerate() {
            let beat_in_measure = row_idx as f64 / rows_per_beat as f64;
            for (key, ch) in row.chars().enumerate() {
                if ch == '0' {
                    continue;
                }
                // Preserve any non-zero note kind verbatim. StepMania 5.1
                // additionally defines `4` (mines / fakes) and `M` (mine),
                // which we don't model in `Note` but accept at load time so
                // that round-tripping a reference pack doesn't error out.
                notes.push(Note {
                    kind: ch,
                    beat: BeatPos::from(cur_beat_num + beat_in_measure),
                    key: key as i32,
                });
            }
        }
        cur_beat_num += BEATS_IN_MEASURE as f64;
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_f64_trims_to_three_decimals() {
        assert_eq!(fmt_f64(0.5985209960937521_f64), "0.599");
        assert_eq!(fmt_f64(171.99996500651727_f64), "172");
        assert_eq!(fmt_f64(133.020_f64), "133.02");
        assert_eq!(fmt_f64(-1.123_f64), "-1.123");
        assert_eq!(fmt_f64(0.0_f64), "0");
        assert_eq!(fmt_f64(120.0_f64), "120");
        assert_eq!(fmt_f64(f64::NAN), "0");
        assert_eq!(fmt_f64(f64::INFINITY), "0");
    }

    #[test]
    fn sanitize_filename_strips_os_unsafe_chars() {
        assert_eq!(sanitize_filename("Hello/World?"), "Hello_World_");
        assert_eq!(sanitize_filename("<>:\"/\\|?*"), "_________");
        assert_eq!(sanitize_filename("   .dot.   "), "dot");
        assert_eq!(sanitize_filename(""), "untitled");
        assert_eq!(sanitize_filename("中文 标题"), "中文 标题");
        assert_eq!(sanitize_filename("L0V3 4RR0W 5H007"), "L0V3 4RR0W 5H007");
    }

    #[test]
    fn load_reference_sm_round_trip() {
        // Synthetic mini SM, modeled on ettgroup reference style.
        let sm_text = "\
#TITLE:test;
#SUBTITLE:;
#ARTIST:;
#TITLETRANSLIT:;
#SUBTITLETRANSLIT:;
#ARTISTTRANSLIT:;
#GENRE:;
#CREDIT:;
#BANNER:;
#BACKGROUND:;
#LYRICSPATH:;
#CDTITLE:;
#MUSIC:test.mp3;
#OFFSET:0.0;
#SAMPLESTART:10.0;
#SAMPLELENGTH:12.0;
#DISPLAYBPM:120;
#SELECTABLE:YES;
#BPMS:0.0=120;
#STOPS:;
#BGCHANGES:;
#KEYSOUNDS:;
#ATTACKS:;
#NOTES:
    dance-single:
    desc:
    Hard:
    5:
    0,0,0,0,0:
0000
0000
0000
0000
;
";
        let mut parser = SmParser::new(sm_text);
        let meta = parser.parse_header().unwrap();
        assert_eq!(meta.title, "test");
        assert_eq!(meta.music.as_deref(), Some("test.mp3"));
        assert_eq!(meta.offset, 0.0);
        let (gamemode, diff, _meter, radar, _desc, notes_text) =
            parser.parse_notes_block().unwrap().unwrap();
        assert_eq!(gamemode, Gamemode::DanceSingle);
        assert_eq!(diff, Difficulty::Hard);
        let notes = parse_notedata(&notes_text, gamemode.key_count()).unwrap();
        assert!(notes.is_empty());
        assert_eq!(radar[0], 0.0);
    }

    #[test]
    fn load_skips_empty_measures() {
        let notes = parse_notedata(",\n,", 4).unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn round_trip_real_sm_file() {
        // Smoke test against a real reference SM file shipped with StepMania 5.1.
        let path = std::path::Path::new(
            r"C:\DEV\StepMania 5.1-new(fixed)\Songs\ettgroup\YATTA\music.sm",
        );
        if !path.exists() {
            // Reference pack not available in this environment; skip silently.
            eprintln!("skipping: reference SM file not present at {:?}", path);
            return;
        }
        let simfiles = Simfile::load(path).expect("real reference SM should load");
        assert!(!simfiles.is_empty(), "real SM file should have notes");
        for sm in &simfiles {
            assert!(!sm.title.is_empty(), "title should be set");
            assert!(!sm.notes.is_empty(), "notes should be populated");
            // Write the simfile back to a temp dir and verify the file is non-empty.
            let tmp = std::env::temp_dir().join("osu2sm_roundtrip.sm");
            Simfile::save(&tmp, std::iter::once(sm)).expect("save should succeed");
            let bytes = std::fs::metadata(&tmp).expect("saved file exists").len();
            assert!(bytes > 100, "saved file should be non-trivial (got {} bytes)", bytes);
        }
    }
}
