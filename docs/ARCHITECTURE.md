# HMIR Architecture Blueprint

HMIR is a local LLM runtime for heterogeneous machines. The design target is simple to describe and hard to fake:

- Detect `NPU`, `GPU`, and `CPU` automatically.
- Prefer `NPU` when it is the best fit.
- Fall back cleanly to `GPU`, then `CPU`.
- Expose one local API instead of making developers reason about per-device runtimes.

This document turns that goal into a concrete, production-oriented system design for Windows, Linux, and macOS.

## Recommended Positioning

Recommended project name: `HMIR`

Expanded name: `Heterogeneous Model Inference Runtime`

One-line tagline:
`Run one local LLM service across NPU, GPU, and CPU with NPU-first scheduling and automatic fallback.`

Target audience:

- local AI developers who want one portable runtime
- open-source maintainers building on top of OpenAI-compatible APIs
- AI PC and edge-device teams that need predictable on-device inference

Core value proposition:
`HMIR turns mixed hardware into one inference target.`

## Design Principles

- `One control plane`: the scheduler owns all routing decisions.
- `Pluggable backends`: OpenVINO, llama.cpp, and future backends share one interface.
- `Model-aware scheduling`: do not route by device name alone.
- `Graceful degradation`: every request should have a fallback path.
- `Cross-platform first`: Windows, Linux, and macOS are primary targets.
- `Honest MVP`: no fake tensor-parallel magic in v1.

## Production Architecture

### Core Components

| Component | Responsibility | Current repo fit |
| --- | --- | --- |
| API Layer | OpenAI-compatible request ingress, auth, streaming, health, metrics | `hmir-api` |
| Scheduler | Device selection, queueing, batching, fallback, load balancing | `hmir-core` |
| Device Capability Detector | Probe hardware, drivers, memory, thermals, backend availability | `hmir-hardware-prober` |
| Model Manager | Model discovery, format validation, warm/load/unload, residency tracking | `hmir-core` |
| Backend Abstraction Layer | Stable interface over runtime-specific backends | `hmir-sys` plus new backend registry layer |
| Execution Engine | Runs plans chosen by the scheduler, streams tokens, reports telemetry | `hmir-core` |
| Telemetry Store | Live counters, EWMA latency, queue depth, model residency, failures | `hmir-core::telemetry` |

### Architecture Diagram

```text
                         +---------------------------+
                         |  CLI / SDK / OpenAI App   |
                         +-------------+-------------+
                                       |
                                       v
                         +---------------------------+
                         |       API / Gateway       |
                         |  REST, SSE, health, auth  |
                         +-------------+-------------+
                                       |
                                       v
                         +---------------------------+
                         |         Scheduler         |
                         |  route, queue, batch,     |
                         |  fallback, load-balance   |
                         +------+------+-------------+
                                |      |
               +----------------+      +----------------+
               |                                      |
               v                                      v
   +--------------------------+          +--------------------------+
   |      Model Manager       |          | Device Capability Store  |
   | load/unload, residency,  |          | hardware, memory, queue, |
   | model manifest, warmup   |          | thermals, backend health |
   +-------------+------------+          +-------------+------------+
                 |                                     |
                 +------------------+------------------+
                                    |
                                    v
                         +---------------------------+
                         |      Execution Engine     |
                         | executes plan + streams   |
                         +------+------+-------------+
                                |      |
        +-----------------------+      +------------------------+
        |                                                       |
        v                                                       v
+---------------------------+                     +---------------------------+
| OpenVINO Backend          |                     | llama.cpp Backend         |
| Intel NPU / GPU / CPU     |                     | CPU / CUDA / Vulkan /     |
| OpenVINO IR model packs   |                     | Metal / ROCm-ready GGUF   |
+---------------------------+                     +---------------------------+
        |                                                       |
        v                                                       v
  Intel NPU / GPU / CPU                         NVIDIA / AMD / Apple / CPU
```

### Request Lifecycle

1. Client sends `/v1/chat/completions`.
2. API layer normalizes request, attaches latency class, model ID, and stream mode.
3. Scheduler asks the capability store for a fresh device snapshot.
4. Model manager resolves the model manifest and compatible backends.
5. Scheduler scores eligible execution plans.
6. Execution engine runs the best plan.
7. If the plan fails or a device becomes unhealthy, scheduler retries on the next fallback target.
8. Telemetry updates latency, queue depth, failures, and device utilization for future decisions.

## Scheduler Design

The scheduler is the product. Everything else exists to make the scheduler informed and cheap to trust.

### Inputs

The scheduler should score each candidate plan using:

- `model size`: parameter count, quantization, estimated KV cache growth
- `request shape`: prompt length, max output tokens, streaming vs batch
- `latency class`: interactive, balanced, throughput
- `memory headroom`: free VRAM, free RAM, NPU SRAM limits when exposed
- `device capabilities`: supported model format, max context, batching support, streaming support
- `live load`: queue depth, active sessions, EWMA latency, thermal throttling
- `backend health`: recent failures, cold-start state, degraded mode

### Scheduling Rules

Baseline priority:

1. `NPU` if the model format is supported, memory fits, and latency target is satisfied.
2. `GPU` if NPU is unavailable, saturated, or incompatible.
3. `CPU` if both accelerator paths are unavailable or overloaded.

Hard guards:

- never route a request to a backend that cannot load the model format
- never route to a device without sufficient estimated memory
- never batch requests if doing so would violate the latency SLA for interactive traffic

Soft preferences:

- prefer already-loaded models over cold loads
- prefer devices with lower queueing delay
- prefer stable backends over recently failing backends
- prefer NPU for small-to-medium quantized chat models when it fits

### Hybrid Execution

HMIR should support two forms of hybrid execution:

1. `Request-level hybrid`
   Different requests for the same model can run on different devices at the same time.
   This is the MVP-friendly version of hybrid orchestration.

2. `Plan-level hybrid`
   A single request can use multiple devices when a backend pair supports it.
   Planned upgrade paths:
   - `speculative draft`: NPU drafts tokens, GPU or CPU verifies
   - `prefill/decode split`: GPU handles large prefill, NPU or CPU handles steady-state decode
   - `overflow offload`: KV cache or late-layer work spills to CPU when accelerator memory is tight

For the MVP, only `request-level hybrid` should be implemented. `Plan-level hybrid` should exist in the design as an execution-plan type, but not be required for v1 launch.

### Decision Flow Diagram

```text
START
  |
  v
Resolve model manifest
  |
  v
Build candidate backend/device plans
  |
  +--> No compatible plans? --> Return clear error
  |
  v
Filter plans by hard constraints
  - model format supported?
  - enough memory?
  - backend healthy?
  - context length allowed?
  |
  +--> None left? --> Try fallback model variant or queue until device frees
  |
  v
Score remaining plans
  - NPU preference bonus
  - warm model bonus
  - low queue penalty
  - low latency bonus
  - memory headroom bonus
  - recent failure penalty
  - cold start penalty
  |
  v
Pick highest score
  |
  +--> Interactive + compatible batch waiting?
  |      |
  |      +--> Yes, if within SLA, micro-batch
  |
  v
Dispatch to execution engine
  |
  +--> Success --> stream response + update telemetry
  |
  +--> Failure --> mark plan unhealthy --> retry next fallback
  |
  v
END
```

### Reference Scoring Model

Use a transparent weighted score before introducing learned routing:

```text
score =
  npu_preference_bonus
+ warm_model_bonus
+ latency_fit_bonus
+ memory_headroom_bonus
+ batching_efficiency_bonus
- queue_delay_penalty
- cold_start_penalty
- failure_risk_penalty
- thermal_throttle_penalty
```

Suggested defaults:

- `npu_preference_bonus = +25`
- `warm_model_bonus = +15`
- `latency_fit_bonus = +20`
- `memory_headroom_bonus = +10`
- `queue_delay_penalty = -1 * estimated_wait_ms / 10`
- `cold_start_penalty = -20`
- `failure_risk_penalty = -30` if recent failures exceed threshold

### Scheduler Pseudocode

```rust
fn schedule(req: InferenceRequest, state: &SchedulerState) -> Result<ExecutionPlan, ScheduleError> {
    let manifest = state.model_manager.resolve(&req.model_id)?;
    let device_snapshot = state.capability_store.snapshot();

    let mut candidates = Vec::new();

    for backend in state.backend_registry.backends() {
        if !backend.supports_model(&manifest) {
            continue;
        }

        for device in device_snapshot.devices_for_backend(backend.id()) {
            let estimate = backend.estimate(&manifest, &req, &device)?;

            if !estimate.fits_memory {
                continue;
            }

            if !estimate.supports_context {
                continue;
            }

            if device.health.is_unhealthy() {
                continue;
            }

            let mut score = 0_i64;

            if device.kind == DeviceKind::Npu {
                score += 25;
            }

            if state.model_manager.is_loaded(&manifest.id, backend.id(), device.id) {
                score += 15;
            } else {
                score -= 20;
            }

            score += latency_fit_score(req.latency_class, &estimate);
            score += memory_headroom_score(&estimate);
            score += batching_score(&req, &device);
            score -= queue_penalty(device.queue_depth, device.ewma_wait_ms);
            score -= failure_penalty(device.health.recent_failures);
            score -= thermal_penalty(device.telemetry.thermal_state);

            candidates.push(ScoredPlan {
                score,
                plan: ExecutionPlan::single_device(backend.id(), device.id, manifest.id.clone()),
            });
        }
    }

    candidates.sort_by(|a, b| b.score.cmp(&a.score));

    let best = candidates.first().ok_or(ScheduleError::NoEligibleDevice)?;
    Ok(best.plan.clone())
}
```

### Evolution Path

The scheduler should evolve in this order:

1. `Rule-based scoring`
2. `EWMA-aware scoring`
   Track real latency, queueing, failures, and cold-start cost.
3. `Adaptive weighting`
   Tune weights per model family and latency class.
4. `Cost-based optimization`
   Optimize for power, throughput, latency, or battery mode.
5. `Performance learning`
   Use bandit-style or Bayesian plan selection with safe fallback guards.

## Backend Abstraction Layer

### Goals

- isolate runtime-specific logic from the scheduler
- let every backend describe what it can and cannot do
- keep device binding explicit

### Unified Interface

```rust
pub trait BackendAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn probe(&self) -> BackendProbeResult;
    fn capabilities(&self) -> BackendCapabilities;

    async fn supports_model(&self, manifest: &ModelManifest) -> Result<bool, BackendError>;
    async fn load_model(&self, request: LoadModelRequest) -> Result<ModelHandle, BackendError>;
    async fn run_inference(
        &self,
        request: InferenceRequest,
        binding: DeviceBinding,
        model: &ModelHandle,
    ) -> Result<TokenStream, BackendError>;
    async fn unload_model(&self, model: &ModelHandle) -> Result<(), BackendError>;
}
```

### Capability Reporting

Each backend should report:

```rust
pub struct BackendCapabilities {
    pub supported_devices: Vec<DeviceKind>,
    pub supported_formats: Vec<ModelFormat>,
    pub max_context_tokens: Option<u32>,
    pub supports_streaming: bool,
    pub supports_batching: bool,
    pub supports_embeddings: bool,
    pub supports_vision: bool,
    pub supports_speculative_draft: bool,
    pub supports_prefill_decode_split: bool,
}
```

### Device Binding

Use explicit device bindings instead of string flags scattered across the codebase:

```rust
pub enum DeviceKind {
    Npu,
    Gpu,
    Cpu,
}

pub struct DeviceBinding {
    pub backend_id: String,
    pub device_id: String,
    pub device_kind: DeviceKind,
    pub memory_budget_bytes: u64,
}
```

### Recommended Backends

| Backend | Primary devices | Formats | OS |
| --- | --- | --- | --- |
| `OpenVINO` | Intel NPU, Intel GPU, CPU | OpenVINO IR directories | Windows, Linux |
| `llama.cpp` | CPU, NVIDIA CUDA, Vulkan, Metal, ROCm-capable paths | GGUF | Windows, Linux, macOS |
| `MLX` | Apple Silicon GPU and ANE-adjacent Apple stack | MLX model packages | macOS |

Notes:

- `OpenVINO` is the best first backend for Intel NPU-first routing.
- `llama.cpp` is the broadest fallback backend and should be the default cross-platform safety net.
- `MLX` is optional for MVP, but it is the cleanest long-term native backend for Apple Silicon.

## Compatible Model Strategy

Do not advertise arbitrary model names. Advertise `model package + backend` combinations.

### MVP-Compatible Model Packs

| Alias | Format | Primary backend | Intended target |
| --- | --- | --- | --- |
| `qwen2.5-1.5b-ov` | OpenVINO IR | OpenVINO | Intel NPU |
| `phi3-mini-ov` | OpenVINO IR | OpenVINO | Intel NPU |
| `phi3-mini` | GGUF | llama.cpp | CPU / light GPU |
| `llama3.2-3b` | GGUF | llama.cpp | GPU / CPU fallback |
| `llama3-8b-gguf` | GGUF | llama.cpp | Larger GPU / CPU fallback |

Rule of thumb:

- `OpenVINO IR` for `OpenVINO` backends
- `GGUF` for `llama.cpp`
- `MLX` packages for `MLX`

## MVP Feature Set

The MVP should feel noticeably better than manually juggling runtimes, but still be small enough to ship.

### Include

- automatic hardware detection on Windows, Linux, and macOS
- backend probing at startup
- NPU-first scheduling when a compatible NPU path exists
- automatic fallback `NPU -> GPU -> CPU`
- request-level load balancing across loaded backends
- OpenAI-compatible `/v1/chat/completions`
- simple `hmir suggest`, `hmir pull`, and `hmir start`
- model auto-load and lazy warmup
- logs showing selected backend and selected device
- telemetry for queue depth, latency, and device utilization

### Exclude From MVP

- tensor parallelism across multiple GPUs
- distributed multi-node serving
- learned routing models
- full multimodal platform coverage
- same-request hybrid execution unless backend support is already proven

## Differentiation Strategy

### Compared With Lemonade

Lemonade already emphasizes broad local compatibility, OpenAI-style APIs, multi-engine support, and cross-platform setup. Source: [lemonade-server.ai](https://lemonade-server.ai/).

HMIR should differentiate by being:

- `device-orchestration first`, not just engine selection first
- `scheduler-centric`, where routing is a primary product feature
- `predictable for developers`, with explicit device logs and deterministic fallback
- `maintainer-friendly`, with one backend contract and one model manifest shape

In short:
`Lemonade is a local AI service platform; HMIR should become the best open runtime for device-aware inference orchestration.`

### Compared With FastFlowLM

FastFlowLM is deeply optimized for AMD Ryzen AI NPUs and presents itself as an NPU-first runtime for that hardware family. Sources: [FastFlowLM GitHub](https://github.com/FastFlowLM/FastFlowLM) and [fastflowlm.com](https://fastflowlm.com/).

HMIR should differentiate by being:

- `cross-hardware`, not vendor-specific
- `cross-backend`, not runtime-specific
- `fallback-native`, not single-accelerator dependent
- `API-platform oriented`, with one local endpoint regardless of backend

In short:
`FastFlowLM is the specialist. HMIR should be the orchestrator.`

## Recommended Repo Structure

Use the existing workspace, but tighten boundaries:

```text
docs/
  ARCHITECTURE.md

crates/
  hmir-api/                # OpenAI-compatible gateway
  hmir-scheduler/          # routing, queueing, batching, fallback
  hmir-device-probe/       # hardware and driver detection
  hmir-model-manager/      # manifests, residency, model lifecycle
  hmir-runtime/            # execution engine
  hmir-backend-openvino/   # OpenVINO adapter
  hmir-backend-llamacpp/   # llama.cpp adapter
  hmir-backend-mlx/        # future Apple backend
  hmir-telemetry/          # metrics, events, health
  hmir-cli/                # suggest/pull/start UX

scripts/
  hmir_npu_service.py      # temporary compatibility worker, not the final control plane
```

### Mapping From Today’s Repo

- `hmir-api` stays the gateway
- `hmir-core` should become the scheduler + runtime + model manager split
- `hmir-hardware-prober` should remain the device probe crate
- `hmir-sys` should shrink into low-level backend bindings plus backend-specific crates
- Python worker scripts should become transitional adapters, not long-term architecture anchors

## Recommended Tech Stack

- `Rust`
  Control plane, scheduler, API, CLI, telemetry, and model management
- `Tokio + Axum`
  Async runtime and OpenAI-compatible server
- `Serde`
  Request, response, manifest, and telemetry schemas
- `tracing + metrics`
  Logs, counters, and scheduler observability
- `OpenVINO`
  Intel NPU and Intel heterogeneous acceleration path
- `llama.cpp`
  Broad CPU/GPU fallback layer across Windows, Linux, and macOS
- `MLX`
  Future native Apple path
- `Python`
  Transitional glue only where backend SDKs are not yet cleanly wrapped in Rust

## Delivery Plan

### Phase 1

- normalize model manifests
- introduce backend registry
- add explicit device inventory and health store
- route requests through a rule-based scheduler

### Phase 2

- add queue-aware batching
- add warm model residency policy
- add request-level load balancing
- add structured device selection logs

### Phase 3

- add speculative draft plans
- add adaptive scoring
- add per-model performance profiles

### Phase 4

- add richer backend set
- add battery/power-aware routing
- add formal benchmarking harness

## Implementation Guidance

If you want the shortest path to a strong open-source v1:

1. Keep `HMIR` as the name.
2. Ship `OpenVINO + llama.cpp` first.
3. Make the scheduler and model manifest the center of the design.
4. Treat the Python NPU worker as an interim bridge, not the end-state.
5. Make every device choice visible in logs and API metadata.

That combination is enough to make HMIR feel distinct, credible, and implementable.
