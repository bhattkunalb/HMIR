# 🚀 HMIR: Heterogeneous Memory-First Inference Runtime

**Run local LLMs that automatically orchestrate NPU + GPU + CPU for maximum performance per watt.**

[![GitHub Release](https://img.shields.io/github/v/release/bhattkunalb/HMIR?label=release)](https://github.com/bhattkunalb/HMIR/releases)
[![License](https://img.shields.io/github/license/bhattkunalb/HMIR)](LICENSE)
[![CI](https://github.com/bhattkunalb/HMIR/actions/workflows/ci.yml/badge.svg)](https://github.com/bhattkunalb/HMIR/actions)
<!-- [![Crates.io](https://img.shields.io/crates/v/hmir.svg)](https://crates.io/crates/hmir) --> <!-- Uncomment after first crates.io publish -->

---

## 📦 One-Command Install & Launch

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.sh | sh

# Windows (PowerShell - Run as Administrator for NPU drivers)
irm https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.ps1 | iex

# Docker (No install required)
docker run --gpus all -p 8080:8080 -p 3001:3001 ghcr.io/bhattkunalb/hmir:latest
```

✅ **That's it.** HMIR auto-detects your hardware, loads a fallback model, opens the dashboard, and starts an OpenAI-compatible API at `http://localhost:8080`.

---

## ⚙️ Initial Requirements

| Component | Requirement |
| --- | --- |
| **OS** | macOS 13+, Windows 10/11 22H2+, Ubuntu 20.04+ / Fedora 36+ |
| **RAM** | 8GB minimum (16GB+ recommended for 7B models) |
| **Storage** | 5GB for runtime + model cache |
| **GPU Drivers** | NVIDIA CUDA 12+, Apple Metal (built-in), AMD ROCm/Vulkan (optional) |
| **NPU Drivers** | Apple Neural Engine (built-in), Intel Core Ultra / Qualcomm Snapdragon (optional, auto-fallback) |

🔍 **Do you need to install `llama.cpp` separately?**  
**No.** HMIR bundles `llama.cpp` as a compiled, statically linked dependency. It is fetched and optimized during build or included in prebuilt binaries. Zero external setup required.

---

## 🎯 Auto-Model Recommendation

Run `hmir suggest` to get hardware-optimized model recommendations:

```bash
$ hmir suggest --strategy latency
🔍 Probing hardware...
✅ Detected: NVIDIA RTX 4070 (8GB VRAM), 32GB RAM, PCIe 4.0
📊 Routing Strategy: Latency-Optimized

RECOMMENDED MODELS:
1. Meta-Llama-3-8B-Instruct-Q4_K_M.gguf
   • VRAM: ~5.2 GB | RAM: 0 GB | Expected TTFT: <450ms
   • Routing: GPU (CUDA) → CPU fallback
   • Command: hmir load models/Meta-Llama-3-8B-Instruct-Q4_K_M.gguf

2. Phi-3-mini-4k-instruct-Q5_K_S.gguf
   • VRAM: ~3.1 GB | RAM: 0 GB | Expected TTFT: <280ms
   • Routing: GPU (CUDA) + NPU draft (if available)
   • Command: hmir load models/Phi-3-mini-4k-instruct-Q5_K_S.gguf
```

---

## 🖥️ Dashboard & API Access

### Live TaskManager UI

Launch with: `hmir start --dashboard`

- Real-time CPU/GPU/NPU utilization bars
- Active task registry with color-coded routing (🔵 GPU, 🟣 NPU, 🟠 CPU)
- Speculative acceptance rate, swap throughput, memory pressure graphs
- Controls: Pause/Resume/Kill, Strategy toggle, Hot-swap models, Force fallback

### OpenAI-Compatible API

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "meta-llama-3-8b",
    "messages": [{"role": "user", "content": "Explain quantum entanglement simply"}],
    "stream": true,
    "priority": "foreground"
  }'
```

Metrics: `http://localhost:8080/metrics` (Prometheus-compatible)

---

## 🔧 Troubleshooting

| Issue | Fix |
| --- | --- |
| NPU not detected | Check driver installation. HMIR safely blacklists missing NPUs and routes to GPU/CPU. Run `hmir logs --level debug` |
| VRAM OOM during long context | HMIR auto-swaps KV cache to RAM. Reduce batch size: `hmir config set batch_max_tokens 2048` |
| Dashboard blank on first run | Wait 2-3 seconds for telemetry stream. Verify port 3001 isn't blocked. Run `hmir status` |
| High CPU usage on startup | Normal: topology mapping + JIT kernel compilation. Stabilizes after first prompt. |

Collect logs: `hmir logs --since 5m > hmir_debug.log`

---

## 🌐 Hardware Compatibility Matrix

| Platform | NPU | GPU | CPU | Speculative Decoding | Notes |
| --- | --- | --- | --- | --- | --- |
| Apple Silicon (M1/M2/M3) | ✅ ANE | ✅ Metal | ✅ ARM | ✅ Unified Memory optimized | Best tokens/watt |
| Windows + Snapdragon X Elite | ✅ Qualcomm QNN | ❌ | ✅ ARM | ✅ NPU draft + CPU verify | Battery champion |
| Linux + RTX 30/40 series | ❌ | ✅ CUDA | ✅ x86 | ⚠️ CPU draft + GPU verify | Max raw throughput |
| Intel Core Ultra | ✅ NPU | ✅ Arc/iGPU | ✅ x86 | ✅ NPU draft + GPU verify | Balanced hybrid |

---

## 🤝 Contributing & Releases

See `CONTRIBUTING.md` for architecture guides, benchmark methodology, and plugin SDK.
Releases follow semantic versioning. Prebuilt binaries include checksums. Auto-update: `hmir-cli update`

**License**: MIT | **Built with**: Rust, llama.cpp, ONNX, egui, axum, tokio
