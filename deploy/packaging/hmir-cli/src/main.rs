use clap::{Parser, Subcommand};
// cSpell:ignore USERPROFILE, WINDOWTITLE
mod commands;

const LONG_ABOUT: &str = "HMIR is a local heterogeneous inference runtime.\n\nIt exposes one OpenAI-compatible local API across NPU, GPU, and CPU so local apps and editors can use the same endpoint without manual device juggling.";

const AFTER_HELP: &str = "Examples:\n  hmir suggest\n  hmir pull qwen2.5-1.5b-ov\n  hmir start --dashboard --model qwen2.5-1.5b-ov\n  hmir start --no-browser --port 8080\n  hmir integrations --model llama3.2-3b\n  hmir logs --tail 200 --grep ERROR\n\nOpenAI-compatible clients should use:\n  Base URL: http://127.0.0.1:8080/v1\n  API Key : hmir-local";

#[derive(Parser)]
#[command(name = "hmir")]
#[command(version)]
#[command(propagate_version = true)]
#[command(about = "HMIR: Heterogeneous Model Inference Runtime")]
#[command(long_about = LONG_ABOUT)]
#[command(after_help = AFTER_HELP)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Suggest the best model and strategy for your current hardware
    Suggest {
        /// The optimization strategy (latency, throughput, battery)
        #[arg(short, long, default_value = "latency")]
        strategy: String,
    },
    /// Pull a model from the registry
    Pull {
        /// The name or URL of the model to pull
        model: String,
    },
    /// Start the inference daemon and optional dashboard
    #[command(visible_alias = "serve")]
    Start {
        /// The port to listen on for the API
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Launch the legacy web dashboard in the browser instead of the native app
        #[arg(short, long)]
        web: bool,
        /// The model to load on startup
        #[arg(short, long)]
        model: Option<String>,
        /// Do not launch any UI (native dashboard or browser)
        #[arg(long, visible_alias = "headless")]
        no_browser: bool,
    },
    /// Stop all running HMIR instances
    Stop,
    /// Launch the native dashboard directly
    #[command(visible_alias = "ui")]
    Dashboard {
        /// The port to connect to the API on
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    /// Show OpenAI-compatible integration settings for editors and local apps
    Integrations {
        /// The API port your HMIR runtime is listening on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Suggested model name to display in the examples
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Inspect local HMIR logs
    Logs {
        /// Number of lines to show from the end of each log
        #[arg(long, default_value = "120")]
        tail: usize,
        /// Filter log lines that contain this text
        #[arg(long)]
        grep: Option<String>,
        /// Follow log updates
        #[arg(short, long)]
        follow: bool,
        /// Print the log directory and exit
        #[arg(long)]
        dir: bool,
    },
    /// Purge runtime caches (OpenVINO, etc.) to resolve loading errors
    Clean,
    /// Uninstall HMIR ELITE and purge all runtime data
    Uninstall,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Suggest { strategy } => {
            println!("🚀 HMIR Hardware Intelligence");
            let recommender = commands::suggest::ModelRecommender::new();
            recommender.suggest(&strategy).await;
        }
        Commands::Pull { model } => {
            println!("📥 HMIR Model Downloader");
            commands::pull::pull_model(&model).await;
        }
        Commands::Start {
            port,
            web,
            model,
            no_browser,
        } => {
            println!("🚀 Launching HMIR ELITE Compute Hub");
            commands::start::start_daemon(port, web, model, no_browser).await;
        }
        Commands::Stop => {
            stop_all_instances();
            println!("✅ HMIR ELITE specialized resources released.");
        }
        Commands::Dashboard { port } => {
            println!("🖥️  Launching HMIR Dashboard...");
            commands::start::launch_dashboard(port).await;
        }
        Commands::Integrations { port, model } => {
            commands::integrations::print_integrations(port, model.as_deref());
        }
        Commands::Logs {
            tail,
            grep,
            follow,
            dir,
        } => {
            commands::logs::run_logs(tail, grep.as_deref(), follow, dir);
        }
        Commands::Clean => {
            commands::clean::run_clean().await;
        }
        Commands::Uninstall => {
            println!("🗑️  HMIR ELITE | COMMENCING FULL SYSTEM UNINSTALL");
            stop_all_instances();

            println!("  [1/2] Purging application data...");
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
            let hmir_dir = std::path::Path::new(&home).join(".hmir");

            if hmir_dir.exists() {
                // Try standard removal first
                if let Err(_) = std::fs::remove_dir_all(&hmir_dir) {
                    // If failed (locks), try the rename-to-delete strategy for sub-binaries
                    println!("  ⚠️  Standard purge blocked. Attempting deep purge...");
                    
                    // Small delay to allow OS to catch up with taskkill
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    
                    let _ = purge_directory_robust(&hmir_dir);
                    
                    if hmir_dir.exists() {
                        println!("  ⚠️  Partial purge: Access is denied. Manual removal of {} may be required after a reboot.", hmir_dir.display());
                    } else {
                        println!("  ✅ Deep purge successful.");
                    }
                } else {
                    println!("  ✅ Runtime directory purged.");
                }
            }

            println!("  [2/2] Cleaning binary environment...");
            println!("  💡 HMIR executable and PATH entries should be removed manually or via uninstall.ps1.");
            println!("\n✨ HMIR ELITE has been uninstalled.");
        }
    }
}

fn stop_all_instances() {
    println!("🛑 HMIR ELITE | TERMINATING ALL COMPUTE INSTANCES");
    if cfg!(target_os = "windows") {
        println!("  [1/3] Closing Inference API...");
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "hmir-api.exe", "/T"])
            .output();

        println!("  [2/3] Closing Hardware Dashboard...");
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "hmir-dashboard.exe", "/T"])
            .output();

        println!("  [3/3] Deactivating NPU Bridges...");
        // Surgical kill using PowerShell to avoid killing unrelated python processes
        let _ = std::process::Command::new("powershell")
            .args(["-Command", "Get-Process python -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like '*hmir_npu_service.py*' } | Stop-Process -Force"])
            .output();
        
        // Give the OS time to release file handles
        std::thread::sleep(std::time::Duration::from_millis(1000));
    } else {
        println!("  [1/3] Closing Inference API...");
        let _ = std::process::Command::new("pkill")
            .args(["-f", "hmir-api"])
            .output();

        println!("  [2/3] Closing Hardware Dashboard...");
        let _ = std::process::Command::new("pkill")
            .args(["-f", "hmir-dashboard"])
            .output();

        println!("  [3/3] Deactivating NPU Bridges...");
        let _ = std::process::Command::new("pkill")
            .args(["-f", "hmir_npu_service.py"])
            .output();
        
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Robustly purge a directory by renaming locked files before deletion
fn purge_directory_robust(path: &std::path::Path) -> std::io::Result<()> {
    if !path.exists() { return Ok(()); }

    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let _ = purge_directory_robust(&entry.path());
        }
        let _ = std::fs::remove_dir(path);
    } else {
        // Attempt direct delete
        if let Err(_) = std::fs::remove_file(path) {
            // Rename locked file to .old and try again or just leave it for next reboot
            let old_path = path.with_extension(format!("{}.old", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            if let Ok(_) = std::fs::rename(path, &old_path) {
                let _ = std::fs::remove_file(old_path);
            }
        }
    }
    Ok(())
}
