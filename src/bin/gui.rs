//! `osu2sm-gui` — osu! 4K mania → StepMania converter front-end.
//!
//! The user does three things: pick the osu! song folder, pick the StepMania
//! song group, press 转换. Everything else (pipeline shape, copy method,
//! cleanup, in-place) is pinned to the fork's single purpose and deliberately
//! not exposed — see `osu2sm::pipeline_4k`.
//!
//! Notes for whoever touches this next:
//!
//! * On Windows the binary is linked with the **windows** subsystem, so there
//!   is no console window. All feedback goes through the `log` facade into
//!   [`UiLogger`] → the log panel at the bottom of the window, and panics are
//!   appended to `%LOCALAPPDATA%\osu2sm\crash.log` and shown in a dialog.
//! * `flexi_logger` is intentionally *not* used here. `log` accepts exactly one
//!   global backend; ours is installed first, so `run_pipeline` sees a logger
//!   is already active (`log::max_level() != Off`) and skips
//!   `Opts::apply_logging` entirely.
//! * The conversion runs on a worker thread. The UI thread only polls three
//!   channels (log lines, run result, scan result) plus a shared
//!   [`Progress`], so the window never blocks.

#![cfg(feature = "gui")]
// Windows: never link a console window, in debug or release. The log panel and
// `crash.log` cover everything a console would show, so `cargo run` behaves the
// same as the shipped exe. Flip this back to
// `cfg_attr(not(debug_assertions), ...)` if you want stderr while debugging.
#![cfg_attr(windows, windows_subsystem = "windows")]

use osu2sm::progress::{self, Progress};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- palette --

const BG: egui::Color32 = egui::Color32::from_rgb(0x14, 0x14, 0x1A);
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x1E, 0x1E, 0x26);
const PANEL_HOVER: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2A, 0x35);
const LOG_BG: egui::Color32 = egui::Color32::from_rgb(0x0F, 0x0F, 0x13);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x66, 0xAA);
const ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(0x50, 0x28, 0x38);
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xE2, 0xE2, 0xE8);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x8A, 0x8A, 0x99);
const OK: egui::Color32 = egui::Color32::from_rgb(0x6C, 0xD8, 0x8A);
const WARN: egui::Color32 = egui::Color32::from_rgb(0xF0, 0xC0, 0x50);
const ERR: egui::Color32 = egui::Color32::from_rgb(0xF0, 0x6B, 0x6B);

/// Upper bound on retained log lines. A full library conversion can emit tens
/// of thousands of lines.
const MAX_LOG_LINES: usize = 4000;

// ------------------------------------------------------------------- log --

struct LogLine {
    level: log::Level,
    msg: String,
}

/// Forwards `log` records to the UI thread. Keeping the fully formatted line
/// means the panel renders exactly what the pipeline logged, including the
/// indentation some nodes use.
struct UiLogger {
    tx: mpsc::Sender<LogLine>,
}

impl log::Log for UiLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            //A closed channel just means the window is gone; ignore it.
            let _ = self.tx.send(LogLine {
                level: record.level(),
                msg: format!("{}", record.args()),
            });
        }
    }

    fn flush(&self) {}
}

fn level_color(level: log::Level) -> egui::Color32 {
    match level {
        log::Level::Error => ERR,
        log::Level::Warn => WARN,
        log::Level::Info => TEXT,
        log::Level::Debug | log::Level::Trace => TEXT_DIM,
    }
}

// ---------------------------------------------------------------- scanning --

/// What the pre-flight scan found. Cheap: it only reads `Mode:` and
/// `CircleSize:` out of each `.osu`.
#[derive(Clone, Debug, Default)]
struct ScanSummary {
    /// Directories containing at least one `.osu` (i.e. beatmapsets).
    songs: usize,
    /// Total `.osu` files.
    osu: usize,
    /// Mania charts with exactly 4 keys — the ones this GUI converts.
    maps_4k: usize,
    /// Everything else (other keycounts, taiko, catch, standard).
    maps_other: usize,
    /// Files that could not be read at all.
    errors: usize,
}

impl ScanSummary {
    fn line(&self) -> String {
        if self.osu == 0 {
            return "没有找到 .osu 文件".to_string();
        }
        if self.errors > 0 {
            format!(
                "{} 首 · {} 个谱面 · 4K {} · 跳过 {} · 读取失败 {}",
                self.songs, self.osu, self.maps_4k, self.maps_other, self.errors
            )
        } else {
            format!(
                "{} 首 · {} 个谱面 · 4K {} · 跳过 {}",
                self.songs, self.osu, self.maps_4k, self.maps_other
            )
        }
    }
}

fn scan_input(root: &Path) -> ScanSummary {
    let mut summary = ScanSummary::default();
    let mut songs: HashSet<PathBuf> = HashSet::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                summary.errors += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let is_osu = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("osu"))
            .unwrap_or(false);
        if !is_osu {
            continue;
        }
        summary.osu += 1;
        if let Some(parent) = path.parent() {
            songs.insert(parent.to_path_buf());
        }
        match inspect_osu(path) {
            Some((3, 4)) => summary.maps_4k += 1,
            Some(_) => summary.maps_other += 1,
            None => summary.errors += 1,
        }
    }
    summary.songs = songs.len();
    summary
}

/// `(Mode, keycount)` for one `.osu`, or `None` if it cannot be read.
fn inspect_osu(path: &Path) -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut mode = 0i32;
    let mut circle_size = 0f64;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Mode:") {
            mode = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("CircleSize:") {
            circle_size = value.trim().parse().unwrap_or(0.);
        }
    }
    Some((mode, circle_size.round() as i32))
}

// --------------------------------------------------------------- app state --

type RunResult = (Result<(), String>, Option<Duration>);

struct App {
    input: String,
    output: String,
    dry_run: bool,
    skip_existing: bool,
    copy_deps: bool,

    logs: VecDeque<LogLine>,
    rx: Receiver<LogLine>,

    progress: Arc<Progress>,
    running: bool,
    cancelled: bool,
    run_rx: Option<Receiver<Result<(), String>>>,
    started: Option<Instant>,
    finished: Option<RunResult>,

    scanning: bool,
    scan_rx: Option<Receiver<ScanSummary>>,
    scan: Option<ScanSummary>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, rx: Receiver<LogLine>) -> Self {
        install_theme(&cc.egui_ctx);
        install_cjk_fonts(&cc.egui_ctx);
        let (input, output, dry_run, skip_existing, copy_deps) = load_state();
        Self {
            input,
            output,
            dry_run,
            skip_existing,
            copy_deps,
            logs: VecDeque::new(),
            rx,
            progress: Arc::new(Progress::new()),
            running: false,
            cancelled: false,
            run_rx: None,
            started: None,
            finished: None,
            scanning: false,
            scan_rx: None,
            scan: None,
        }
    }

    // ----------------------------------------------------------- plumbing --

    /// Drain the log/run/scan channels. Called once per frame, before drawing.
    fn pump(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(line) => {
                    while self.logs.len() >= MAX_LOG_LINES {
                        self.logs.pop_front();
                    }
                    self.logs.push_back(line);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if let Some(rx) = self.run_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.running = false;
                    self.finished = Some((result, self.started.map(|s| s.elapsed())));
                    self.started = None;
                }
                Err(TryRecvError::Empty) => self.run_rx = Some(rx),
                Err(TryRecvError::Disconnected) => {
                    //The worker died without reporting (a panic). The panic
                    //hook has already logged it.
                    self.running = false;
                    self.started = None;
                    self.finished = Some((Err("转换线程意外退出，详见日志".to_string()), None));
                }
            }
        }
        if let Some(rx) = self.scan_rx.take() {
            match rx.try_recv() {
                Ok(summary) => {
                    self.scanning = false;
                    self.scan = Some(summary);
                }
                Err(TryRecvError::Empty) => self.scan_rx = Some(rx),
                Err(TryRecvError::Disconnected) => self.scanning = false,
            }
        }
    }

    /// Accept folders dragged onto the window: fill the first empty field,
    /// otherwise replace the input.
    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        let mut took_one = false;
        for path in dropped {
            if !path.is_dir() {
                continue;
            }
            if self.input.trim().is_empty() {
                self.input = path_to_string(&path);
            } else if self.output.trim().is_empty() {
                self.output = path_to_string(&path);
            } else {
                self.input = path_to_string(&path);
            }
            took_one = true;
        }
        if took_one {
            self.scan = None;
            self.start_scan(ctx);
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        let input = self.input.trim().to_string();
        self.scan = None;
        if input.is_empty() || !Path::new(&input).is_dir() {
            self.scanning = false;
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.scan_rx = Some(rx);
        self.scanning = true;
        thread::spawn(move || {
            let summary = scan_input(Path::new(&input));
            let _ = tx.send(summary);
        });
        ctx.request_repaint();
    }

    fn start_run(&mut self, ctx: &egui::Context) {
        if self.running {
            return;
        }
        let input = self.input.trim().to_string();
        let output = self.output.trim().to_string();
        if input.is_empty() || output.is_empty() {
            self.finished = Some((Err("请先选择输入目录和输出目录".to_string()), None));
            return;
        }
        if !Path::new(&input).is_dir() {
            self.finished = Some((Err(format!("输入目录不存在：{}", input)), None));
            return;
        }
        if !self.dry_run {
            if let Err(err) = std::fs::create_dir_all(&output) {
                self.finished = Some((Err(format!("无法创建输出目录：{:#}", err)), None));
                return;
            }
        }
        save_state(
            &input,
            &output,
            self.dry_run,
            self.skip_existing,
            self.copy_deps,
        );

        let progress = Arc::new(Progress::new());
        progress::install(Arc::clone(&progress));
        self.progress = progress;

        self.logs.clear();
        self.finished = None;
        self.cancelled = false;
        self.started = Some(Instant::now());
        self.running = true;

        let (tx, rx) = mpsc::channel();
        self.run_rx = Some(rx);

        let opts = osu2sm::pipeline_4k(
            &input,
            &output,
            &osu2sm::FourKeyOptions {
                dry_run: self.dry_run,
                skip_existing: self.skip_existing,
                copy_deps: self.copy_deps,
            },
        );
        thread::spawn(move || {
            let result = osu2sm::run_pipeline(opts).map_err(|err| format!("{:#}", err));
            let _ = tx.send(result);
            progress::uninstall();
        });
        ctx.request_repaint();
    }

    // ------------------------------------------------------------ drawing --

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("osu! → StepMania")
                    .size(22.0)
                    .strong()
                    .color(TEXT),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("4K mania → dance-single")
                    .size(12.0)
                    .color(ACCENT),
            );
        });
        ui.label(
            egui::RichText::new("选好两个目录，然后按转换。其它参数已固定为 4K 专用流程。")
                .size(12.0)
                .color(TEXT_DIM),
        );
        ui.add_space(6.0);
        ui.separator();
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let mut rescanned = false;
        if dir_row(ui, "osu! 谱面目录", r"D:\osu!\Songs", &mut self.input) {
            rescanned = true;
        }
        ui.add_space(4.0);
        if dir_row(
            ui,
            "StepMania 输出目录",
            r"D:\StepMania 5.1\Songs\MyPack",
            &mut self.output,
        ) {
            //Nothing to re-scan for the output side.
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.dry_run, "试运行");
            ui.label(
                egui::RichText::new("只列出路径，不写文件")
                    .size(11.0)
                    .color(TEXT_DIM),
            );
            ui.add_space(14.0);
            ui.checkbox(&mut self.skip_existing, "跳过已存在");
            ui.label(
                egui::RichText::new("已有 .sm 的歌曲不再覆盖")
                    .size(11.0)
                    .color(TEXT_DIM),
            );
            ui.add_space(14.0);
            ui.checkbox(&mut self.copy_deps, "复制音频/背景");
        });

        if rescanned {
            let ctx = ui.ctx().clone();
            self.start_scan(&ctx);
        }
    }

    fn actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let label = if self.dry_run { "试运行" } else { "▶  转 换" };
            let primary = egui::Button::new(
                egui::RichText::new(label)
                    .size(15.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            )
            .fill(if self.dry_run { PANEL_HOVER } else { ACCENT })
            .rounding(egui::Rounding::same(8.0))
            .min_size(egui::vec2(150.0, 40.0));
            if ui.add_enabled(!self.running, primary).clicked() {
                self.start_run(ctx);
            }

            if self.running {
                let cancel = egui::Button::new(
                    egui::RichText::new("取消")
                        .size(14.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(ACCENT_DIM)
                .rounding(egui::Rounding::same(8.0))
                .min_size(egui::vec2(86.0, 40.0));
                if ui.add(cancel).clicked() {
                    self.progress.request_cancel();
                    self.cancelled = true;
                }
            } else {
                let rescan = egui::Button::new(egui::RichText::new("扫描输入").size(14.0))
                    .rounding(egui::Rounding::same(8.0))
                    .min_size(egui::vec2(96.0, 40.0));
                if ui.add(rescan).clicked() {
                    self.start_scan(ctx);
                }
            }

            let output = self.output.trim().to_string();
            if !output.is_empty() {
                let open = egui::Button::new(egui::RichText::new("打开输出目录").size(14.0))
                    .rounding(egui::Rounding::same(8.0))
                    .min_size(egui::vec2(116.0, 40.0));
                if ui.add(open).clicked() {
                    let _ = std::fs::create_dir_all(&output);
                    open_folder(Path::new(&output));
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let (done, total, files) = self.progress.counts();
        ui.horizontal(|ui| {
            if self.running {
                let fraction = if total == 0 {
                    0.0
                } else {
                    (done as f32 / total as f32).clamp(0.0, 1.0)
                };
                let bar_width = (ui.available_width() - 260.0).max(140.0);
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(bar_width)
                        .animate(total == 0)
                        .text(format!("{}/{} 首 · 已写 {} 个 .sm", done, total, files)),
                );
                if let Some(started) = self.started {
                    ui.label(
                        egui::RichText::new(format!("{:.1}s", started.elapsed().as_secs_f32()))
                            .color(TEXT_DIM)
                            .size(12.0),
                    );
                }
            } else if self.scanning {
                ui.spinner();
                ui.label(
                    egui::RichText::new("正在扫描输入目录…")
                        .color(TEXT_DIM)
                        .size(12.0),
                );
            } else if let Some((result, elapsed)) = &self.finished {
                match result {
                    Ok(()) => {
                        let took = elapsed
                            .map(|e| format!(" · {:.1}s", e.as_secs_f32()))
                            .unwrap_or_default();
                        //A cancelled run still returns `Ok`: the pipeline stops
                        //cleanly between beatmapsets.
                        let (text, color) = if self.cancelled {
                            (format!("已取消 · 写出 {} 个 .sm{}", files, took), WARN)
                        } else {
                            (format!("完成 · 写出 {} 个 .sm{}", files, took), OK)
                        };
                        ui.label(egui::RichText::new(text).color(color).size(13.0));
                    }
                    Err(message) => {
                        ui.label(egui::RichText::new(message.as_str()).color(ERR).size(13.0));
                    }
                }
            } else if let Some(summary) = &self.scan {
                ui.label(
                    egui::RichText::new(summary.line())
                        .color(TEXT_DIM)
                        .size(12.0),
                );
            } else {
                ui.label(egui::RichText::new("就绪").color(TEXT_DIM).size(12.0));
            }
        });
    }

    fn log_panel(&self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(LOG_BG)
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                let height = ui.available_height().max(90.0);
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(height)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if self.logs.is_empty() {
                            ui.label(
                                egui::RichText::new("（转换日志会出现在这里）")
                                    .color(TEXT_DIM)
                                    .size(12.0),
                            );
                        }
                        for line in &self.logs {
                            ui.label(
                                egui::RichText::new(line.msg.as_str())
                                    .monospace()
                                    .size(12.0)
                                    .color(level_color(line.level)),
                            );
                        }
                    });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        self.handle_drops(ctx);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                self.header(ui);
                ui.add_space(8.0);
                self.form(ui);
                ui.add_space(12.0);
                self.actions(ui, ctx);
                ui.add_space(10.0);
                self.status_bar(ui);
                ui.add_space(8.0);
                self.log_panel(ui);
            });

        if self.running || self.scanning {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }
}

// ---------------------------------------------------------------- widgets --

/// One labelled directory row: label, text field, browse button.
/// Returns `true` when the value changed through the file dialog.
fn dir_row(ui: &mut egui::Ui, label: &str, hint: &str, value: &mut String) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new(label).size(12.0).color(TEXT_DIM));
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 96.0).max(160.0);
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(width)
                .font(egui::FontId::monospace(13.0)),
        );
        if ui.button("浏览…").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                *value = path_to_string(&path);
                changed = true;
            }
        }
    });
    changed
}

// ------------------------------------------------------------ os plumbing --

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn open_folder(path: &Path) {
    if cfg!(target_os = "windows") {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    } else if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("open").arg(path).spawn();
    } else {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// `%LOCALAPPDATA%\osu2sm` on Windows, `~/.config/osu2sm` elsewhere.
fn state_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("osu2sm"))
    } else {
        std::env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".config").join("osu2sm"))
    }
}

fn state_file() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("gui-state.txt"))
}

/// `(input, output, dry_run, skip_existing, copy_deps)`.
fn load_state() -> (String, String, bool, bool, bool) {
    let mut input = String::new();
    let mut output = String::new();
    let mut dry_run = false;
    let mut skip_existing = false;
    let mut copy_deps = true;
    if let Some(path) = state_file() {
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('\t') {
                    match key {
                        "input" => input = value.to_string(),
                        "output" => output = value.to_string(),
                        "dry_run" => dry_run = value == "1",
                        "skip_existing" => skip_existing = value == "1",
                        "copy_deps" => copy_deps = value == "1",
                        _ => {}
                    }
                }
            }
        }
    }
    (input, output, dry_run, skip_existing, copy_deps)
}

fn save_state(input: &str, output: &str, dry_run: bool, skip_existing: bool, copy_deps: bool) {
    let dir = match state_dir() {
        Some(dir) => dir,
        None => return,
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let body = format!(
        "input\t{}\noutput\t{}\ndry_run\t{}\nskip_existing\t{}\ncopy_deps\t{}\n",
        input,
        output,
        u8::from(dry_run),
        u8::from(skip_existing),
        u8::from(copy_deps),
    );
    let _ = std::fs::write(dir.join("gui-state.txt"), body);
}

/// Write panics to disk and show them in a dialog: release builds have no
/// console, so a panic would otherwise look like "the window just closed".
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = format!("osu2sm-gui 崩溃：\n{}", info);
        if let Some(dir) = state_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("crash.log");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write as _;
                let _ = writeln!(file, "{}", message);
                let _ = writeln!(file, "  log: {}", path.display());
            }
        }
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("osu2sm 出错了")
            .set_description(message.as_str())
            .show();
        previous(info);
    }));
}

// ------------------------------------------------------------------- theme --

fn install_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = LOG_BG;
    visuals.faint_bg_color = PANEL;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x7A, 0x2E, 0x50);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.rounding = egui::Rounding::same(6.0);
    }
    visuals.widgets.inactive.weak_bg_fill = PANEL;
    visuals.widgets.hovered.weak_bg_fill = PANEL_HOVER;
    visuals.widgets.hovered.bg_fill = PANEL_HOVER;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_fill = ACCENT;
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::new(22.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(12.5, egui::FontFamily::Monospace),
        ),
        (
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::new(11.5, egui::FontFamily::Proportional),
        ),
    ]
    .into();
    ctx.set_style(style);
}

/// Load a CJK-capable system font so Chinese/Japanese text (and most Windows
/// user paths) render as glyphs instead of tofu boxes. egui ships Latin-only
/// fonts, so this is not optional for a zh-CN user.
fn install_cjk_fonts(ctx: &egui::Context) {
    let candidates = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\msyhl.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/PingFang.ttc",
    ];

    let mut fonts = egui::FontDefinitions::default();
    let mut installed = false;
    for path in &candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("cjk".to_owned(), egui::FontData::from_owned(bytes));
            installed = true;
            break;
        }
    }
    if !installed {
        return;
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// A tiny procedurally drawn osu!-style hitcircle, so the exe has a real icon
/// without shipping (or decoding) an image asset.
fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let centre = (SIZE as f32 - 1.0) / 2.0;
    let outer = centre - 1.0;
    let ring_inner = outer * 0.80;
    let dot = outer * 0.26;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let distance = (dx * dx + dy * dy).sqrt();
            let pixel = if distance <= dot {
                [0xFF, 0xFF, 0xFF, 0xFF]
            } else if distance >= ring_inner && distance <= outer {
                [0xFF, 0x66, 0xAA, 0xFF]
            } else if distance < ring_inner {
                [0x1E, 0x1E, 0x26, 0xFF]
            } else if distance <= outer + 1.0 {
                [0xFF, 0x66, 0xAA, 0x66]
            } else {
                [0, 0, 0, 0]
            };
            let index = ((y * SIZE + x) * 4) as usize;
            rgba[index..index + 4].copy_from_slice(&pixel);
        }
    }
    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

// -------------------------------------------------------------------- main --

fn main() -> eframe::Result<()> {
    let (tx, rx) = mpsc::channel();
    //Install our logger before anything else: `log` takes exactly one global
    //backend, and the pipeline's own `flexi_logger` attempt must lose.
    let _ = log::set_boxed_logger(Box::new(UiLogger { tx }));
    log::set_max_level(log::LevelFilter::Info);
    install_panic_hook();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title("osu2sm — osu! 4K → StepMania")
            .with_icon(Arc::new(app_icon())),
        ..Default::default()
    };

    let result = eframe::run_native(
        "osu2sm",
        options,
        Box::new(move |cc| {
            let mut app = App::new(cc, rx);
            app.start_scan(&cc.egui_ctx);
            Box::new(app)
        }),
    );
    //Without a console, a failed start (no GPU backend, bad driver, ...) would
    //otherwise look like "nothing happened".
    if let Err(err) = &result {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("osu2sm 无法启动")
            .set_description(format!("{:#}", err))
            .show();
    }
    result
}
