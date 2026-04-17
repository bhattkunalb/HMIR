pub mod task_registry;

use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TelemetryError {
    #[error("Broadcast channel congested dropping metrics natively")]
    BroadcastOverflow,
}

use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TelemetryEvent {
    SequenceStart { id: u64, model: String, strategy: String },
    TokenGenerated { id: u64, token: u32, device: String, itl_ms: f64 },
    SpeculativeBatch { accepted: usize, rejected: usize, draft_device: String },
    MemoryPressure { vram_used: usize, ram_used: usize, swap_rate: f64 },
    HardwareState { 
        cpu_util: f64, 
        gpu_util: f64, 
        npu_util: f64, 
        cpu_temp: f64, 
        gpu_temp: f64,
        vram_used: f64, 
        vram_total: f64,
        ram_used: f64, 
        ram_total: f64,
        tps: f64,
        power_w: f64,
        node_uptime: u64,
        kv_cache: f32,
        cpu_name: String,
        gpu_name: String,
        npu_name: String,
        disk_free: f64,
    },
    DownloadStatus { 
        model: String, 
        status: String, 
        progress: f32,
    },
    ModelMounted { name: String, engine: String },
}

pub struct TelemetrySink {
    tx: broadcast::Sender<TelemetryEvent>,
    tokens_emitted: AtomicUsize, 
}

impl TelemetrySink {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            tokens_emitted: AtomicUsize::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TelemetryEvent> {
        self.tx.subscribe()
    }

    #[inline(always)]
    pub fn emit(&self, event: TelemetryEvent) -> Result<(), TelemetryError> {
        if let TelemetryEvent::TokenGenerated { .. } = event {
            self.tokens_emitted.fetch_add(1, Ordering::Relaxed);
        }
        
        let _ = self.tx.send(event); // Swallow receiver-less errors 
        Ok(())
    }
}
