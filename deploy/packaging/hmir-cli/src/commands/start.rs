use std::time::Duration;
use tokio::process::Command;

pub async fn start_daemon(port: u16, dashboard: bool, model: Option<String>) {
    let mut bin_dir = std::env::current_exe().unwrap_or_default();
    bin_dir.pop();

    println!("🚀 HMIR ELITE | INITIALIZING INFERENCE NODE");
    println!("--------------------------------------------------");

    // 1. Start NPU Execution Bridge (Python)
    print!("⚙️ Activating NPU Bridge... ");
    let mut bridge_path = std::env::current_exe().unwrap_or_default();
    bridge_path.pop(); // Remove bin name
    bridge_path.push("scripts");
    bridge_path.push("hmir_npu_worker.py");

    let bridge_proc = Command::new("python")
        .arg(&bridge_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match bridge_proc {
        Ok(_) => println!("✅ [OK]"),
        Err(e) => println!(
            "⚠️ [WARN] Failed to spawn bridge: {}. (Ensure python is in PATH)",
            e
        ),
    }

    // 2. Start HMIR API Server
    print!("🔌 Starting Inference API (port {})... ", port);
    let api_path = bin_dir.join("hmir-api.exe");
    let mut api_cmd = Command::new(&api_path);
    api_cmd.env("HMIR_PORT", port.to_string());
    if let Some(m) = model {
        api_cmd.env("HMIR_DEFAULT_MODEL", m);
    }

    match api_cmd.spawn() {
        Ok(_) => {
            // Health check loop
            let client = reqwest::Client::new();
            let mut success = false;
            for _i in 1..=10 {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                if let Ok(res) = client
                    .get(format!("http://127.0.0.1:{}/v1/health", port))
                    .send()
                    .await
                {
                    if res.status().is_success() {
                        success = true;
                        break;
                    }
                }
                print!(".");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            if success {
                println!("✅ [OK]");
            } else {
                println!("⚠️ [WARN] API started but health-check timed out. Check logs.");
            }
        }
        Err(e) => {
            println!("❌ [ERROR] Failed to start API: {}", e);
            return;
        }
    }

    // 3. Start Native Dashboard
    if dashboard {
        print!("🖥️ Launching Native Dashboard... ");
        let dash_path = bin_dir.join("hmir-dashboard.exe");
        match Command::new(&dash_path).spawn() {
            Ok(_) => println!("✅ [OK]"),
            Err(e) => println!("⚠️ [WARN] Could not launch native dashboard: {}", e),
        }
    }

    // 4. Auto-Open Web Portal
    let unified_url = format!("http://127.0.0.1:{}", port);
    println!("🌐 Auto-opening Web Console: {}", unified_url);
    tokio::time::sleep(Duration::from_secs(2)).await; // Give API time to bind
    if let Err(e) = webbrowser::open(&unified_url) {
        println!("⚠️  Unable to open browser: {}", e);
    }

    println!("--------------------------------------------------");
    println!("🚀 HMIR API: {}", unified_url);
    println!("💎 Node is running in background.");
    println!("💡 Use 'hmir stop' to terminate all instances.");
}
