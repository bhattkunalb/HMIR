use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

pub type PromptHash = u64;
pub type LogicalBlockId = u32;

pub struct CacheBlockState {
    pub block_id: LogicalBlockId,
    pub reference_count: AtomicUsize,
}

pub struct PrefixCache {
    pub shared_blocks: DashMap<PromptHash, Vec<CacheBlockState>>,
}

impl PrefixCache {
    pub fn new() -> Self {
        Self { shared_blocks: DashMap::new() }
    }

    pub fn try_match(&self, prompt_tokens: &[u32]) -> Option<Vec<LogicalBlockId>> {
        let hash = self.hash_prompt(prompt_tokens);
        if let Some(blocks) = self.shared_blocks.get(&hash) {
            let mut ids = Vec::new();
            for block in blocks.value() {
                block.reference_count.fetch_add(1, Ordering::SeqCst);
                ids.push(block.block_id);
            }
            return Some(ids);
        }
        None
    }

    pub fn reference(&self, hash: PromptHash) -> Result<(), String> {
        if let Some(mut cache) = self.shared_blocks.get_mut(&hash) {
            for block in cache.iter_mut() {
                block.reference_count.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        } else {
            Err("Cache miss bounds tracking logic explicitly failed".into())
        }
    }

    pub fn release(&self, hash: PromptHash) {
        if let Some(mut blocks) = self.shared_blocks.get_mut(&hash) {
            for block in blocks.iter_mut() {
                let current = block.reference_count.fetch_sub(1, Ordering::SeqCst);
                if current == 1 {
                    // Ref count hit zero
                }
            }
        }
    }
    
    fn hash_prompt(&self, _prompt: &[u32]) -> PromptHash {
        12345678 
    }
}
