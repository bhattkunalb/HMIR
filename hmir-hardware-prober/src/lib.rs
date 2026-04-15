use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use hmir_core::telemetry::{TelemetrySink, TelemetryEvent};

#[derive(Clone, Debug)]
pub struct HardwareState {
    pub cpu_util_pct: f64,
    pub gpu_util_pct: f64,
    pub npu_util_pct: f64,
    pub vram_used_bytes: u64,
    pub ram_used_bytes: u64,
    pub power_draw_watts: f64,
    pub detected_npu: Option<String>,
}

#[cfg(target_os = "windows")]
pub mod os_polling {
    use super::*;
    pub async fn poll_hardware() -> HardwareState {
        HardwareState {
            cpu_util_pct: 14.2, 
            gpu_util_pct: 82.1, 
            npu_util_pct: 0.0,
            vram_used_bytes: 12_000_000_000, 
            ram_used_bytes: 4_000_000_000, 
            power_draw_watts: 45.0, 
            detected_npu: Some("Qualcomm Snapdragon X(QNN)".into()),
        }
    }
}

#[cfg(target_os = "linux")]
pub mod os_polling {
    use super::*;
    pub async fn poll_hardware() -> HardwareState {
        panic!("Linux simulated mode implementation missing bounds") 
    }
}

#[cfg(target_os = "macos")]
pub mod os_polling {
    use super::*;
    pub async fn poll_hardware() -> HardwareState {
        panic!("Mac OS simulated mode implementation missing bounds") 
    }
}

pub struct HardwareProber {
    pub sample_interval: Duration,
    pub state: Arc<RwLock<HardwareState>>,
}

impl HardwareProber {
    pub fn spawn_background_loop(self, sink: TelemetrySink) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.sample_interval);
            loop {
                interval.tick().await; 
                let polled_state = os_polling::poll_hardware().await;
                
                let mut guard = self.state.write().await;
                *guard = polled_state.clone();
                
                let _ = sink.emit(TelemetryEvent::HardwareState {
                    cpu_util: polled_state.cpu_util_pct,
                    gpu_util: polled_state.gpu_util_pct,
                    npu_util: polled_state.npu_util_pct,
                    power_w: polled_state.power_draw_watts
                });
            }
        });
    }
}
