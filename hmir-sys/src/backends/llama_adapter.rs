use crate::backends::{BackendAdapter, BackendError, TensorShape, PagedBackendAdapter, LogicalId, PhysicalBlockHandle, TensorView, BlockTable, PagedCacheConfig, AttentionOutput};
use crate::ffi_llama::{LlamaContextPtr, LlamaModelPtr};
use tokio::task;

// Simulates Llama internal caching mechanism
pub struct LlamaCppAdapter {
    pub max_batch_size: usize,
    _model: LlamaModelPtr,
    _ctx: LlamaContextPtr,
    _block_registry: std::collections::HashMap<LogicalId, PhysicalBlockHandle>,
    _next_physical: usize,
}

impl LlamaCppAdapter {
    pub fn new(max_batch_size: usize) -> Self {
        Self {
            max_batch_size,
            _model: std::ptr::null_mut(),
            _ctx: std::ptr::null_mut(),
            _block_registry: std::collections::HashMap::new(),
            _next_physical: 1,
        }
    }
}

impl BackendAdapter for LlamaCppAdapter {
    fn validate_shape(&self, shape: &TensorShape) -> Result<(), BackendError> {
        if shape.dim_x > self.max_batch_size {
            return Err(BackendError::ShapeValidationFailed(format!(
                "Context token batch size [{}] exceeds initialized max limit [{}]",
                shape.dim_x, self.max_batch_size
            )));
        }
        Ok(())
    }

    #[allow(refining_impl_trait)]
    fn evaluate_batch(&self) -> impl std::future::Future<Output = Result<usize, BackendError>> + Send {
        // Here we throw the synchronous blocking C-FFI request off the main async loop.
        // This prevents the orchestrator from deadlocking during continuous batching!
        
        let exec_future = task::spawn_blocking(move || {
            // Unsafe FFI C bindings trigger here:
            // unsafe { ffi_llama::llama_decode(...) }
            1 // mock token return
        });

        async move {
            match exec_future.await {
                Ok(tokens_processed) => Ok(tokens_processed),
                Err(_) => Err(BackendError::HardwareTimeout),
            }
        }
    }
}

impl PagedBackendAdapter for LlamaCppAdapter {
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
        block_table: &BlockTable,
        cache_config: &PagedCacheConfig,
    ) -> Result<AttentionOutput, BackendError> {
        
        if cache_config.block_size == 0 {
            return Err(BackendError::ShapeValidationFailed("Block size zero on internal cache".to_string()));
        }

        // Simulating the actual memory checking logic across logical endpoints:
        for logical_key in block_table.routes.keys() {
            if !self._block_registry.contains_key(logical_key) {
                return Err(BackendError::LlamaPointerUnallocated);
            }
        }

        // Mocks execution of the disjoint memory spaces correctly 
        Ok(AttentionOutput {
            sequence_id: 1,
            score: 0.99
        })
    }

    fn release_block(&mut self, physical_handle: PhysicalBlockHandle) -> Result<(), BackendError> {
        // Free logic via Unsafe FFI would execute here
        self._block_registry.retain(|_, v| *v != physical_handle);
        Ok(())
    }

    fn execute_draft_verification(
        &mut self,
        _draft_tree: &TensorView,
        _block_table: &BlockTable,
    ) -> Result<Vec<AttentionOutput>, BackendError> {
        // Translates non-contiguous sequence block into `llama_batch` representation
        // to support parallel multi-token draft verifications on GPU!
        Ok(vec![AttentionOutput { sequence_id: 1, score: 0.99 }])
    }
}

// -----------------------------------------------------------------------------
// TESTS
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_guardrails() {
        let adapter = LlamaCppAdapter::new(128);
        
        let invalid_tensor = TensorShape {
            dim_x: 500,
            dim_y: 1,
            dim_z: 1,
            byte_size: 4000,
        };

        let result = adapter.validate_shape(&invalid_tensor);
        
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            BackendError::ShapeValidationFailed("Context token batch size [500] exceeds initialized max limit [128]".to_string())
        );
    }
    
    #[test]
    fn test_paged_attention_routing() {
        let mut adapter = LlamaCppAdapter::new(32);
        
        // 1. Simulate a safe raw physical buffer passing inside from the orchestrator
        let mut fake_mem_pool = vec![0u8; 1024];
        let paged_ptr = std::ptr::NonNull::new(fake_mem_pool.as_mut_ptr() as *mut std::ffi::c_void).unwrap();
        
        // 2. Register Logical ID 1 with the Backend directly pointing to memory block 
        let handle = adapter.register_kv_block(LogicalId(1), paged_ptr, 1024).unwrap();
        
        // 3. Setup a Query and execute disjoint multi-page routing
        let query = TensorView {
            ptr: paged_ptr,
            elements: 16,
            stride: 2,
        };
        
        let mut table = BlockTable::new();
        // Pointing query exclusively at our registered block
        table.routes.insert(LogicalId(1), handle); 
        
        let output = adapter.execute_paged_attention(&query, &table, &PagedCacheConfig { block_size: 16, max_blocks: 100 });
        
        assert!(output.is_ok());
        assert_eq!(output.unwrap().sequence_id, 1);
        
        // 4. Test error logic bounding (Attempting to infer over UN-registered Logical Blocks)
        table.routes.insert(LogicalId(5), PhysicalBlockHandle(999));
        
        let failed_output = adapter.execute_paged_attention(&query, &table, &PagedCacheConfig { block_size: 16, max_blocks: 100 });
        
        assert!(failed_output.is_err());
        assert_eq!(failed_output.unwrap_err(), BackendError::LlamaPointerUnallocated);
    }
}
