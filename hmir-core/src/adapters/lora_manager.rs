use dashmap::{DashMap, DashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use thiserror::Error;
use crate::telemetry::task_registry::SequenceId;
use crate::telemetry::TelemetrySink;

#[derive(Error, Debug)]
pub enum AdapterError {
    #[error("VRAM Constraints Exhausted")]
    VRAMExhausted,
    #[error("Disk IO Failure")]
    IOFailure,
}

pub struct AdapterState {
    pub scale: f32,
    pub ref_count: AtomicUsize,
    pub active_sequences: DashSet<SequenceId>,
}

pub struct LoRAAdapterManager {
    pub loaded_adapters: DashMap<String, AdapterState>,
    telemetry: Arc<TelemetrySink>,
}

impl LoRAAdapterManager {
    pub fn new(telemetry: Arc<TelemetrySink>) -> Self {
        Self {
            loaded_adapters: DashMap::new(),
            telemetry,
        }
    }

    pub async fn load(&self, _path: &str, scale: f32) -> Result<String, AdapterError> {
        let adapter_id = "lora_agent_ext".to_string();
        
        self.loaded_adapters.insert(adapter_id.clone(), AdapterState {
            scale,
            ref_count: AtomicUsize::new(0),
            active_sequences: DashSet::new(),
        });

        Ok(adapter_id)
    }

    pub async fn attach(&self, adapter_id: &str, seq_id: SequenceId) -> Result<(), AdapterError> {
        if let Some(adapter) = self.loaded_adapters.get(adapter_id) {
            adapter.ref_count.fetch_add(1, Ordering::SeqCst);
            adapter.active_sequences.insert(seq_id);
            Ok(())
        } else {
            Err(AdapterError::IOFailure)
        }
    }

    pub async fn unload(&self, adapter_id: &str) -> Result<(), AdapterError> {
        self.loaded_adapters.remove(adapter_id);
        Ok(())
    }
}
