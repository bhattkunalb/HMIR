use clap::{Parser, Subcommand};
// cSpell:ignore USERPROFILE, WINDOWTITLE
mod commands;

#[derive(Parser)]
#[command(name = "hmir")]
#[command(about = "HMIR: Heterogeneous Model Inference Runtime", long_about = None)]
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
    Start {
        /// The port to listen on for the API
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Launch the native telemetry dashboard
        #[arg(short, long)]
        dashboard: bool,
        /// The model to load on startup
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Stop all running HMIR instances
    Stop,
    /// Launch the native telemetry dashboard directly
    Dashboard {
        /// The port to connect to the API on
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
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
            dashboard,
            model,
        } => {
            println!("🚀 Launching HMIR ELITE Compute Hub");
            commands::start::start_daemon(port, dashboard, model).await;
        }
        Commands::Stop => {
            stop_all_instances();
            println!("✅ HMIR ELITE specialized resources released.");
        }
        Commands::Dashboard { port } => {
            println!("🖥️  Launching HMIR ELITE Dashboard...");
            // We reuse the start_daemon logic but specify dashboard=true
            // Optimization: if API is already running, start_daemon should handle it (it does via health checks)
            commands::start::start_daemon(port, true, None).await;
        }
        Commands::Uninstall => {
            println!("🗑️  HMIR ELITE | COMMENCING FULL SYSTEM UNINSTALL");
            stop_all_instances();

            println!("  [1/2] Purging application data...");
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
            let hmir_dir = std::path::Path::new(&home).join(".hmir");

            if hmir_dir.exists() {
                match std::fs::remove_dir_all(&hmir_dir) {
                    Ok(_) => println!("  ✅ Runtime directory purged."),
                    Err(e) => println!(
                        "  ⚠️  Partial purge: {}. Manual removal of {} may be required.",
                        e,
                        hmir_dir.display()
                    ),
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
    println!("  [1/3] Closing Inference API...");
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "hmir-api.exe", "/T"])
        .output();

    println!("  [2/3] Closing Hardware Dashboard...");
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "hmir-dashboard.exe", "/T"])
        .output();

    println!("  [3/3] Deactivating NPU Bridges...");
    // Kill any python processes matching the worker pattern
    let _ = std::process::Command::new("taskkill")
        .args([
            "/F",
            "/IM",
            "python.exe",
            "/FI",
            "WINDOWTITLE eq HMIR_NPU_BRIDGE*",
        ])
        .output();
}
