use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadSpec<'a> {
    DirectUrl { url: &'a str },
    Snapshot { repo_id: &'a str, folder_name: &'a str },
}

pub async fn pull_model(model_name: &str) {
    let spec = match resolve_download_spec(model_name) {
        Some(spec) => spec,
        None => {
            println!(
                "❌ Unknown model alias: {}. Try 'qwen2.5-1.5b-ov' or 'llama3.2-3b'.",
                model_name
            );
            return;
        }
    };

    match spec {
        DownloadSpec::DirectUrl { url } => {
            if let Err(e) = download_direct(url, model_name).await {
                println!("❌ {}", e);
            }
        }
        DownloadSpec::Snapshot {
            repo_id,
            folder_name,
        } => {
            if let Err(e) = download_snapshot(repo_id, folder_name).await {
                println!("❌ {}", e);
            }
        }
    }
}

fn resolve_download_spec(model_name: &str) -> Option<DownloadSpec<'_>> {
    let spec = match model_name {
        "qwen2.5-1.5b-ov" => DownloadSpec::Snapshot {
            repo_id: "OpenVINO/qwen2.5-1.5b-instruct-int4-ov",
            folder_name: "qwen2.5-1.5b-ov",
        },
        "phi3-mini-ov" => DownloadSpec::Snapshot {
            repo_id: "OpenVINO/Phi-3-mini-4k-instruct-int4-ov",
            folder_name: "phi3-mini-ov",
        },
        "llama3-8b-gguf" | "llama3-8b-cuda" | "llama3-8b-dml" => DownloadSpec::DirectUrl {
            url: "https://huggingface.co/bartowski/Meta-Llama-3-8B-Instruct-GGUF/resolve/main/Meta-Llama-3-8B-Instruct-Q4_K_M.gguf",
        },
        "phi3-mini" => DownloadSpec::DirectUrl {
            url: "https://huggingface.co/bartowski/Phi-3-mini-4k-instruct-GGUF/resolve/main/Phi-3-mini-4k-instruct-Q4_K_M.gguf",
        },
        "llama3.2-3b" => DownloadSpec::DirectUrl {
            url: "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        },
        _ if model_name.starts_with("http") => DownloadSpec::DirectUrl { url: model_name },
        _ => return None,
    };

    Some(spec)
}

async fn download_snapshot(repo_id: &str, folder_name: &str) -> Result<(), String> {
    let script_path = resolve_script_path("download_npu_model.py")
        .ok_or_else(|| "Unable to locate scripts/download_npu_model.py".to_string())?;

    let python = resolve_python_command();

    println!(
        "📥 Downloading {} into local model store as {}",
        repo_id, folder_name
    );

    let status = Command::new(&python)
        .arg(script_path)
        .arg(repo_id)
        .arg(folder_name)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to launch {}: {}", python, e))?;

    if !status.success() {
        return Err(format!(
            "Snapshot download failed for {} with exit code {:?}",
            repo_id,
            status.code()
        ));
    }

    println!("✅ Successfully pulled {}!", folder_name);
    Ok(())
}

async fn download_direct(url: &str, model_name: &str) -> Result<(), String> {
    let client = Client::new();
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to reach registry: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("Registry returned HTTP {}", res.status()));
    }

    let total_size = res.content_length().unwrap_or(0);

    let mut dest_path = data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    dest_path.push("hmir");
    dest_path.push("models");

    fs::create_dir_all(&dest_path)
        .await
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let filename = url.split('/').next_back().unwrap_or("model.bin");
    dest_path.push(filename);

    println!("📥 Downloading {} to {}", model_name, dest_path.display());

    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .unwrap()
        .progress_chars("#>-"));

    let mut file = File::create(&dest_path)
        .await
        .map_err(|e| format!("Failed to create destination file: {}", e))?;
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("Download stream error: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write model chunk: {}", e))?;
        let new = std::cmp::min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(new);
    }

    pb.finish_with_message("Download complete");
    println!("\n✅ Successfully pulled {}!", model_name);
    println!("📍 Location: {}", dest_path.display());
    Ok(())
}

fn data_local_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local").join("share"))
            })
    }
}

fn resolve_python_command() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "python"
    }

    #[cfg(not(target_os = "windows"))]
    {
        "python3"
    }
}

fn resolve_script_path(script_name: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().map(|mut path| {
        path.pop();
        path
    });

    let candidates = [
        exe_dir.as_ref().map(|dir| dir.join("scripts").join(script_name)),
        std::env::current_dir()
            .ok()
            .map(|dir| dir.join("scripts").join(script_name)),
    ];

    candidates.into_iter().flatten().find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::{resolve_download_spec, DownloadSpec};

    #[test]
    fn resolves_openvino_snapshot_aliases() {
        assert_eq!(
            resolve_download_spec("qwen2.5-1.5b-ov"),
            Some(DownloadSpec::Snapshot {
                repo_id: "OpenVINO/qwen2.5-1.5b-instruct-int4-ov",
                folder_name: "qwen2.5-1.5b-ov",
            })
        );

        assert_eq!(
            resolve_download_spec("phi3-mini-ov"),
            Some(DownloadSpec::Snapshot {
                repo_id: "OpenVINO/Phi-3-mini-4k-instruct-int4-ov",
                folder_name: "phi3-mini-ov",
            })
        );
    }

    #[test]
    fn resolves_gguf_aliases() {
        assert_eq!(
            resolve_download_spec("llama3.2-3b"),
            Some(DownloadSpec::DirectUrl {
                url: "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
            })
        );
    }
}
