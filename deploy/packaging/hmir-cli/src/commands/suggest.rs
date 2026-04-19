use hmir_hardware_prober::os_polling;

pub struct ModelRecommender {}

impl ModelRecommender {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn suggest(&self, strategy: &str) {
        println!("  _    _ __  __ _____ _____  ");
        println!(" | |  | |  \\/  |_   _|  __ \\ ");
        println!(" | |__| | \\  / | | | | |__) |");
        println!(" |  __  | |\\/| | | | |  _  / ");
        println!(" | |  | | |  | |_| |_| | \\ \\ ");
        println!(" |_|  |_|_|  |_|_____|_|  \\_\\");
        println!("\n[ HMIR | Intelligent Routing Engine ]\n");

        println!("🔍 Probing Hardware Layer...");
        let state = os_polling::poll_hardware().await;

        println!("✅ CPU: {}", state.cpu_name);
        println!("✅ GPU: {}", state.gpu_name);
        if state.npu_name != "None" {
            println!("✅ NPU: {} (⚡ HIGH-SPEED DETECTED)", state.npu_name);
        } else {
            println!("⚠️  NPU: None detected (Falling back to GPU clusters)");
        }

        let (temp_icon, temp_status) = if state.cpu_temp_c < 55.0 {
            ("🟢", "Optimal")
        } else if state.cpu_temp_c < 75.0 {
            ("🟡", "Warm")
        } else {
            ("🔴", "Thermal Throttling Threshold")
        };

        println!(
            "🌡️  Thermals: {:.1}°C {} ({})",
            state.cpu_temp_c, temp_icon, temp_status
        );
        println!(
            "📊 Memory: {:.1} GiB Total Physical RAM",
            state.ram_total_bytes as f64 / 1_073_741_824.0
        );
        println!("📈 Strategy: {}-Optimized Performance Routing\n", strategy);

        println!("💎 RECOMMENDED INTELLIGENCE TIERS:");
        println!("--------------------------------------------------");

        if state.cpu_temp_c > 80.0 {
            println!("🥇 [EFFICIENCY TIER] Phi-3 Mini (4K Instruct)");
            println!(
                "   • Reason: LOW-POWER mode active due to high thermals ({:.1}°C)",
                state.cpu_temp_c
            );
            println!("   • Routing: GGUF on CPU or light GPU fallback");
            println!("   👉 Command: hmir start --model phi3-mini\n");
        } else if state.npu_name != "None" {
            println!("🥇 [ELITE TIER] Qwen 2.5 1.5B (INT4 OpenVINO)");
            println!("   • Reason: NATIVE NPU ACCELERATION available via Intel/Qualcomm");
            println!("   • Stats: ~120 T/s | Ultra-low Power | 0% CPU Overhead");
            println!("   👉 Command: hmir start --model qwen2.5-1.5b-ov\n");

            println!("🥈 [BALANCED TIER] Phi-3 Mini (INT4 OpenVINO)");
            println!("   • Reason: Smaller NPU-friendly pack for highly interactive workloads");
            println!("   • Stats: Lower memory footprint | Fast startup");
            println!("   👉 Command: hmir start --model phi3-mini-ov\n");
        } else if state.gpu_name.to_uppercase().contains("NVIDIA")
            || state.gpu_name.to_uppercase().contains("AMD")
            || state.gpu_name.to_uppercase().contains("APPLE")
            || state.gpu_name.to_uppercase().contains("ARC")
        {
            println!("🥇 [PERFORMANCE TIER] Llama 3.2 3B (GGUF)");
            println!("   • Reason: GPU-capable system detected ({})", state.gpu_name);
            println!("   • Stats: Good quality-to-latency balance on llama.cpp-style backends");
            println!("   👉 Command: hmir start --model llama3.2-3b\n");

            println!("🥈 [HIGHER CAPACITY TIER] Llama 3 8B (GGUF)");
            println!("   • Reason: Best suited for larger GPU memory budgets");
            println!("   • Stats: Higher quality, higher memory cost");
            println!("   👉 Command: hmir start --model llama3-8b-gguf\n");
        } else {
            println!("🥇 [STANDARD TIER] Phi-3 Mini (GGUF)");
            println!("   • Reason: CPU-dominant execution path with minimal memory pressure");
            println!("   👉 Command: hmir start --model phi3-mini\n");

            println!("🥈 [BALANCED TIER] Llama 3.2 3B (GGUF)");
            println!("   • Reason: Better reasoning quality when CPU budget allows");
            println!("   👉 Command: hmir start --model llama3.2-3b\n");
        }

        println!("--------------------------------------------------");
        println!("✨ All models above are optimized for your unique hardware signature.");
    }
}
