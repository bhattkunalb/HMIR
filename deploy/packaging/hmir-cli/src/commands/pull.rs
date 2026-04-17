use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::PathBuf;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;

pub async fn pull_model(model_name: &str) {
    let url = match model_name {
        "qwen2.5-1.5b-ov" => "https://huggingface.co/OpenVINO/qwen2.5-1.5b-instruct-int4-ov/resolve/main/openvino_model.bin",
        "llama3-8b-ov" => "https://huggingface.co/bartowski/Meta-Llama-3-8B-Instruct-GGUF/resolve/main/Meta-Llama-3-8B-Instruct-Q4_K_M.gguf",
        "llama3-8b-cuda" => "https://huggingface.co/bartowski/Meta-Llama-3-8B-Instruct-GGUF/resolve/main/Meta-Llama-3-8B-Instruct-Q4_K_M.gguf",
        "llama3-8b-dml" => "https://huggingface.co/bartowski/Meta-Llama-3-8B-Instruct-GGUF/resolve/main/Meta-Llama-3-8B-Instruct-Q4_K_S.gguf",
        "llama3-8b-gguf" => "https://huggingface.co/bartowski/Meta-Llama-3-8B-Instruct-GGUF/resolve/main/Meta-Llama-3-8B-Instruct-Q4_K_S.gguf",
        "phi3-mini" => "https://huggingface.co/bartowski/Phi-3-mini-4k-instruct-GGUF/resolve/main/Phi-3-mini-4k-instruct-Q4_K_M.gguf",
        "llama3.2-3b" => "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        _ => {
            if model_name.starts_with("http") {
                model_name
            } else {
                println!("❌ Unknown model alias: {}. Try 'llama3-8b-ov'", model_name);
                return;
            }
        }
    };

    let client = Client::new();
    let res = match client.get(url).send().await {
        Ok(res) => res,
        Err(e) => {
            println!("❌ Failed to reach registry: {}", e);
            return;
        }
    };

    let total_size = res.content_length().unwrap_or(0);
    
    // Setup local path in %LOCALAPPDATA%/hmir/models
    let mut dest_path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    dest_path.push("hmir");
    dest_path.push("models");
    
    if let Err(e) = fs::create_dir_all(&dest_path).await {
        println!("❌ Failed to create models directory: {}", e);
        return;
    }

    let filename = url.split('/').last().unwrap_or("model.bin");
    dest_path.push(filename);

    println!("📥 Downloading {} to {}", model_name, dest_path.display());

    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .unwrap()
        .progress_chars("#>-"));

    let mut file = File::create(&dest_path).await.unwrap();
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        file.write_all(&chunk).await.unwrap();
        let new = std::cmp::min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(new);
    }

    pb.finish_with_message("Download complete");
    println!("\n✅ Successfully pulled {}!", model_name);
    println!("📍 Location: {}", dest_path.display());
}

// Need 'dirs' crate for cross-platform data paths
mod dirs {
    use std::path::PathBuf;
    pub fn data_local_dir() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        }
        #[cfg(not(target_os = "windows"))]
        {
            None // Extend for Linux/Mac if needed
        }
    }
}
