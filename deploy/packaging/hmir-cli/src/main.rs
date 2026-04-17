use clap::{Parser, Subcommand};
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
        Commands::Start { port, dashboard, model } => {
            println!("🚀 Launching HMIR Inference Node");
            commands::start::start_daemon(port, dashboard, model).await;
        }
    }
}
