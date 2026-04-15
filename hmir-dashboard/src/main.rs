use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use hmir_core::telemetry::{TelemetryEvent};
use tokio::sync::broadcast;

pub struct DashboardApp {
    telemetry_receiver: broadcast::Receiver<TelemetryEvent>,
    live_gpu_history: Vec<f64>,
    live_cpu_history: Vec<f64>,
}

impl DashboardApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, rx: broadcast::Receiver<TelemetryEvent>) -> Self {
        Self {
            telemetry_receiver: rx,
            live_gpu_history: vec![0.0],
            live_cpu_history: vec![0.0],
        }
    }
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top Panel: Control Limits
        egui::TopBottomPanel::top("hardware_status").show(ctx, |ui| {
            ui.heading("HMIR Elite Node Dashboard");
            ui.horizontal(|ui| {
                ui.label("GPU VRAM Constraint: 48%");
                ui.label("NPU SRAM Yield: Active(Speculative)");
            });
        });

        // Left Panel: Commands
        egui::SidePanel::left("control_panel").show(ctx, |ui| {
            ui.heading("Orchestration Commands");
            if ui.button("⏹ Force Fallback (CPU)").clicked() {
                // Send force-trigger IPC bounds
            }
            if ui.button("🔥 Hot-Swap Target Model").clicked() { /* ... */ }
        });

        // Center Panel: High-Framerate Data Tracking Matrix
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Ok(event) = self.telemetry_receiver.try_recv() {
                if let TelemetryEvent::HardwareState { gpu_util, cpu_util, .. } = event {
                    self.live_gpu_history.push(gpu_util);
                    self.live_cpu_history.push(cpu_util);
                }
            }
            
            ui.heading("Live VRAM Node Distribution Layout");
            let points: PlotPoints = self.live_gpu_history.iter()
                .enumerate()
                .map(|(i, &y)| [i as f64, y])
                .collect();
                
            Plot::new("hardware_plot")
                .view_aspect(2.0)
                .show(ui, |plot_ui| plot_ui.line(Line::new(points).name("GPU Usage")));
        });

        ctx.request_repaint(); // 60 FPS update cycle maintaining stream metrics visibly
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions::default();
    
    // Simulate mapping to the global core telemetry channels
    let (tx, rx) = broadcast::channel(1024);
    
    // Mock simulation pushing real dummy tracking vectors async out of path limits
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = tx.send(TelemetryEvent::HardwareState {
                cpu_util: 12.0, gpu_util: 85.5, npu_util: 0.0, power_w: 120.0,
            });
        }
    });

    eframe::run_native(
        "HMIR Telemetry Limits",
        options,
        Box::new(|cc| Box::new(DashboardApp::new(cc, rx))),
    )
}
