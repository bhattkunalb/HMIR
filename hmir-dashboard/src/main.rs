use eframe::egui;
use futures_util::StreamExt;
use hmir_core::telemetry::TelemetryEvent;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc};

#[derive(Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub name: String,
}

#[derive(Clone, Default)]
struct ChatEntry {
    role: String,
    content: String,
    is_error: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Overview,
    Chat,
    Models,
    Logs,
}

pub enum DashboardCmd {
    SwitchModel(String),
    RestartNode,
    ToggleNode(bool),
    OpenDir(String),
    BrowseModels,
    DownloadModel {
        repo_id: String,
        folder_name: String,
    },
    DismountModel,
    SendChat(String),
    ClearChat,
}

pub struct DashboardApp {
    telemetry_receiver: broadcast::Receiver<TelemetryEvent>,
    cmd_sender: mpsc::Sender<DashboardCmd>,
    installed_models: Arc<Mutex<Vec<String>>>,
    chat_history: Arc<Mutex<Vec<ChatEntry>>>,
    api_base_url: String,
    current_tab: Tab,
    mini_mode: bool,
    active_model: String,
    api_active: bool,
    selected_model: String,
    chat_input: String,
    log_filter: String,
    raw_logs: String,
    last_log_refresh: Instant,
    last_telemetry_at: Instant,
    live_temp: f64,
    live_gpu_temp: f64,
    live_ram: f64,
    live_ram_total: f64,
    live_vram: f64,
    live_vram_total: f64,
    live_npu_vram: f64,
    live_tps: f64,
    live_npu: f64,
    live_uptime: u64,
    live_kv: f32,
    live_disk_free: f64,
    live_disk_total: f64,
    cpu_name: String,
    gpu_name: String,
    npu_name: String,
    cpu_cores: u32,
    cpu_threads: u32,
    cpu_l3: f64,
    gpu_driver: String,
    npu_driver: String,
    ram_speed: u32,
    disk_model: String,
    dl_active: bool,
    dl_model: String,
    dl_status: String,
    dl_progress: f32,
}

impl DashboardApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        rx: broadcast::Receiver<TelemetryEvent>,
        cmd_tx: mpsc::Sender<DashboardCmd>,
        models_shared: Arc<Mutex<Vec<String>>>,
        chat_history: Arc<Mutex<Vec<ChatEntry>>>,
        api_base_url: String,
    ) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.window_fill = egui::Color32::from_rgb(5, 5, 7);
        style.visuals.panel_fill = egui::Color32::from_rgb(5, 5, 7);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(15, 18, 23);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(30, 35, 45);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 242, 255);
        style.visuals.selection.bg_fill = egui::Color32::from_rgba_unmultiplied(0, 242, 255, 40);
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        cc.egui_ctx.set_style(style);

        Self {
            telemetry_receiver: rx,
            cmd_sender: cmd_tx,
            installed_models: models_shared,
            chat_history,
            api_base_url,
            current_tab: Tab::Overview,
            mini_mode: false,
            active_model: "NONE".to_string(),
            api_active: false,
            selected_model: String::new(),
            chat_input: String::new(),
            log_filter: String::new(),
            raw_logs: "No logs available yet.".to_string(),
            last_log_refresh: Instant::now() - Duration::from_secs(10),
            last_telemetry_at: Instant::now() - Duration::from_secs(10),
            live_temp: 0.0,
            live_gpu_temp: 0.0,
            live_ram: 0.0,
            live_ram_total: 0.1,
            live_vram: 0.0,
            live_vram_total: 0.1,
            live_npu_vram: 0.0,
            live_tps: 0.0,
            live_npu: 0.0,
            live_uptime: 0,
            live_kv: 0.0,
            live_disk_free: 0.0,
            live_disk_total: 0.1,
            cpu_name: "Detecting CPU...".to_string(),
            gpu_name: "Detecting GPU...".to_string(),
            npu_name: "Detecting NPU...".to_string(),
            cpu_cores: 0,
            cpu_threads: 0,
            cpu_l3: 0.0,
            gpu_driver: "Unknown".to_string(),
            npu_driver: "Unknown".to_string(),
            ram_speed: 0,
            disk_model: "Unknown".to_string(),
            dl_active: false,
            dl_model: String::new(),
            dl_status: String::new(),
            dl_progress: 0.0,
        }
    }

    fn data_root() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hmir")
    }

    fn logs_dir() -> PathBuf {
        Self::data_root().join("logs")
    }

    fn models_dir() -> PathBuf {
        Self::data_root().join("models")
    }

    fn send_chat(&mut self) {
        let prompt = self.chat_input.trim().to_string();
        if prompt.is_empty() {
            return;
        }

        let _ = self.cmd_sender.try_send(DashboardCmd::SendChat(prompt));
        self.chat_input.clear();
        self.current_tab = Tab::Chat;
    }

    fn refresh_logs_if_needed(&mut self, force: bool) {
        if !force && self.last_log_refresh.elapsed() < Duration::from_millis(750) {
            return;
        }

        self.last_log_refresh = Instant::now();
        let mut combined = Vec::new();
        for path in [Self::logs_dir().join("api.log"), Self::logs_dir().join("dashboard_error.log")] {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("log");
                combined.push(format!("===== {} =====\n{}", name, content));
            }
        }

        let merged = if combined.is_empty() {
            "No log files found yet.".to_string()
        } else {
            combined.join("\n\n")
        };

        if self.log_filter.trim().is_empty() {
            self.raw_logs = tail_lines(&merged, 250);
            return;
        }

        let needle = self.log_filter.to_lowercase();
        let filtered = merged
            .lines()
            .filter(|line| line.to_lowercase().contains(&needle))
            .collect::<Vec<_>>()
            .join("\n");
        self.raw_logs = tail_lines(&filtered, 250);
    }

    fn draw_metric_card(ui: &mut egui::Ui, title: &str, value: String, accent: egui::Color32) {
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 25, 180))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 15)))
            .rounding(12.0)
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(160.0, 84.0));
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(title.to_uppercase())
                            .size(10.0)
                            .color(egui::Color32::from_gray(140))
                            .strong()
                            .extra_letter_spacing(1.0),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(value)
                            .size(26.0)
                            .strong()
                            .color(accent),
                    );
                });
            });
    }

    fn draw_status_badge(ui: &mut egui::Ui, active: bool) {
        let (text, fg, bg) = if active {
            (
                "SYSTEM ONLINE",
                egui::Color32::from_rgb(120, 255, 170),
                egui::Color32::from_rgb(24, 54, 37),
            )
        } else {
            (
                "SYSTEM OFFLINE",
                egui::Color32::from_rgb(255, 135, 135),
                egui::Color32::from_rgb(62, 28, 28),
            )
        };

        egui::Frame::none()
            .fill(bg)
            .rounding(999.0)
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).strong().color(fg).size(11.0));
            });
    }

    fn render_overview(&mut self, ui: &mut egui::Ui) {
        ui.heading("Overview");
        ui.label("Native control center for runtime status, model control, local chat, and integrations.");
        ui.add_space(12.0);

        ui.horizontal_wrapped(|ui| {
            Self::draw_metric_card(
                ui,
                "Throughput",
                format!("{:.1} tok/s", self.live_tps),
                egui::Color32::from_rgb(88, 195, 255),
            );
            Self::draw_metric_card(
                ui,
                "NPU Util",
                format!("{:.0}%", self.live_npu),
                egui::Color32::from_rgb(101, 255, 175),
            );
            Self::draw_metric_card(
                ui,
                "CPU Temp",
                format!("{:.1} C", self.live_temp),
                if self.live_temp > 85.0 {
                    egui::Color32::from_rgb(255, 120, 120)
                } else {
                    egui::Color32::from_rgb(225, 235, 245)
                },
            );
            Self::draw_metric_card(
                ui,
                "KV Cache",
                format!("{:.1}%", self.live_kv),
                egui::Color32::from_rgb(255, 195, 90),
            );
        });

        ui.add_space(16.0);
        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.heading("Hardware");
                ui.separator();
                ui.label(format!("CPU: {}", self.cpu_name));
                ui.label(format!(
                    "Cores / Threads: {} / {}",
                    self.cpu_cores, self.cpu_threads
                ));
                ui.label(format!("L3 Cache: {:.1} MB", self.cpu_l3));
                ui.label(format!(
                    "RAM: {:.1} / {:.1} GiB @ {} MT/s",
                    self.live_ram / 1024.0 / 1024.0 / 1024.0,
                    self.live_ram_total / 1024.0 / 1024.0 / 1024.0,
                    self.ram_speed
                ));
                ui.add_space(8.0);
                ui.label(format!("GPU: {}", self.gpu_name));
                ui.label(format!("GPU Driver: {}", self.gpu_driver));
                ui.label(format!(
                    "GPU Memory: {:.1} / {:.1} GiB",
                    self.live_vram / 1024.0 / 1024.0 / 1024.0,
                    self.live_vram_total / 1024.0 / 1024.0 / 1024.0
                ));
                ui.label(format!("GPU Temp: {:.1} C", self.live_gpu_temp));
                ui.add_space(8.0);
                ui.label(format!("NPU: {}", self.npu_name));
                ui.label(format!("NPU Driver: {}", self.npu_driver));
                ui.label(format!(
                    "NPU Allocated Pool: {:.1} GiB",
                    self.live_npu_vram / 1024.0 / 1024.0 / 1024.0
                ));
                ui.add_space(8.0);
                ui.label(format!(
                    "Storage: {:.1} / {:.1} GB free on {}",
                    self.live_disk_free, self.live_disk_total, self.disk_model
                ));
            });

            cols[1].group(|ui| {
                ui.heading("Orchestration");
                ui.separator();

                let models = self.installed_models.lock().unwrap().clone();
                if self.selected_model.is_empty() {
                    if let Some(first) = models.first() {
                        self.selected_model = first.clone();
                    }
                }

                ui.label("Active model");
                ui.code(&self.active_model);
                ui.add_space(8.0);
                ui.label("Select a local model pack");
                egui::ComboBox::from_id_source("dashboard_model_select")
                    .selected_text(if self.selected_model.is_empty() {
                        "No models installed".to_string()
                    } else {
                        self.selected_model.clone()
                    })
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for model in models {
                            ui.selectable_value(&mut self.selected_model, model.clone(), model);
                        }
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Mount").clicked() && !self.selected_model.is_empty() {
                        let _ = self
                            .cmd_sender
                            .try_send(DashboardCmd::SwitchModel(self.selected_model.clone()));
                    }
                    if ui.button("Unmount").clicked() {
                        let _ = self.cmd_sender.try_send(DashboardCmd::DismountModel);
                    }
                    if ui.button("Open Models Folder").clicked() {
                        let _ = self.cmd_sender.try_send(DashboardCmd::BrowseModels);
                    }
                });

                if self.dl_active {
                    ui.add_space(12.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 190, 105),
                        format!(
                            "Download: {} [{}] {:.0}%",
                            self.dl_model, self.dl_status, self.dl_progress
                        ),
                    );
                }

                ui.add_space(14.0);
                ui.heading("Integration Access");
                ui.separator();
                ui.label("Use HMIR anywhere a tool accepts an OpenAI-compatible base URL.");
                ui.code(format!("{}/v1", self.api_base_url));
                ui.label("Suggested local API key: hmir-local");
                ui.label("Suggested model: use the currently mounted model or a known local alias.");
                ui.add_space(8.0);
                ui.label("Works with:");
                ui.label("- Cursor or similar editors with custom OpenAI provider support");
                ui.label("- VS Code extensions that accept OpenAI-compatible endpoints");
                ui.label("- OpenClaw, OpenJarvis, Antigravity, Open WebUI, and custom SDKs");
                ui.label("- Python, JavaScript, and shell clients using the OpenAI API shape");
            });
        });
    }

    fn render_chat(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chat");
        ui.label("Native chat is built into the dashboard, so you do not need a separate browser tab.");
        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            ui.label("Endpoint:");
            ui.code(format!("{}/v1/chat/completions", self.api_base_url));
            ui.separator();
            ui.label("Mounted model:");
            ui.code(self.active_model.clone());
        });

        ui.add_space(12.0);
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(16, 19, 24))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let history = self.chat_history.lock().unwrap().clone();
                        for entry in history {
                            let accent = if entry.is_error {
                                egui::Color32::from_rgb(255, 120, 120)
                            } else if entry.role == "assistant" {
                                egui::Color32::from_rgb(101, 255, 175)
                            } else {
                                egui::Color32::from_rgb(88, 195, 255)
                            };

                            ui.group(|ui| {
                                ui.label(
                                    egui::RichText::new(entry.role.to_uppercase())
                                        .strong()
                                        .color(accent),
                                );
                                ui.label(entry.content);
                            });
                            ui.add_space(8.0);
                        }
                    });
            });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let input = ui.add_sized(
                [ui.available_width() - 160.0, 30.0],
                egui::TextEdit::singleline(&mut self.chat_input)
                    .hint_text("Send a local prompt through HMIR"),
            );
            let enter_pressed = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter_pressed || ui.button("Send").clicked() {
                self.send_chat();
            }
            if ui.button("Clear").clicked() {
                let _ = self.cmd_sender.try_send(DashboardCmd::ClearChat);
            }
        });
    }

    fn render_models(&mut self, ui: &mut egui::Ui) {
        ui.heading("Models");
        ui.label("Install, inspect, and switch model packs without leaving the dashboard.");
        ui.add_space(12.0);

        let suggestions = if self.npu_name.to_lowercase().contains("apple") {
            vec![
                (
                    "MLX Llama 3.1 8B",
                    "mlx-community/Llama-3.1-8B-Instruct-4bit",
                    "llama3.1-8b-mlx",
                ),
                (
                    "MLX Qwen 2.5 7B",
                    "mlx-community/Qwen2.5-7B-Instruct-4bit",
                    "qwen2.5-7b-mlx",
                ),
            ]
        } else {
            vec![
                (
                    "OpenVINO Qwen 2.5 1.5B",
                    "OpenVINO/qwen2.5-1.5b-instruct-int4-ov",
                    "qwen2.5-1.5b-ov",
                ),
                (
                    "OpenVINO Phi-3 Mini",
                    "OpenVINO/Phi-3-mini-4k-instruct-int4-ov",
                    "phi3-mini-ov",
                ),
                (
                    "GGUF Llama 3.2 3B",
                    "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
                    "llama3.2-3b",
                ),
            ]
        };

        ui.label("Recommended packs");
        ui.horizontal_wrapped(|ui| {
            for (name, repo, folder) in suggestions {
                egui::Frame::group(ui.style())
                    .fill(egui::Color32::from_rgb(19, 23, 28))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(210.0, 90.0));
                        ui.label(egui::RichText::new(name).strong());
                        ui.label(folder);
                        ui.add_space(8.0);
                        if ui.button("Install").clicked() {
                            let _ = self.cmd_sender.try_send(DashboardCmd::DownloadModel {
                                repo_id: repo.to_string(),
                                folder_name: folder.to_string(),
                            });
                        }
                    });
            }
        });

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label("Installed models");
            if ui.button("Open Folder").clicked() {
                let _ = self.cmd_sender.try_send(DashboardCmd::BrowseModels);
            }
        });
        ui.separator();

        let models = self.installed_models.lock().unwrap().clone();
        if models.is_empty() {
            ui.label("No local models found yet.");
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for model in models {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&model).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Mount").clicked() {
                                let _ = self.cmd_sender.try_send(DashboardCmd::SwitchModel(model.clone()));
                            }
                        });
                    });
                });
                ui.add_space(8.0);
            }
        });
    }

    fn render_logs(&mut self, ui: &mut egui::Ui) {
        self.refresh_logs_if_needed(false);

        ui.heading("Logs");
        ui.label("Advanced log view across API and dashboard error logs.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.add_sized(
                [ui.available_width() - 180.0, 28.0],
                egui::TextEdit::singleline(&mut self.log_filter).hint_text("grep text"),
            );
            if ui.button("Refresh").clicked() {
                self.refresh_logs_if_needed(true);
            }
            if ui.button("Open Folder").clicked() {
                let _ = self.cmd_sender.try_send(DashboardCmd::OpenDir("logs".to_string()));
            }
        });

        ui.add_space(10.0);
        let mut log_text = self.raw_logs.clone();
        ui.add_sized(
            ui.available_size(),
            egui::TextEdit::multiline(&mut log_text)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
    }
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.telemetry_receiver.try_recv() {
            match event {
                TelemetryEvent::HardwareState {
                    cpu_temp,
                    gpu_temp,
                    ram_used,
                    ram_total,
                    vram_used,
                    vram_total,
                    npu_vram_used,
                    tps,
                    npu_util,
                    node_uptime,
                    kv_cache,
                    cpu_name,
                    gpu_name,
                    npu_name,
                    cpu_cores,
                    cpu_threads,
                    cpu_l3_cache_mb,
                    gpu_driver,
                    npu_driver,
                    disk_free,
                    disk_total,
                    disk_model,
                    ram_speed_mts,
                    ..
                } => {
                    self.api_active = true;
                    self.last_telemetry_at = Instant::now();
                    self.live_temp = cpu_temp;
                    self.live_gpu_temp = gpu_temp;
                    self.live_ram = ram_used;
                    self.live_ram_total = ram_total;
                    self.live_vram = vram_used;
                    self.live_vram_total = vram_total;
                    self.live_npu_vram = npu_vram_used;
                    self.live_tps = tps;
                    self.live_npu = npu_util;
                    self.live_uptime = node_uptime;
                    self.live_kv = kv_cache;
                    self.cpu_name = cpu_name;
                    self.gpu_name = gpu_name;
                    self.npu_name = npu_name;
                    self.cpu_cores = cpu_cores;
                    self.cpu_threads = cpu_threads;
                    self.cpu_l3 = cpu_l3_cache_mb;
                    self.gpu_driver = gpu_driver;
                    self.npu_driver = npu_driver;
                    self.live_disk_free = disk_free;
                    self.live_disk_total = disk_total;
                    self.disk_model = disk_model;
                    self.ram_speed = ram_speed_mts;
                }
                TelemetryEvent::DownloadStatus {
                    model,
                    status,
                    progress,
                } => {
                    self.dl_active = status != "Completed" && status != "Failed";
                    self.dl_model = model;
                    self.dl_status = status;
                    self.dl_progress = progress;
                }
                TelemetryEvent::ModelMounted { name, .. } => {
                    self.active_model = name;
                    self.api_active = true;
                }
                _ => {}
            }
        }

        if self.last_telemetry_at.elapsed() > Duration::from_secs(6) {
            self.api_active = false;
        }

        egui::SidePanel::left("nav")
            .exact_width(180.0)
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(10, 10, 12)).stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10))))
            .show(ctx, |ui| {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("HMIR").size(24.0).strong().color(egui::Color32::from_rgb(0, 242, 255)));
                    ui.label(egui::RichText::new("ELITE RUNTIME").size(9.0).strong().color(egui::Color32::from_gray(100)).extra_letter_spacing(2.0));
                });
                ui.add_space(40.0);

                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    for (tab, label, icon) in [
                        (Tab::Overview, " OVERVIEW", "🏠"),
                        (Tab::Chat, " CHAT", "💬"),
                        (Tab::Models, " MODELS", "📦"),
                        (Tab::Logs, " LOGS", "📜"),
                    ] {
                        let is_selected = self.current_tab == tab;
                        let text = egui::RichText::new(format!("{} {}", icon, label))
                            .size(13.0)
                            .strong();
                        
                        if ui.add(egui::SelectableLabel::new(is_selected, text)).clicked() {
                            self.current_tab = tab;
                        }
                        ui.add_space(8.0);
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(20.0);
                    if ui.button(if self.mini_mode { "EXPAND VIEW" } else { "COMPACT VIEW" }).clicked() {
                        self.mini_mode = !self.mini_mode;
                        let new_size = if self.mini_mode {
                            egui::vec2(980.0, 640.0)
                        } else {
                            egui::vec2(1240.0, 780.0)
                        };
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(new_size));
                    }
                    ui.label(egui::RichText::new("v1.0.0-ELITE").size(10.0).color(egui::Color32::from_gray(60)));
                });
            });

        egui::TopBottomPanel::top("header")
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading("HMIR Control Plane");
                    Self::draw_status_badge(ui, self.api_active);
                    ui.separator();
                    ui.label("API");
                    ui.code(&self.api_base_url);
                    ui.separator();
                    ui.label("Model");
                    ui.code(&self.active_model);
                });

                ui.horizontal_wrapped(|ui| {
                    let power_label = if self.api_active { "Stop API" } else { "Start API" };
                    if ui.button(power_label).clicked() {
                        self.api_active = !self.api_active;
                        let _ = self.cmd_sender.try_send(DashboardCmd::ToggleNode(self.api_active));
                    }
                    if ui.button("Restart API").clicked() {
                        let _ = self.cmd_sender.try_send(DashboardCmd::RestartNode);
                    }
                    if ui.button("Models Folder").clicked() {
                        let _ = self.cmd_sender.try_send(DashboardCmd::BrowseModels);
                    }
                    if ui.button("Logs Folder").clicked() {
                        let _ = self.cmd_sender.try_send(DashboardCmd::OpenDir("logs".to_string()));
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::Overview => self.render_overview(ui),
            Tab::Chat => self.render_chat(ui),
            Tab::Models => self.render_models(ui),
            Tab::Logs => self.render_logs(ui),
        });

        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

fn tail_lines(text: &str, max_lines: usize) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn api_base_url() -> String {
    std::env::var("HMIR_API_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

fn append_dashboard_error(message: &str) {
    let log_dir = DashboardApp::logs_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let path = log_dir.join("dashboard_error.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", message);
    }
}

fn executable_name(base: &str) -> String {
    format!("{}{}", base, std::env::consts::EXE_SUFFIX)
}

fn sibling_binary(base: &str) -> PathBuf {
    if let Ok(mut path) = std::env::current_exe() {
        path.pop();
        return path.join(executable_name(base));
    }

    PathBuf::from(executable_name(base))
}

fn open_path(path: &Path) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };

    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn stop_process(base: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        std::process::Command::new("taskkill")
            .args(["/F", "/IM", &executable_name(base), "/T"])
            .output()
            .map(|_| ())
            .map_err(|err| err.to_string())
    } else {
        std::process::Command::new("pkill")
            .args(["-f", base])
            .output()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}

fn start_api_process(api_base: &str) -> Result<(), String> {
    let port = api_base
        .rsplit(':')
        .next()
        .unwrap_or("8080")
        .trim_end_matches('/')
        .parse::<u16>()
        .unwrap_or(8080);
    let api_bin = sibling_binary("hmir-api");

    std::process::Command::new(api_bin)
        .env("HMIR_PORT", port.to_string())
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

async fn run_chat_request(
    client: &reqwest::Client,
    api_base: &str,
    prompt: &str,
) -> Result<String, String> {
    let response = client
        .post(format!("{}/v1/chat/completions", api_base))
        .json(&serde_json::json!({
            "messages": [{"role": "user", "content": prompt}],
            "stream": true
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut answer = String::new();

    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|err| err.to_string())?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(boundary) = buffer.find("\n\n") {
            let event = buffer[..boundary].to_string();
            buffer = buffer[boundary + 2..].to_string();

            for line in event.lines() {
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };

                let payload = data.trim();
                if payload == "[DONE]" {
                    return Ok(answer);
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                    if let Some(err) = json.get("error").and_then(|value| value.as_str()) {
                        return Err(err.to_string());
                    }

                    if let Some(content) = json
                        .get("choices")
                        .and_then(|value| value.get(0))
                        .and_then(|value| value.get("delta"))
                        .and_then(|value| value.get("content"))
                        .and_then(|value| value.as_str())
                    {
                        answer.push_str(content);
                    }
                }
            }
        }
    }

    if answer.is_empty() {
        Err("The local runtime returned no tokens.".to_string())
    } else {
        Ok(answer)
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1240.0, 780.0]),
        ..Default::default()
    };

    let api_base = api_base_url();
    let (tx, rx) = broadcast::channel(1024);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DashboardCmd>(32);
    let models_shared = Arc::new(Mutex::new(Vec::new()));
    let chat_history = Arc::new(Mutex::new(vec![ChatEntry {
        role: "assistant".to_string(),
        content: "HMIR is ready. Use this built-in chat or point any OpenAI-compatible client at the local API.".to_string(),
        is_error: false,
    }]));

    let models_for_bg = models_shared.clone();
    let chat_for_bg = chat_history.clone();
    let api_for_bg = Arc::new(api_base.clone());

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let client = reqwest::Client::new();
            let client_for_commands = client.clone();
            let client_for_telemetry = client.clone();
            let client_for_models = client.clone();
            let api_base_ref = api_for_bg.clone();
            let chat_history_ref = chat_for_bg.clone();
            let models_shared_ref = models_for_bg.clone();

            let api_for_commands = api_base_ref.clone();
            let api_for_telemetry = api_base_ref.clone();
            let api_for_models = api_base_ref.clone();
            let tx_for_telemetry = tx.clone();

            tokio::spawn(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        DashboardCmd::SwitchModel(name) => {
                            let _ = client_for_commands
                                .post(format!("{}/v1/engine/switch", *api_for_commands))
                                .json(&serde_json::json!({ "name": name }))
                                .send()
                                .await;
                        }
                        DashboardCmd::RestartNode => {
                            let _ = stop_process("hmir-api");
                            tokio::time::sleep(Duration::from_millis(800)).await;
                            if let Err(err) = start_api_process(&api_for_commands) {
                                append_dashboard_error(&format!("Failed to restart API: {}", err));
                            }
                        }
                        DashboardCmd::ToggleNode(active) => {
                            if active {
                                if let Err(err) = start_api_process(&api_for_commands) {
                                    append_dashboard_error(&format!("Failed to start API: {}", err));
                                }
                            } else if let Err(err) = stop_process("hmir-api") {
                                append_dashboard_error(&format!("Failed to stop API: {}", err));
                            }
                        }
                        DashboardCmd::OpenDir(target) => {
                            let path = if target == "logs" {
                                DashboardApp::logs_dir()
                            } else {
                                DashboardApp::data_root().join(target)
                            };
                            if let Err(err) = open_path(&path) {
                                append_dashboard_error(&format!("Failed to open {}: {}", path.display(), err));
                            }
                        }
                        DashboardCmd::BrowseModels => {
                            if let Err(err) = open_path(&DashboardApp::models_dir()) {
                                append_dashboard_error(&format!("Failed to open models folder: {}", err));
                            }
                        }
                        DashboardCmd::DownloadModel {
                            repo_id,
                            folder_name,
                        } => {
                            let _ = client_for_commands
                                .post(format!("{}/v1/models/download", *api_for_commands))
                                .json(&serde_json::json!({
                                    "repo_id": repo_id,
                                    "folder_name": folder_name
                                }))
                                .send()
                                .await;
                        }
                        DashboardCmd::DismountModel => {
                            let _ = client_for_commands
                                .post(format!("{}/v1/engine/eject", *api_for_commands))
                                .send()
                                .await;
                        }
                        DashboardCmd::SendChat(prompt) => {
                            {
                                let mut history = chat_history_ref.lock().unwrap();
                                history.push(ChatEntry {
                                    role: "user".to_string(),
                                    content: prompt.clone(),
                                    is_error: false,
                                });
                                history.push(ChatEntry {
                                    role: "assistant".to_string(),
                                    content: "Thinking...".to_string(),
                                    is_error: false,
                                });
                            }

                            let result = run_chat_request(&client_for_commands, &api_for_commands, &prompt).await;
                            let mut history = chat_history_ref.lock().unwrap();
                            if let Some(last) = history.last_mut() {
                                match result {
                                    Ok(answer) => {
                                        last.content = answer;
                                        last.is_error = false;
                                    }
                                    Err(err) => {
                                        last.content = format!("Request failed: {}", err);
                                        last.is_error = true;
                                    }
                                }
                            }
                        }
                        DashboardCmd::ClearChat => {
                            let mut history = chat_history_ref.lock().unwrap();
                            history.clear();
                            history.push(ChatEntry {
                                role: "assistant".to_string(),
                                content: "Chat cleared. HMIR is ready for the next local request.".to_string(),
                                is_error: false,
                            });
                        }
                    }
                }
            });

            tokio::spawn(async move {
                loop {
                    match client_for_telemetry
                        .get(format!("{}/v1/telemetry", *api_for_telemetry))
                        .send()
                        .await
                    {
                        Ok(response) => {
                            let mut stream = response.bytes_stream();
                            while let Some(item) = stream.next().await {
                                match item {
                                    Ok(bytes) => {
                                        let chunk = String::from_utf8_lossy(&bytes);
                                        for line in chunk.lines() {
                                            if let Some(payload) = line.strip_prefix("data:") {
                                                if let Ok(event) =
                                                    serde_json::from_str::<TelemetryEvent>(payload.trim())
                                                {
                                                    let _ = tx_for_telemetry.send(event);
                                                }
                                            }
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(_) => {}
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            });

            loop {
                match client_for_models
                    .get(format!("{}/v1/models/installed", *api_for_models))
                    .send()
                    .await
                {
                    Ok(response) => if let Ok(models) = response.json::<Vec<String>>().await {
                        let mut guard = models_shared_ref.lock().unwrap();
                        *guard = models;
                    },
                    Err(_) => {}
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    });

    eframe::run_native(
        "HMIR",
        options,
        Box::new(|cc| {
            Box::new(DashboardApp::new(
                cc,
                rx,
                cmd_tx,
                models_shared,
                chat_history,
                api_base,
            ))
        }),
    )
}
