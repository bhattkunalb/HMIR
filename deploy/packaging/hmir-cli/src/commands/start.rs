use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub async fn start_daemon(port: u16, web: bool, model: Option<String>, no_browser: bool) {
    let bin_dir = current_bin_dir();
    let client = reqwest::Client::new();
    let unified_url = format!("http://127.0.0.1:{}", port);

    println!("🚀 HMIR ELITE | INITIALIZING INFERENCE NODE");
    println!("--------------------------------------------------");

    // 0. Pre-flight check: Is the port already in use?
    print!("🔍 Checking port {} availability... ", port);
    let is_busy = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok();

    if is_busy {
        // Check if it's our own HMIR node
        if let Ok(res) = client
            .get(format!("{}/v1/health", unified_url))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if res.status().is_success() {
                println!("✅ [ATTACHED]");
                println!("💡 HMIR node is already running on this port. Using existing instance.");
            } else {
                println!("❌ [ERROR]");
                println!("🛑 Port {} is occupied by another process that is NOT an HMIR node.", port);
                println!("   Please free the port or use '--port <PORT>' to start on a different port.");
                return;
            }
        } else {
            println!("❌ [ERROR]");
            println!("🛑 Port {} is blocked or occupied by a non-responsive process.", port);
            return;
        }
    } else {
        println!("✅ [FREE]");

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

        #[cfg(windows)]
        {
            const DETACHED_PROCESS: u32 = 0x00000008;
            api_cmd.creation_flags(DETACHED_PROCESS);
        }

        match api_cmd.spawn() {
            Ok(_) => {
                // Health check loop
                let mut success = false;
                for _i in 1..=30 {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    if let Ok(res) = client
                        .get(format!("{}/v1/health", unified_url))
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
    }

    // 3. Start Native Dashboard
    if !web && !no_browser {
        print!("🖥️ Launching Native Dashboard... ");
        let dash_path = bin_dir.join(executable_name("hmir-dashboard"));
        match Command::new(&dash_path)
            .env("HMIR_API_BASE_URL", &unified_url)
            .spawn()
        {
            Ok(_) => println!("✅ [OK]"),
            Err(e) => println!("⚠️ [WARN] Could not launch native dashboard: {}", e),
        }
    }

    // 4. Auto-Open Web Portal only for browser-first flow
    if web && !no_browser {
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
    if !web {
        println!("🖥️  Native dashboard includes chat, model controls, integrations, and logs.");
    } else {
        println!("🌐 Legacy Web UI has been launched in your browser.");
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
