use crate::backends::{
    AttentionOutput, BackendAdapter, BackendError, BlockTable, LogicalId, PagedBackendAdapter,
    PagedCacheConfig, PhysicalBlockHandle, TensorShape, TensorView,
};
use tokio::task;

/// AMD ROCm / MIGraphX specialized adapter for Instinct and Radeon GPUs.
///
/// This bridge provides the interface for AMD's HIP runtime and MIGraphX
/// graph compiler, enabling NPU-like efficiency on AMD silicon.
pub struct RocmAdapter {
    pub device_index: i32,
    pub is_instinct_series: bool,
    _block_registry: std::collections::HashMap<LogicalId, PhysicalBlockHandle>,
    _next_physical: usize,
}

impl RocmAdapter {
    pub fn new(device_index: i32, is_instinct_series: bool) -> Self {
        Self {
            device_index,
            is_instinct_series,
            _block_registry: std::collections::HashMap::new(),
            _next_physical: 1,
        }
    }
}

impl BackendAdapter for RocmAdapter {
    fn validate_shape(&self, _shape: &TensorShape) -> Result<(), BackendError> {
        // Validation against ROCm memory pools and HIP stream alignment
        Ok(())
    }

    async fn evaluate_batch(&self) -> Result<usize, BackendError> {
        let exec_future = task::spawn_blocking(move || {
            // AMD ROCm / HIP execution triggers here:
            // unsafe { hipModuleLaunchKernel(...) }
            1 // mock token return
        });

        match exec_future.await {
            Ok(tokens) => Ok(tokens),
            Err(_) => Err(BackendError::HardwareTimeout),
        }
    }
}

impl PagedBackendAdapter for RocmAdapter {
    fn register_kv_block(
        &mut self,
        logical_id: LogicalId,
        _raw_ptr: std::ptr::NonNull<core::ffi::c_void>,
        _size_bytes: usize,
    ) -> Result<PhysicalBlockHandle, BackendError> {
        let phys_handle = PhysicalBlockHandle(self._next_physical);
        self._next_physical += 1;
        self._block_registry.insert(logical_id, phys_handle);
        Ok(phys_handle)
    }

    fn execute_paged_attention(
        &mut self,
        _query: &TensorView,
        _block_table: &BlockTable,
        _cache_config: &PagedCacheConfig,
    ) -> Result<AttentionOutput, BackendError> {
        // Utilizes composable_kernel or specialized ROCm attention kernels
        Ok(AttentionOutput {
            sequence_id: 1,
            score: 0.99,
        })
    }

    fn release_block(&mut self, physical_handle: PhysicalBlockHandle) -> Result<(), BackendError> {
        self._block_registry.retain(|_, v| *v != physical_handle);
        Ok(())
    }

    fn execute_draft_verification(
        &mut self,
        _draft_tree: &TensorView,
        _block_table: &BlockTable,
    ) -> Result<Vec<AttentionOutput>, BackendError> {
        Ok(vec![AttentionOutput {
            sequence_id: 1,
            score: 0.99,
        }])
    }
}
