use std::collections::BinaryHeap;
use std::cmp::Ordering;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SequencePriority {
    Foreground { max_ttft_ms: u64 },
    Background { max_itl_ms: u64 },
    Agent { max_latency_budget_ms: u64 },
}

#[derive(Eq, PartialEq)]
pub struct InternalRequest {
    pub logic_id: u64,
    pub priority: SequencePriority,
    pub prompt: Vec<u32>,
}

impl Ord for InternalRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.priority, &other.priority) {
            (SequencePriority::Foreground{..}, SequencePriority::Background{..}) => Ordering::Greater,
            (SequencePriority::Background{..}, SequencePriority::Foreground{..}) => Ordering::Less,
            _ => Ordering::Equal, 
        }
    }
}

impl PartialOrd for InternalRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct PriorityBoundedQueue {
    queue: BinaryHeap<InternalRequest>,
}

impl PriorityBoundedQueue {
    pub fn new() -> Self {
        Self { queue: BinaryHeap::new() }
    }

    pub fn enqueue(&mut self, req: super::continuous_batcher::SequenceRequest) -> u64 {
        let logic_id = fastrand::u64(..);
        self.queue.push(InternalRequest { logic_id, priority: req.priority, prompt: req.prompt });
        logic_id
    }
    
    pub fn dequeue_next_admissible(&mut self) -> Option<InternalRequest> {
        self.queue.pop()
    }
}
