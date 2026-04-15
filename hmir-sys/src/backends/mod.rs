#![allow(async_fn_in_trait)]

pub mod error;
pub mod llama_adapter;
pub mod onnx_adapter;
pub mod paged;

pub use error::BackendError;
pub use paged::*;

/// Defines a standard memory boundary constraint shape.
pub struct TensorShape {
    pub dim_x: usize,
    pub dim_y: usize,
    pub dim_z: usize,
    pub byte_size: usize,
}

/// Core interface bounding the safe Rust execution context from the Unsafe C-FFI engines.
pub trait BackendAdapter {
    /// Safe pre-flight check guaranteeing memory boundaries won't trigger a C-level Segfault
    fn validate_shape(&self, shape: &TensorShape) -> Result<(), BackendError>;

    /// Executes the generation loop. Because underlying C functions block the active OS thread entirely,
    /// implementing engines MUST push this execution to a `tokio::task::spawn_blocking` frame.
    async fn evaluate_batch(&self) -> Result<usize, BackendError>;
}
