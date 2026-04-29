//! AMD ROCm / HIP FFI Boundaries
//!
//! Direct interaction with the HIP runtime for AMD Instinct and Radeon accelerators.

#![allow(dead_code, unused_variables)]

use core::ffi::{c_int, c_void};

pub type HipError = c_int;

extern "C" {
    /// Launches a HIP kernel on the specified AMD device
    pub fn hipModuleLaunchKernel(
        f: *mut c_void,
        gridDimX: u32,
        gridDimY: u32,
        gridDimZ: u32,
        blockDimX: u32,
        blockDimY: u32,
        blockDimZ: u32,
        sharedMemBytes: u32,
        stream: *mut c_void,
        kernelParams: *mut *mut c_void,
        extra: *mut *mut c_void,
    ) -> HipError;
}
