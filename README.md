# 💎 HMIR ELITE: Heterogeneous Model Inference Runtime

**HMIR (Heterogeneous Model Inference Runtime)** is a high-performance, local-first intelligence engine that orchestrates NPUs, GPUs, and CPUs into a single unified compute fabric. 

Built for the "AI PC" era, HMIR prioritizes thermal-efficient NPU execution (Intel AI Boost, Qualcomm Hexagon, Apple Neural Engine) while speculative-scheduling across available GPU clusters to deliver maximum performance-per-watt.

---

## ⚡ Power One-Click Install

### Windows (AI PC Native)

Run as Administrator in PowerShell to enable deep hardware probing:

```powershell
irm https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.ps1 | iex
```

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.sh | sh
```

---

## 🛰️ Elite Orchestration CLI

The new `hmir` CLI manages the entire lifecycle of your intelligence node.

### 🔍 Hardware Intelligence Routing

Probes your silicon layer and suggests the optimal intelligence tier for your current thermals and memory pressure.

```bash
$ hmir suggest
  _    _ __  __ _____ _____  
 | |  | |  \/  |_   _|  __ \ 
 | |__| | \  / | | | | |__) |
 |  __  | |\/| | | | |  _  / 
 | |  | | |  | |_| |_| | \ \ 
 |_|  |_|_|  |_|_____|_|  \\_\\

[ HMIR ELITE | Intelligence Routing Engine ]

🔍 Probing Hardware Layer...
✅ CPU: Intel(R) Core(TM) Ultra 7 155H
✅ GPU: Intel(R) Arc(TM) Graphics
✅ NPU: Intel(R) AI Boost (⚡ HIGH-SPEED DETECTED)
🌡️  Thermals: 42.1°C 🟢 (Optimal)
📊 Memory: 32.0 GiB Total Physical RAM

💎 RECOMMENDED INTELLIGENCE TIERS:
--------------------------------------------------
🥇 [ELITE TIER] Qwen 2.5 1.5B (INT4 OpenVINO)
   • Reason: NATIVE NPU ACCELERATION available via Intel/Qualcomm
   • Stats: ~120 T/s | Ultra-low Power | 0% CPU Overhead
   👉 Command: hmir start --model qwen2.5-1.5b-ov

🥈 [ULTIMATE TIER] Llama 3.1 8B (INT4 OpenVINO)
   • Reason: High-fidelity reasoning on Intel(R) AI Boost silicon
   • Stats: ~25 T/s | Balanced Power
   👉 Command: hmir start --model llama-3.1-8b-ov
--------------------------------------------------
```

### 🚀 Instant Deployment
Start the background daemon and automatically launch the unified web console.
```bash
hmir start --dashboard
```

---

## 🖥️ Unified Control Center (Dashboard)

HMIR includes a native, high-performance telemetry dashboard (`hmir-dashboard`) built with Rust and egui.

- **Real-time Silicon Monitoring**: Per-core utilization, NPU throughput, and thermal zones.
- **VRAM Logic**: Native tracking of dedicated vs. shared video memory.
- **Intelligence Vault**: One-click NPU model downloads and hot-swapping.
- **Unified Chat**: Access the local web portal at `http://localhost:8081` for the full chat experience.

---

## 🏗️ Technical Architecture

- **`hmir-core`**: The heartbeat. Handles the scheduling logic and heterogeneous memory management.
- **`hmir-hardware-prober`**: Deep silicon discovery across WMI (Windows), sysfs (Linux), and sysctl (macOS).
- **`hmir-api`**: High-throughput Axum server with OpenAI-compatible endpoint compatibility.
- **`hmir-npu-worker`**: Execution bridge for OpenVINO and QNN-optimized NPU interference.

---

## 🤝 Community & Support

- **Repository**: [bhattkunalb/HMIR](https://github.com/bhattkunalb/HMIR)
- **License**: MIT
- **Built with**: Rust 🦀, OpenVINO, llama.cpp, egui, axum.

---
**HMIR: The Silicon-Aware Runtime.**
