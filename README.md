# HMIR (Heterogeneous Memory-First Inference Runtime)
*Principal ML Systems Architecture for Local LLM Execution*

HMIR is a next-generation local inference engine treating NPU, GPU, and CPU execution as a memory-bandwidth-bound optimization problem rather than a compute-bound routing problem.

## Core Philosophy
1. **Explicit Cost Modeling:** We map RAM/VRAM/PCIe topologies and only dispatch compute when $T_{transfer} < T_{compute\_saved}$.
2. **Unified Paged Memory:** VRAM and RAM are treated as a unified virtual address space using `PagedAttention` methodologies.
3. **Intent-Driven:** Hardware selection is guided by intents `["latency", "throughput", "battery"]` instead of manual `device="cuda:0"` flags.
4. **Heterogeneous Speculative Decoding:** Drafts fast on the NPU, verifies on the GPU.

## Phase 1 Implementation Details
Currently implemented in `hmir-core`:
* **Memory Management (`memory::allocator`)**: Boilerplate for a logical Page Table that controls Virtual-to-Physical block mapping across PCIE bounds for KV Cache, along with Zero-Copy `mmap` wrappers.
* **Topology & Cost Matrix (`topology::mapper`)**: Boilerplate traits to model hardware limits via `hwloc` mappings, tracking Effective TFLOPS and Interconnect Bandwidth (GB/s).

## Development Setup

*Note: Since `cargo` is required, ensure you have the Rust `nightly` toolchain installed for advanced SIMD / inline assembly features in future phases.*

```bash
# 1. Build Core
cargo build --release

# 2. Run test suites for transfer calculations and page allocations
cargo test -p hmir-core
```
