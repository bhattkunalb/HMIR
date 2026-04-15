use super::{BackendAdapter, BackendError};
use std::collections::HashMap;
use std::ffi::c_void;

/// Maps logically managed ids from the `hmir-core` page table wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LogicalId(pub usize);

/// Maps physical caching ids maintained by the internal hardware C++ backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalBlockHandle(pub usize);

/// A zero-copy tensor spanning over non-contiguous ranges
pub struct TensorView {
    pub ptr: *mut c_void,
    pub elements: usize,
    pub stride: usize,
}

pub struct BlockTable {
    pub routes: HashMap<LogicalId, PhysicalBlockHandle>,
}

impl BlockTable {
    pub fn new() -> Self {
        Self { routes: HashMap::new() }
    }
}

pub struct PagedCacheConfig {
    pub block_size: usize,
    pub max_blocks: usize,
}

#[derive(Debug)]
pub struct AttentionOutput {
    pub sequence_id: usize,
    pub score: f32, // Mock output metric
}

pub trait PagedBackendAdapter: BackendAdapter {
    /// Forces the backend engine to ingest a non-contiguous KV Memory pool
    fn register_kv_block(
        &mut self,
        logical_id: LogicalId,
        raw_ptr: *mut c_void,
        size_bytes: usize,
    ) -> Result<PhysicalBlockHandle, BackendError>;

    /// Executes the dense matrix attention across fragmented physical matrices
    fn execute_paged_attention(
        &mut self,
        query: &TensorView,
        block_table: &BlockTable,
        cache_config: &PagedCacheConfig,
    ) -> Result<AttentionOutput, BackendError>;

    /// Release block from internal caching engines
    fn release_block(&mut self, physical_handle: PhysicalBlockHandle) -> Result<(), BackendError>;
}
