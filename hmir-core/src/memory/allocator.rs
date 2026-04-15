use std::collections::HashMap;

/// Unique identifier for a memory block in the logical address space
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub usize);

/// Identifies where a block of memory is physically residing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStatus {
    /// In GPU VRAM, hot and ready for multiplication
    ResidentVram,
    /// In NPU High-Speed Buffer
    ResidentNpu,
    /// Swapped out to System RAM (CPU bounding)
    SwappedRam,
    /// Unallocated
    Free,
}

/// A zero-copy representation of a tensor (metadata, ptr placeholder) mapped from disk
#[derive(Debug, Clone)]
pub struct MmapTensor {
    pub id: String,
    pub size_bytes: usize,
    /// Memory mapped base pointer (placeholder)
    pub mapped_ptr: usize, 
}

/// The Logical Page Table maps abstract KV Cache blocks across heterogeneous physical memories.
/// It tracks LRU (Least Recently Used) counters for graceful eviction from VRAM strictly into RAM, 
/// avoiding OS level paging which causes extreme latencies.
#[derive(Debug)]
pub struct LogicalPageTable {
    pub max_vram_blocks: usize,
    pub max_ram_blocks: usize,
    // BlockId mapping to current physical location
    mappings: HashMap<BlockId, PageStatus>,
    // A crude LRU tracker for demonstration
    lru_counter: HashMap<BlockId, u64>,
    clock: u64,
}

impl LogicalPageTable {
    pub fn new(max_vram_blocks: usize, max_ram_blocks: usize) -> Self {
        Self {
            max_vram_blocks,
            max_ram_blocks,
            mappings: HashMap::new(),
            lru_counter: HashMap::new(),
            clock: 0,
        }
    }

    /// Attempt to allocate a block in VRAM. If VRAM is full, evict the coldest block to RAM.
    pub fn allocate_vram_block(&mut self, block: BlockId) -> Result<(), &'static str> {
        self.clock += 1;
        let vram_count = self.mappings.values().filter(|&s| *s == PageStatus::ResidentVram).count();

        if vram_count >= self.max_vram_blocks {
            // Trigger Eviction Swap Protocol
            self.evict_coldest_to_ram()?;
        }

        self.mappings.insert(block, PageStatus::ResidentVram);
        self.lru_counter.insert(block, self.clock);
        
        Ok(())
    }

    /// Touches a block, marking it recently used (updates LRU)
    pub fn touch(&mut self, block: BlockId) {
        self.clock += 1;
        self.lru_counter.insert(block, self.clock);
    }

    /// Evicts the Least Recently Used block from VRAM into System RAM
    fn evict_coldest_to_ram(&mut self) -> Result<(), &'static str> {
        let mut coldest = None;
        let mut min_clock = u64::MAX;

        for (id, status) in self.mappings.iter() {
            if *status == PageStatus::ResidentVram {
                if let Some(&tick) = self.lru_counter.get(id) {
                    if tick < min_clock {
                        min_clock = tick;
                        coldest = Some(*id);
                    }
                }
            }
        }

        if let Some(cold_block) = coldest {
            let ram_count = self.mappings.values().filter(|&s| *s == PageStatus::SwappedRam).count();
            if ram_count >= self.max_ram_blocks {
                return Err("Out of Memory (OOM): Both VRAM and System RAM are exhausted.");
            }

            // Pseudo DMA Transfer execution path would go here.
            // Move abstract pointer location to SwappedRam.
            self.mappings.insert(cold_block, PageStatus::SwappedRam);
            Ok(())
        } else {
            Err("Failed to find a valid block to evict.")
        }
    }

    pub fn get_status(&self, block: BlockId) -> Option<&PageStatus> {
        self.mappings.get(&block)
    }
}

// -----------------------------------------------------------------------------
// TESTS
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vram_allocation_and_eviction() {
        // Assume 2 blocks of VRAM and 2 blocks of RAM
        let mut pager = LogicalPageTable::new(2, 2);

        let block1 = BlockId(1);
        let block2 = BlockId(2);
        let block3 = BlockId(3);

        // Fill VRAM
        assert!(pager.allocate_vram_block(block1).is_ok());
        assert!(pager.allocate_vram_block(block2).is_ok());
        
        assert_eq!(pager.get_status(block1), Some(&PageStatus::ResidentVram));
        assert_eq!(pager.get_status(block2), Some(&PageStatus::ResidentVram));

        // Touch block 1 so block 2 becomes the coldest (LRU)
        pager.touch(block1);

        // Allocating block 3 should evict block 2 to RAM
        assert!(pager.allocate_vram_block(block3).is_ok());

        assert_eq!(pager.get_status(block1), Some(&PageStatus::ResidentVram));
        assert_eq!(pager.get_status(block2), Some(&PageStatus::SwappedRam)); // Successfully evicted
        assert_eq!(pager.get_status(block3), Some(&PageStatus::ResidentVram));
    }

    #[test]
    fn test_total_oom_guardrail() {
        // 1 VRAM block, 1 RAM block limit
        let mut pager = LogicalPageTable::new(1, 1);
        
        assert!(pager.allocate_vram_block(BlockId(1)).is_ok());
        assert!(pager.allocate_vram_block(BlockId(2)).is_ok()); // Pushes 1 to RAM
        
        // Block 1 is in RAM, Block 2 is in VRAM. Both are full. 
        // Adding Block 3 should trigger a hard OOM fallback error matching guardrails.
        let result = pager.allocate_vram_block(BlockId(3));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Out of Memory (OOM): Both VRAM and System RAM are exhausted.");
    }
}
