use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

pub async fn start_daemon(port: u16, dashboard: bool, model: Option<String>, no_browser: bool) {
    let bin_dir = current_bin_dir();

    println!("🚀 HMIR ELITE | INITIALIZING INFERENCE NODE");
    println!("--------------------------------------------------");

    // 1. Start NPU Execution Bridge (Python)
    print!("⚙️ Activating hardware bridge... ");
    if let Some(bridge_path) = resolve_script_path(&bin_dir, "hmir_npu_service.py") {
        let bridge_proc = Command::new(resolve_python_command(&bin_dir))
            .arg(&bridge_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match bridge_proc {
            Ok(_) => println!("✅ [OK]"),
            Err(e) => println!(
                "⚠️ [WARN] Failed to spawn bridge: {}. HMIR will still try GPU/CPU paths.",
                e
            ),
        }
    } else {
        println!("⚠️ [WARN] No bridge script found. Continuing without Python bridge.");
    }

    // 2. Start HMIR API Server
    print!("🔌 Starting Inference API (port {})... ", port);
    let api_path = bin_dir.join(executable_name("hmir-api"));
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
        let dash_path = bin_dir.join(executable_name("hmir-dashboard"));
        match Command::new(&dash_path)
            .env("HMIR_API_BASE_URL", format!("http://127.0.0.1:{}", port))
            .spawn()
        {
            Ok(_) => println!("✅ [OK]"),
            Err(e) => println!("⚠️ [WARN] Could not launch native dashboard: {}", e),
        }
    }

    // 4. Auto-Open Web Portal only for browser-first flow
    let unified_url = format!("http://127.0.0.1:{}", port);
    if !dashboard && !no_browser {
        println!("🌐 Auto-opening Web Console: {}", unified_url);
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Err(e) = webbrowser::open(&unified_url) {
            println!("⚠️  Unable to open browser: {}", e);
        }
    }

    println!("--------------------------------------------------");
    println!("🚀 HMIR API: {}", unified_url);
    println!("🔌 OpenAI-compatible base URL: {}/v1", unified_url);
    println!("💎 Node is running in background.");
    if dashboard {
        println!("🖥️  Native dashboard includes chat, model controls, integrations, and logs.");
    }
    if no_browser {
        println!("🌙 Headless mode enabled. Ready for editor and agent integrations.");
    }
    println!("💡 Use 'hmir stop' to terminate all instances.");
}

pub async fn launch_dashboard(port: u16) {
    let bin_dir = current_bin_dir();
    let dash_path = bin_dir.join(executable_name("hmir-dashboard"));
    let url = format!("http://127.0.0.1:{}", port);

    match Command::new(dash_path)
        .env("HMIR_API_BASE_URL", url)
        .spawn()
    {
        Ok(_) => println!("✅ Dashboard launched."),
        Err(e) => println!("❌ Failed to launch dashboard: {}", e),
    }
}

fn current_bin_dir() -> PathBuf {
    let mut path = std::env::current_exe().unwrap_or_default();
    path.pop();
    path
}

fn executable_name(base: &str) -> String {
    format!("{}{}", base, std::env::consts::EXE_SUFFIX)
}

fn resolve_script_path(bin_dir: &Path, script_name: &str) -> Option<PathBuf> {
    let candidates = [
        bin_dir.join("scripts").join(script_name),
        std::env::current_dir()
            .unwrap_or_default()
            .join("scripts")
            .join(script_name),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn resolve_python_command(bin_dir: &Path) -> String {
    let candidates = [
        std::env::current_dir()
            .unwrap_or_default()
            .join(".venv")
            .join("Scripts")
            .join("python.exe"),
        std::env::current_dir()
            .unwrap_or_default()
            .join(".venv")
            .join("bin")
            .join("python"),
        bin_dir.join(".venv").join("Scripts").join("python.exe"),
        bin_dir.join(".venv").join("bin").join("python"),
    ];

    if let Some(path) = candidates.into_iter().find(|path| path.exists()) {
        return path.to_string_lossy().to_string();
    }

    if cfg!(target_os = "windows") {
        "python".to_string()
    } else {
        "python3".to_string()
    }
}
