# osu2sm-NEXT

A reworked fork of `osu2sm` that decouples from the osu! install layout, produces StepMania-5.1-conformant simfiles, and ships a GUI front-end.

The conversion core (`rekey` / `remap` / `rate` / `select` / `align` / `space` / `simultaneous` / `filter` / `pipe`) is unchanged from upstream — every node still produces the same in-memory `Simfile` representation it always did. What changed is the *edges*: how simfiles get in, how they get out, and how the user drives the tool.

# What's new

- **Decoupled input.** `OsuLoad::fix_input` defaults to `false`, and the auto-detect of an osu! install is opt-in. You can point the tool at any folder of `.osu` files — no need for a full osu! installation nearby.
- **`SmLoad` source.** A new `SmLoad` node reads existing `.sm` files (e.g. a StepMania song group) and feeds them through the same downstream pipeline. Useful for re-rating, re-keying or simply re-packing an existing collection.
- **Decoupled output.** `SimfileWrite::in_place` defaults to `false`, and the auto-detect of a StepMania install is opt-in. You can write to any folder.
- **Standard StepMania layout.** Simfiles are written as `<group>/<song>/<title>.sm`, matching the convention used by the reference packs under `StepMania 5.1/Songs/<group>/<song>/<files>`. A `FlatInGroup` layout is available for quick flat dumps.
- **SM 5.1 field cleanup.** The output drops the upstream-osu2sm attribution comment, normalises `f64` precision (3 decimals, trailing zeros trimmed), clamps `#SAMPLESTART` to non-negative, fixes `#SAMPLELENGTH` to a 12-second preview instead of the full song length, and empties the `*_TRANSLIT` and `CREDIT` fields by default.
- **Graphical front-end.** `cargo build --features gui` produces `osu2sm-gui`, an `eframe` window with live status, dry-run preview, and config load/save.
- **`dry_run` mode.** Flip the toggle and the writer logs every file it *would* write without touching the filesystem.

# Getting started

There are two binaries.

## CLI

```
cargo build --release
./target/release/osu2sm examples/default.config.txt
```

If no config path is supplied, the CLI looks for `<exe-name>.config.txt` next to the binary, falling back to a freshly written default.

## GUI

```
cargo build --release --features gui
./target/release/osu2sm-gui
```

The GUI shows the current RON config inline. Edit it, click **Reload from text**, then **▶ Run** (or **Preview (dry-run)**).

# Configuration

The config is a RON document. See `examples/default.config.txt` for the full annotated example. The two source options are mutually exclusive at the top of the pipeline:

```ron
// Either:
OsuLoad((input: "C:/path/to/osu/Songs", ...)),
// ...or:
SmLoad((input: "C:/StepMania 5.1/Songs/ettgroup", recursive: true, ...)),
```

…and the sink is configured with:

```ron
SimfileWrite((
    output: "C:/StepMania 5.1/Songs/MyPack",
    layout: PerSongFolder,        // or FlatInGroup
    copy: [Copy, Symlink, AssertIdentical],
    in_place: false,
    cleanup: false,
    fix_output: false,
    dry_run: false,
)),
```

# Output format

A converted `.sm` looks like this (key fields shown):

```
#TITLE:L0V3;
#SUBTITLE:;
#ARTIST:E0ri4;
#TITLETRANSLIT:;
#SUBTITLETRANSLIT:;
#ARTISTTRANSLIT:;
#CREDIT:;
#BACKGROUND:bg.jpg;
#MUSIC:audio.mp3;
#OFFSET:0.599;
#SAMPLESTART:0;
#SAMPLELENGTH:12.000;
#DISPLAYBPM:172;
#BPMS:0=172;
...
//---------------dance-single - Hard----------------
#NOTES:
    dance-single:
    ...
```

Compare against the reference pack under `StepMania 5.1/Songs/ettgroup/<song>/<title>.sm` for the conventions this codebase targets.

# License

GPL-3.0 — see `LICENSE`.