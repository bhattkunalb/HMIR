use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

pub async fn run_status(port: u16) {
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_default();

    let url = format!("http://127.0.0.1:{}/v1", port);
    
    println!("🔍 HMIR ELITE | RUNTIME STATUS");
    println!("   Endpoint: {}\n", url);

    // 1. Check Health
    match client.get(format!("{}/health", url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            println!("   ✅ Runtime: ONLINE");
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(engine) = json.get("engine").and_then(|v| v.as_str()) {
                    println!("   📡 Engine : {}", engine);
                }
            }
        }
        _ => {
            println!("   ❌ Runtime: OFFLINE (Port {} may be blocked or service not started)", port);
            return;
        }
    }

    // 2. Check Active Model
    match client.get(format!("{}/models/installed", url)).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(models) = json.get("models").and_then(|v| v.as_array()) {
                    let active = models.iter().find(|m| m.get("active").and_then(|v| v.as_bool()).unwrap_or(false));
                    if let Some(m) = active {
                        let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
                        println!("   📦 Active : {}", name);
                    } else {
                        println!("   📦 Active : None (Idle)");
                    }
                }
            }
        }
        Err(_) => println!("   📦 Active : Unknown (Failed to fetch model list)"),
    }

    // 3. Hardware Snapshot
    if let Ok(resp) = client.get(format!("{}/hardware/snapshot", url)).send().await {
        if let Ok(json) = resp.json::<Value>().await {
            println!("\n   🖥️  Hardware Profile:");
            if let Some(cpu) = json.get("cpu_name").and_then(|v| v.as_str()) {
                println!("      CPU: {}", cpu);
            }
            if let Some(gpu) = json.get("gpu_name").and_then(|v| v.as_str()) {
                println!("      GPU: {}", gpu);
            }
            if let Some(npu) = json.get("npu_name").and_then(|v| v.as_str()) {
                println!("      NPU: {}", npu);
            }
            
            let ram_used = json.get("ram_used").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let ram_total = json.get("ram_total").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if ram_total > 0.0 {
                println!("      RAM: {:.1} GB / {:.1} GB ({:.1}%)", ram_used, ram_total, (ram_used / ram_total) * 100.0);
            }
        }
    }
    
    println!("\n✨ HMIR ELITE is healthy and ready for inference.");
}
