//! `osu2sm-gui` — a dead-simple osu! → StepMania converter front-end.
//!
//! Three fields: an osu! input directory, an SM output directory, and a
//! "Convert" button. That's the whole UI.

#![cfg(feature = "gui")]

use osu2sm::node::ConcreteNode;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

struct App {
    osu_input: String,
    sm_output: String,
    status: String,
    output: Arc<Mutex<String>>,
    /// Shared with the worker thread so it can flip `false` on its way out.
    running: Arc<AtomicBool>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            osu_input: String::new(),
            sm_output: String::new(),
            status: "就绪".into(),
            output: Arc::new(Mutex::new(String::new())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("osu! → StepMania");
            ui.add_space(12.0);

            ui.label("osu! 输入目录(包含 .osu 文件的目录或 osu! Songs 根):");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.osu_input)
                        .hint_text(r"D:\osu!\Songs  或  D:\osu!\Songs\12345 Artist - Title")
                        .desired_width(ui.available_width() - 80.0),
                );
                if ui.button("浏览…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.osu_input = path_to_string(&p);
                    }
                }
            });

            ui.add_space(6.0);
            ui.label("SM 输出目录(StepMania Songs 下的某个分组,如 .../Songs/MyPack):");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.sm_output)
                        .hint_text(r"D:\StepMania 5.1\Songs\MyPack")
                        .desired_width(ui.available_width() - 80.0),
                );
                if ui.button("浏览…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.sm_output = path_to_string(&p);
                    }
                }
            });

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() / 2.0 - 60.0);
                let running = self.running.load(Ordering::SeqCst);
                let btn = ui.add_enabled(
                    !running,
                    egui::Button::new(if running { "转换中…" } else { "▶  转换" })
                        .min_size(egui::vec2(120.0, 36.0)),
                );
                if btn.clicked() {
                    self.start_run();
                }
            });

            ui.add_space(12.0);
            ui.label(format!("状态: {}", self.status));
            ui.add_space(4.0);
            let output = self.output.lock().unwrap().clone();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    egui::TextEdit::multiline(&mut output.clone())
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .show(ui);
                });
        });
        if self.running.load(Ordering::SeqCst) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

impl App {
    fn start_run(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }
        let input = self.osu_input.trim().to_string();
        let output_dir = self.sm_output.trim().to_string();
        if input.is_empty() {
            self.status = "请先选择 osu! 输入目录".into();
            return;
        }
        if output_dir.is_empty() {
            self.status = "请先选择 SM 输出目录".into();
            return;
        }

        let mut opts = osu2sm::default_opts();
        for node in opts.nodes.iter_mut() {
            match node {
                ConcreteNode::OsuLoad(n) => n.input = input.clone(),
                ConcreteNode::SimfileWrite(n) => n.output = output_dir.clone(),
                _ => {}
            }
        }
        opts.log_stderr = true;
        opts.log_stdout = false;
        opts.log_file = false;
        opts.log = "info".into();

        self.output.lock().unwrap().clear();
        self.running.store(true, Ordering::SeqCst);
        self.status = "转换中…".into();
        let buf = Arc::clone(&self.output);
        let running = Arc::clone(&self.running);
        thread::spawn(move || {
            let result = osu2sm::run_pipeline(opts);
            let mut g = buf.lock().unwrap();
            match result {
                Ok(()) => g.push_str("\n[完成]\n"),
                Err(e) => g.push_str(&format!("\n[失败] {:#}\n", e)),
            }
            drop(g);
            // Flip the button back on regardless of success/failure.
            running.store(false, Ordering::SeqCst);
        });
    }
}

fn path_to_string(p: &PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// Load a CJK-capable system font on Windows so the GUI can render
/// Chinese (and Japanese/Korean) text instead of tofu boxes.
///
/// We try a handful of well-known Windows fonts in order of preference.
/// If none are found we silently fall back to egui's built-in Latin-only
/// fonts — the GUI still works, but non-ASCII text will be unreadable.
fn install_cjk_fonts(ctx: &egui::Context) {
    let candidates = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simfang.ttf",
        "C:\\Windows\\Fonts\\NotoSansCJK-Regular.ttc",
    ];

    let mut fonts = egui::FontDefinitions::default();
    let mut installed = false;
    for path in &candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            installed = true;
            break;
        }
    }
    if !installed {
        // No CJK font available — leave default fonts in place.
        ctx.set_fonts(fonts);
        return;
    }
    // Put CJK first so glyphs are picked before the default fallback.
    fonts.families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts.families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "osu2sm",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            install_cjk_fonts(&cc.egui_ctx);
            Box::new(App::default())
        }),
    )
}