use tokio::sync::mpsc;
// Mocking SequenceStatus since we are bounding
#[derive(Debug, Clone, PartialEq)]
pub enum SequenceStatus {
    Waiting,
    Running,
}

pub type SequenceId = u64;

#[derive(Debug, Clone)]
pub enum ControlCommand {
    Pause(SequenceId),
    Resume(SequenceId),
    Kill(SequenceId),
    SetStrategy { strategy: String },
    HotSwapModel { new_path: String },
    ForceFallback { to_device: String },
}

pub struct TaskState {
    pub status: SequenceStatus,
    pub active_model: String,
    pub generation_latency: Vec<f64>,
}

pub struct TaskRegistry {
    pub control_tx: mpsc::Sender<ControlCommand>,
}

impl TaskRegistry {
    pub fn dispatch_command(&self, command: ControlCommand) {
        let _ = self.control_tx.try_send(command);
    }
}
