//! Unsafe C-FFI Interface Boundaries
//!
//! This crate contains raw representations of native objects matching their C++ representations.

pub mod backends;
pub mod ffi_cuda;
pub mod ffi_llama;
pub mod ffi_mlx;
pub mod ffi_onnx;
pub mod ffi_rocm;

// Standard testing for verifying ABI memory alignments.
#[cfg(test)]
mod tests {
    use crate::ffi_llama::LlamaContextParams;

    #[test]
    fn test_ffi_abi_size_assumptions() {
        // Ensuring 64-bit bounds alignment matches standard C struct expectations.
        // This prevents segmentation faults when tossing pointers to C++.
        assert_eq!(
            std::mem::size_of::<LlamaContextParams>(),
            64, // Expected structure byte length for safety
            "C-ABI struct LlamaContextParams size does not match layout!"
        );
    }
}
