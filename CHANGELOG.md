# 0.2.0

## Decoupling

- `OsuLoad::fix_input` defaults to `false`. The osu! install auto-detect
  is still available but only runs when the user opts in. The CLI's stdin
  prompt no longer assumes the source is an osu! song folder.
- `SimfileWrite::in_place` defaults to `false` (was `true`). The
  StepMania install auto-detect (`fix_output`) also defaults to `false`.
- New `SmLoad` node: load existing `.sm` files and feed them through the
  same `Rekey` → `Rate` → `Select` → ... pipeline as osu! sources.
- The hardcoded `Songs/Osu` group is gone; the StepMania base finder
  defaults to `Songs` (no implicit group name).

## Output

- New `OutputLayout` field on `SimfileWrite`:
  `PerSongFolder` (default — matches `<group>/<song>/<title>.sm`) and
  `FlatInGroup` (writes directly under `<output>/`).
- Output filename is now `<sanitized_title>.sm`, with extra `-<gamemode>-
  <diff>` suffix when a single song produces multiple simfiles. The
  upstream `osu2sm-<music>.sm` prefix is gone.
- `cleanup` now removes every `.sm` / `.ssc` under the output, not just
  the legacy `osu2sm-*.sm` files.
- New `dry_run` flag: logs every would-be file path without writing.
- Default `copy` order changed from
  `[Hardlink, Copy, Symlink, AssertIdentical]` to
  `[Copy, Symlink, AssertIdentical]`. `Hardlink` is no longer the
  default because it can't cross drives and is invisible to most users.

## SM 5.1 field cleanup

- Removed the `// Simfile converted from osu! automatically...` header
  comment.
- Added an `fmt_f64` helper. `#OFFSET`, `#BPMS`, `#DISPLAYBPM` and the
  sample fields are now formatted with up to 3 decimal places and
  trailing zeros trimmed.
- `#SAMPLESTART` is clamped to ≥ 0.
- `#SAMPLELENGTH` defaults to `12.000` (the standard song-wheel preview
  length) instead of using the full song length.
- `#TITLETRANSLIT`, `#SUBTITLETRANSLIT`, `#ARTISTTRANSLIT` are emitted
  empty when the transliteration equals the primary field.
- `#CREDIT` no longer gets the osu! mapper's name dropped in by
  default; `OsuLoad` leaves it empty and the user can fill it in if
  desired.
- Each `#NOTES:` block is preceded by a
  `//---------------<gamemode> - <diff>----------------` visual separator,
  matching the convention in the reference packs.

## GUI

- `osu2sm-gui` is now a single-purpose front-end: osu! 4K mania →
  `dance-single`. The pipeline is built in code by
  `osu2sm::pipeline_4k`, so the window has no RON editor and no access to
  `standard`, `Remap`, `Align`, `Space`, `SmLoad`, `in_place` or
  `cleanup`.
- On Windows the binary is linked with the `windows` subsystem in debug and
  release alike, so no console window ever appears on launch. All feedback
  goes to the in-window log panel; panics are appended to
  `%LOCALAPPDATA%\osu2sm\crash.log` and shown in a dialog, and a failed start
  is reported in a dialog as well.
- The input folder is scanned before converting (song count, chart count,
  4K count, skipped count). Conversion runs on a worker thread with a
  progress bar and a cancel button that stops between beatmapsets, and the
  window state (paths + toggles) is persisted to
  `%LOCALAPPDATA%\osu2sm\gui-state.txt`.
- New `SimfileWrite::overwrite` (default `true`). The "跳过已存在" checkbox
  sets it to `false`, making re-runs incremental.
- New `osu2sm::progress` module: a process-global sink that is a no-op
  unless a `Progress` is installed, so the CLI is unaffected.

## Architecture

- The crate is now a library (`osu2sm`) with two binaries: `osu2sm` (CLI)
  and `osu2sm-gui` (GUI). The shared logic lives in
  `osu2sm::bin_shared` and the runner is exposed as
  `osu2sm::run_pipeline(Opts)`.
- New `Simfile::load(&Path) -> Result<Vec<Simfile>>` for reading
  existing `.sm` files. Used by `SmLoad`.

# TODO (carryover from 0.1.0)

- Check beatmapset 30876, "Hacking to the gate", since audio is
  sometimes not playing in StepMania.
- Check beatmapset 22164, "Firework", since the background image
  sometimes doesn't show up.

# Maybe?

- Use difficulties from `osu!.db`.
- Use `.ssc` instead of `.sm`.
- Perhaps convert taiko and ctb.
- Abort parsing quickly if osu! gamemode or keycount is not compatible.
- Use `bumpalo` for fastness.
- Apply text transformations to difficulty names.