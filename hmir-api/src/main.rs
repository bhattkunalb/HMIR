use axum::{
    extract::State,
    response::{
        sse::{Event, Sse},
        Html,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};

lazy_static::lazy_static! {
    static ref LOG_HISTORY: Mutex<VecDeque<String>> = Mutex::new(VecDeque::with_capacity(100));
    static ref LOG_BUS: broadcast::Sender<String> = {
        let (tx, _) = broadcast::channel(1024);
        tx
    };
}
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub name: String,
}

#[derive(Clone)]
pub struct AppState {
    pub active_model: Arc<Mutex<String>>,
    pub engine_status: Arc<Mutex<String>>,
    pub telemetry: Arc<hmir_core::telemetry::TelemetrySink>,
    pub log_bus: broadcast::Sender<String>,
    pub start_time: std::time::Instant,
    pub client: reqwest::Client,
}

#[derive(Deserialize)]
pub struct SwitchModelPayload {
    pub name: String,
}

fn data_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hmir")
}

fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

fn models_dir() -> PathBuf {
    data_root().join("models")
}

fn resolve_script_path(script_name: &str) -> PathBuf {
    let candidates = [
        std::env::current_dir()
            .unwrap_or_default()
            .join("scripts")
            .join(script_name),
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("scripts")
            .join(script_name),
    ];

    candidates
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(script_name))
}

fn resolve_python_command() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|mut path| {
            path.pop();
            Some(path)
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let candidates = [
        cwd.join(".venv").join("Scripts").join("python.exe"),
        cwd.join(".venv").join("bin").join("python"),
        exe_dir.join(".venv").join("Scripts").join("python.exe"),
        exe_dir.join(".venv").join("bin").join("python"),
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

fn log_event(msg: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let formatted = format!("[{}] {}", timestamp, msg);

    // 1. History Buffer
    if let Ok(mut history) = LOG_HISTORY.lock() {
        if history.len() >= 100 {
            history.pop_front();
        }
        history.push_back(formatted.clone());
    }

    // 2. Real-time Bus
    let _ = LOG_BUS.send(formatted.clone());

    // 3. Persistent Log
    let mut path = logs_dir();
    let _ = std::fs::create_dir_all(&path);
    path.push("api.log");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", formatted);
    }
}

pub async fn list_installed_models() -> Json<Vec<String>> {
    let mut models = Vec::new();
    let path = models_dir();

    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                // Check for OpenVINO directories
                if let Some(name) = entry.file_name().to_str() {
                    models.push(name.to_string());
                }
                continue;
            }
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                let ext_l = ext.to_lowercase();
                if ext_l == "gguf" || ext_l == "ov" || ext_l == "bin" {
                    if let Some(name) = entry.file_name().to_str() {
                        models.push(name.to_string());
                    }
                }
            }
        }
    }
    Json(models)
}

pub async fn switch_model(
    State(state): State<AppState>,
    Json(payload): Json<SwitchModelPayload>,
) -> Json<serde_json::Value> {
    let engine = if payload.name.to_lowercase().contains("ov")
        || payload.name.to_lowercase().contains("openvino")
    {
        "OPENVINO/NPU"
    } else {
        "LLAMA.CPP/CUDA"
    };

    {
        let mut active = state.active_model.lock().unwrap();
        let mut status = state.engine_status.lock().unwrap();
        *active = payload.name.clone();
        *status = "Mounting".to_string();
    }

    log_event(&format!("MOUNTING ENGINE: {} ({})", payload.name, engine));

    let mut final_status = "Mounted".to_string();

    // If it's NPU, notify the worker
    if engine == "OPENVINO/NPU" {
        let client = reqwest::Client::new();
        match client
            .post("http://127.0.0.1:8089/v1/engine/load")
            .json(&serde_json::json!({ "name": payload.name }))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                log_event(&format!("[SUCCESS] {} MOUNTED ON {}", payload.name, engine));
                final_status = "Mounted".to_string();
            }
            Ok(resp) => {
                let err = resp.text().await.unwrap_or_default();
                log_event(&format!("[ERROR] NPU Worker failed to load model: {}", err));
                final_status = "Failed".to_string();
            }
            Err(e) => {
                log_event(&format!("[ERROR] Could not connect to NPU Worker: {}", e));
                final_status = "Worker Offline".to_string();
            }
        }
    } else {
        log_event(&format!("[SUCCESS] {} MOUNTED ON {}", payload.name, engine));
    }

    {
        let mut status = state.engine_status.lock().unwrap();
        *status = final_status.clone();
    }

    let _ = state
        .telemetry
        .emit(hmir_core::telemetry::TelemetryEvent::ModelMounted {
            name: payload.name.clone(),
            engine: engine.to_string(),
        });

    let active = state.active_model.lock().unwrap().clone();
    Json(serde_json::json!({ "status": final_status, "active": active, "engine": engine }))
}

pub async fn eject_model(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut active = state.active_model.lock().unwrap();
    let mut status = state.engine_status.lock().unwrap();
    let prev = active.clone();
    *active = "None".to_string();
    *status = "Unmounted".to_string();

    log_event(&format!("UNMOUNTING ENGINE: {}", prev));

    let _ = state
        .telemetry
        .emit(hmir_core::telemetry::TelemetryEvent::ModelMounted {
            name: "None".to_string(),
            engine: "None".to_string(),
        });

    Json(serde_json::json!({ "status": "ejected", "previous": prev }))
}

#[derive(Deserialize)]
pub struct DownloadModelPayload {
    pub repo_id: String,
    pub folder_name: String,
}

pub async fn download_model(
    State(state): State<AppState>,
    Json(payload): Json<DownloadModelPayload>,
) -> Json<serde_json::Value> {
    log_event(&format!(
        "STARTING DOWNLOAD: {} into {}",
        payload.repo_id, payload.folder_name
    ));

    let tel_clone = state.telemetry.clone();
    let repo = payload.repo_id.clone();
    let folder = payload.folder_name.clone();

    tokio::spawn(async move {
        let _ = tel_clone.emit(hmir_core::telemetry::TelemetryEvent::DownloadStatus {
            model: repo.clone(),
            status: "Starting".to_string(),
            progress: 0.0,
        });

        let script_path = resolve_script_path("download_npu_model.py");
        let python_bin = resolve_python_command();

        let mut child = tokio::process::Command::new(python_bin)
            .arg(script_path)
            .arg(&repo)
            .arg(&folder)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn downloader");

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        loop {
            tokio::select! {
                line = stdout_reader.next_line() => {
                    if let Ok(Some(l)) = line {
                        log_event(&format!("[DL-STDOUT] {}", l));
                    } else { break; }
                }
                line = stderr_reader.next_line() => {
                    if let Ok(Some(l)) = line {
                        log_event(&format!("[DL-STDERR] {}", l));
                    }
                }
            }
        }

        let status = child.wait().await;
        match status {
            Ok(s) if s.success() => {
                let _ = tel_clone.emit(hmir_core::telemetry::TelemetryEvent::DownloadStatus {
                    model: repo,
                    status: "Completed".to_string(),
                    progress: 100.0,
                });
            }
            _ => {
                let _ = tel_clone.emit(hmir_core::telemetry::TelemetryEvent::DownloadStatus {
                    model: repo,
                    status: "Failed".to_string(),
                    progress: 0.0,
                });
            }
        }
    });

    Json(serde_json::json!({ "status": "initiated", "model": payload.repo_id }))
}

async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    log_event("INCOMING CHAT REQUEST -> NPU PROXY");
    log_event(&format!(
        "  Messages: {} | Stream: {:?}",
        req.messages.len(),
        req.stream
    ));

    // Standard NPU Worker port is 8089 (defined in scripts/hmir_npu_service.py)
    let url = "http://127.0.0.1:8089/v1/chat/completions";
    let (tx, rx) = mpsc::channel(128);

    // Pre-flight: check if NPU worker is reachable and READY
    let health_resp = state
        .client
        .get("http://127.0.0.1:8089/health")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    match health_resp {
        Ok(resp) => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if body["status"] == "READY" {
                    log_event("  NPU Worker health: [ONLINE/READY]");
                } else {
                    log_event(&format!("  NPU Worker health: [{}] (Loading model...)", body["status"]));
                }
            } else {
                log_event("  NPU Worker health: [ONLINE] (Unknown status body)");
            }
        }
        Err(e) => {
            log_event(&format!("[WARN] NPU WORKER UNREACHABLE on :8089 - {}", e));
            log_event("  Hint: Run 'python scripts/hmir_npu_service.py' or restart with 'hmir start'");
        }
    }

    tokio::spawn(async move {
        log_event(&format!("  Proxying POST -> {}", url));
        let resp = match state
            .client
            .post(url)
            .json(&req)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
        {
            Ok(r) => {
                log_event(&format!("  NPU Worker responded: HTTP {}", r.status()));
                r
            }
            Err(e) => {
                let detail = if e.is_connect() {
                    "CONNECTION REFUSED on :8089 — NPU Worker is NOT running. Launch it with: python scripts/hmir_npu_service.py".to_string()
                } else if e.is_timeout() {
                    "TIMEOUT — NPU Worker did not respond within 120s. Model may be loading or frozen.".to_string()
                } else {
                    format!("PROXY ERROR — {}", e)
                };
                log_event(&format!("[ERROR] NPU PROXY FAILURE: {}", detail));
                let err_payload = format!(r#"{{"error": "{}"}}"#, detail.replace('"', "'"));
                let _ = tx.send(Ok(Event::default().data(err_payload))).await;
                return;
            }
        };

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            log_event(&format!("[ERROR] NPU Worker returned HTTP {}: {}", status, body));
            let _ = tx
                .send(Ok(Event::default().data(format!(
                    r#"{{"error": "NPU returned HTTP {}: {}"}}"#,
                    status,
                    body.replace('"', "'")
                ))))
                .await;
            return;
        }

        let mut stream = resp.bytes_stream();
        use futures::StreamExt;
        let mut token_count: usize = 0;
        let mut buffer = String::new();

        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    let chunk_text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&chunk_text);

                    while let Some(line_end) = buffer.find("\n\n") {
                        let full_event = buffer[..line_end].to_string();
                        buffer = buffer[line_end + 2..].to_string();

                        for line in full_event.lines() {
                            if line.starts_with("data: ") {
                                let data = line.strip_prefix("data: ").unwrap_or("").trim();
                                if !data.is_empty() {
                                    // Safety: Remove any actual newlines that would cause axum to panic
                                    let sanitized_data = data.replace('\n', " ").replace('\r', " ");
                                    token_count += 1;
                                    let _ = tx.send(Ok(Event::default().data(sanitized_data))).await;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log_event(&format!(
                        "[WARN] Stream interrupted after {} tokens: {}",
                        token_count, e
                    ));
                    break;
                }
            }
        }
        log_event(&format!(
            "[DONE] Chat completed - {} tokens streamed",
            token_count
        ));
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::new())
}

#[derive(Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<serde_json::Value>,
    pub temperature: Option<f32>,
    pub stream: Option<bool>,
}

async fn telemetry_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.telemetry.subscribe();
    let stream = async_stream::stream! {
        while let Ok(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().data(data));
            }
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new())
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "healthy", "engine": "NPU-Proxy-Active"}))
}

async fn serve_web_ui() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn log_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.log_bus.subscribe();

    // Stream history first
    let history = {
        let h = LOG_HISTORY.lock().unwrap();
        h.clone()
    };

    let stream = async_stream::stream! {
        for line in history {
            yield Ok(Event::default().data(line));
        }
        loop {
            if let Ok(msg) = rx.recv().await {
                yield Ok(Event::default().data(msg));
            }
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("HMIR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let telemetry = hmir_core::telemetry::TelemetrySink::new(1024);
    let telemetry_arc = std::sync::Arc::new(telemetry);
    let active_model = Arc::new(Mutex::new("None".to_string()));
    let engine_status = Arc::new(Mutex::new("Unmounted".to_string()));

    let state = AppState {
        active_model: active_model.clone(),
        engine_status: engine_status.clone(),
        telemetry: telemetry_arc.clone(),
        log_bus: LOG_BUS.clone(),
        start_time: std::time::Instant::now(),
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default(),
    };

    log_event(&format!("HMIR API v2.0.0 STARTING (Port {})...", port));

    // -- Auto-spawn NPU Worker --
    log_event("[BOOT] Launching NPU Inference Worker (port 8089)...");
    let worker_script = resolve_script_path("hmir_npu_service.py");

    // Resolve the project root for setting working directory on spawned processes
    let project_root = std::env::current_dir().unwrap_or_default();

    // Resolve model path to absolute path in LOCALAPPDATA
    let model_path = {
        let default_name = "qwen2.5-1.5b-ov";
        let local_models = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hmir")
            .join("models");
        let candidate = local_models.join(default_name);
        if candidate.exists() {
            candidate.to_string_lossy().to_string()
        } else {
            default_name.to_string()
        }
    };
    log_event(&format!("  Model path: {}", model_path));

    if worker_script.exists() {
        let python_bin = resolve_python_command();
        log_event(&format!("  Using Python runtime: {}", python_bin));
        log_event(&format!("  Worker script: {}", worker_script.display()));
        log_event(&format!("  Working directory: {}", project_root.display()));

        match std::process::Command::new(&python_bin)
            .arg("-u")
            .arg(&worker_script)
            .current_dir(&project_root)
            .env("HMIR_MODEL_PATH", &model_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                log_event(&format!("  NPU Worker spawned (PID: {})", child.id()));
                // Stream worker stdout/stderr into our log bus
                let child_stderr = child.stderr.take();
                let child_stdout = child.stdout.take();
                tokio::spawn(async move {
                    if let Some(stdout) = child_stdout {
                        let mut reader =
                            BufReader::new(tokio::process::ChildStdout::from_std(stdout).unwrap())
                                .lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            log_event(&format!("[NPU-OUT] {}", line));
                        }
                    }
                });
                tokio::spawn(async move {
                    if let Some(stderr) = child_stderr {
                        let mut reader =
                            BufReader::new(tokio::process::ChildStderr::from_std(stderr).unwrap())
                                .lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            log_event(&format!("[NPU-ERR] {}", line));
                        }
                    }
                });

                // Wait for worker to come online (up to 90s for NPU compilation)
                log_event("  Waiting for NPU Worker to bind on :8089 (may take 60-90s for NPU model compilation)...");
                let client = reqwest::Client::new();
                let mut worker_online = false;
                for attempt in 1..=45 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    if let Ok(resp) = client
                        .get("http://127.0.0.1:8089/health")
                        .timeout(std::time::Duration::from_secs(2))
                        .send()
                        .await
                    {
                        if resp.status().is_success() {
                            if let Ok(body) = resp.json::<serde_json::Value>().await {
                                if body["status"] == "READY" {
                                    log_event(&format!("  [ONLINE] NPU Worker READY after {}s", attempt * 2));
                                    worker_online = true;
                                    break;
                                }
                            }
                        }
                    }
                    if attempt % 10 == 0 {
                        log_event(&format!("  Still waiting... ({}s elapsed)", attempt * 2));
                    }
                }
                if !worker_online {
                    log_event("  [WARN] NPU Worker did not respond within 90s. Chat will fail until worker is ready.");
                }

                // -- Watchdog Task --
                let python_bin_watch = python_bin.clone();
                let worker_script_watch = worker_script.clone();
                let project_root_watch = project_root.clone();
                let model_path_watch = model_path.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                        match client
                            .get("http://127.0.0.1:8089/health")
                            .timeout(std::time::Duration::from_secs(5))
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                // Worker is healthy
                            }
                            _ => {
                                log_event("⚠️ [WATCHDOG] NPU Worker unresponsive. Attempting RESTART...");
                                // Try to spawn again
                                let _ = std::process::Command::new(&python_bin_watch)
                                    .arg("-u")
                                    .arg(&worker_script_watch)
                                    .current_dir(&project_root_watch)
                                    .env("HMIR_MODEL_PATH", &model_path_watch)
                                    .stdout(Stdio::inherit())
                                    .stderr(Stdio::inherit())
                                    .spawn();
                                
                                // Wait a bit for it to come up
                                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                            }
                        }
                    }
                });
            }
            Err(e) => {
                log_event(&format!("  [WARN] Failed to spawn NPU Worker: {}", e));
                log_event("  Chat functionality will be unavailable.");
            }
        }
    } else {
        log_event(&format!(
            "  [WARN] Worker script not found: {}",
            worker_script.display()
        ));
        log_event("  Chat functionality will be unavailable.");
    }

    // Background Hardware Polling
    let tel_clone = telemetry_arc.clone();
    let start_time_copy = state.start_time;
    let status_ref = state.engine_status.clone();
    tokio::spawn(async move {
        loop {
            let hw = hmir_hardware_prober::os_polling::poll_hardware().await;
            let ram_gb = hw.ram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let vram_gb = hw.vram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let current_status = status_ref.lock().unwrap().clone();

            let _ = tel_clone.emit(hmir_core::telemetry::TelemetryEvent::HardwareState {
                cpu_util: hw.cpu_util_pct,
                gpu_util: hw.gpu_util_pct,
                npu_util: hw.npu_util_pct,
                cpu_temp: hw.cpu_temp_c,
                gpu_temp: hw.gpu_temp_c,
                vram_used: vram_gb,
                vram_total: hw.vram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                gpu_vram_dedicated: hw.gpu_vram_dedicated_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                gpu_vram_shared: hw.gpu_vram_shared_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                npu_vram_used: hw.npu_vram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                ram_used: ram_gb,
                ram_total: hw.ram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                tps: 0.0,
                power_w: hw.power_draw_watts,
                node_uptime: start_time_copy.elapsed().as_secs(),
                kv_cache: 0.0,
                cpu_name: hw.cpu_name.clone(),
                cpu_cores: hw.cpu_cores,
                cpu_threads: hw.cpu_threads,
                cpu_l3_cache_mb: hw.cpu_l3_cache_mb,
                gpu_name: hw.gpu_name.clone(),
                gpu_driver: hw.gpu_driver.clone(),
                npu_name: hw.npu_name.clone(),
                npu_driver: hw.npu_driver.clone(),
                disk_free: hw.disk_free_gb,
                disk_total: hw.disk_total_gb,
                disk_model: hw.disk_model.clone(),
                ram_speed_mts: hw.ram_speed_mts,
                engine_status: current_status,
            });
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
        }
    });

    // Unified Router (API + Web UI)
    let app = Router::new()
        .route("/", get(serve_web_ui))
        .route("/v1/models/installed", get(list_installed_models))
        .route("/v1/models/download", post(download_model))
        .route("/v1/engine/switch", post(switch_model))
        .route("/v1/engine/eject", post(eject_model))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/telemetry", get(telemetry_stream))
        .route("/v1/logs", get(log_stream))
        .route("/v1/health", get(health_check))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("[ERROR] Port {} is already in use.", port);
            eprintln!("   Try running 'hmir stop' first, or use a different port with '--port'.");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("[ERROR] Failed to bind to port {}: {}", port, e);
            std::process::exit(1);
        }
    };
    println!("-- HMIR Elite Unified Node: http://127.0.0.1:{}", port);
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[ERROR] Server error: {}", e);
    }
}
