use dashmap::DashMap;
use crate::telemetry::task_registry::SequenceId;
use crate::telemetry::TelemetrySink;
use crate::topology::mapper::HardwareTopologyMapper;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BatchError {
    #[error("VRAM Constraints Exhausted")]
    ResourceExhaustion,
}

pub struct SequenceState {
    pub id: SequenceId,
    pub active_tokens: Vec<u32>,
    pub status: String,
}

pub struct SequenceRequest {
    pub priority: super::priority_queue::SequencePriority,
    pub prompt: Vec<u32>,
}

pub struct BatchPlan {
    pub active_sequences: Vec<SequenceId>,
    pub mapped_indices: Vec<usize>,
}

pub struct BatchOutput {
    pub chunked_tokens: Vec<u32>,
}

pub struct ContinuousBatcher {
    active_sequences: DashMap<SequenceId, SequenceState>,
    pending_queue: super::priority_queue::PriorityBoundedQueue,
    topology: Arc<HardwareTopologyMapper>,
    telemetry: Arc<TelemetrySink>,
}

impl ContinuousBatcher {
    pub fn new(topology: Arc<HardwareTopologyMapper>, telemetry: Arc<TelemetrySink>) -> Self {
        Self {
            active_sequences: DashMap::new(),
            pending_queue: super::priority_queue::PriorityBoundedQueue::new(),
            topology,
            telemetry,
        }
    }

    pub async fn submit(&mut self, req: SequenceRequest) -> Result<SequenceId, BatchError> {
        let sid = self.pending_queue.enqueue(req);
        Ok(sid)
    }

    pub fn assemble_next_batch(&mut self, max_batch_tokens: usize) -> BatchPlan {
        let mut plan = BatchPlan { active_sequences: Vec::new(), mapped_indices: Vec::new() };
        let mut batch_size = 0;

        while let Some(req) = self.pending_queue.dequeue_next_admissible() {
            if batch_size + req.prompt.len() > max_batch_tokens {
                break;
            }
            batch_size += req.prompt.len();
            plan.active_sequences.push(req.logic_id);
            plan.mapped_indices.push(batch_size);
            
            self.active_sequences.insert(req.logic_id, SequenceState {
                id: req.logic_id,
                active_tokens: req.prompt,
                status: "batched".into(),
            });
        }
        
        plan
    }

    pub fn apply_batch_output(&mut self, _output: BatchOutput) {
        // Output mapped back
    }
}
