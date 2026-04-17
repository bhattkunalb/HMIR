use tokio::process::Command;
use std::path::PathBuf;
use std::time::Duration;

pub async fn start_daemon(port: u16, dashboard: bool, model: Option<String>) {
    let mut bin_dir = std::env::current_exe().unwrap_or_default();
    bin_dir.pop(); 

    println!("🚀 HMIR ELITE | INITIALIZING INFERENCE NODE");
    println!("--------------------------------------------------");

    // 1. Start NPU Execution Bridge (Python)
    print!("⚙️ Activating NPU Bridge... ");
    let mut bridge_path = std::env::current_dir().unwrap_or_default();
    bridge_path.push("scripts");
    bridge_path.push("hmir_npu_worker.py");

    let mut bridge_proc = Command::new("python")
        .arg(&bridge_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match bridge_proc {
        Ok(_) => println!("✅ [OK]"),
        Err(e) => println!("⚠️ [WARN] Failed to spawn bridge: {}. (Ensure python is in PATH)", e),
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
        Ok(_) => println!("✅ [OK]"),
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
    println!("🌐 Auto-opening Web Console: http://localhost:8081");
    tokio::time::sleep(Duration::from_secs(2)).await; // Give API time to bind
    if let Err(e) = webbrowser::open("http://localhost:8081") {
         println!("⚠️  Unable to open browser: {}", e);
    }

    println!("--------------------------------------------------");
    println!("💎 Node is running in background.");
    println!("💡 Use 'hmir stop' to terminate all instances.");
}
