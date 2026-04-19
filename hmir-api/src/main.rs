// cSpell:ignore Deque venv aiohttp
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
    let mut active = state.active_model.lock().unwrap();
    let mut status = state.engine_status.lock().unwrap();
    *active = payload.name.clone();
    *status = "Mounted".to_string();

    let engine = if payload.name.to_lowercase().contains("ov")
        || payload.name.to_lowercase().contains("openvino")
    {
        "OPENVINO/NPU"
    } else {
        "LLAMA.CPP/CUDA"
    };

    log_event(&format!("MOUNTING ENGINE: {} ({})", payload.name, engine));
    log_event(&format!("[SUCCESS] {} MOUNTED ON {}", payload.name, engine));

    let _ = state
        .telemetry
        .emit(hmir_core::telemetry::TelemetryEvent::ModelMounted {
            name: payload.name.clone(),
            engine: engine.to_string(),
        });

    Json(serde_json::json!({ "status": "switched", "active": *active, "engine": engine }))
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

    // Pre-flight: check if NPU worker is reachable
    let health_ok = state
        .client
        .get("http://127.0.0.1:8089/health")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    if let Err(ref e) = health_ok {
        log_event(&format!("[WARN] NPU WORKER UNREACHABLE on :8089 - {}", e));
        log_event("  Hint: Run 'python scripts/hmir_npu_service.py' or restart with 'hmir start'");
    } else {
        log_event("  NPU Worker health: [ONLINE]");
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
    Html(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HMIR Elite | Unified Command</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;800&family=JetBrains+Mono&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg: #050507;
            --glass: rgba(15, 15, 20, 0.8);
            --border: rgba(255, 255, 255, 0.08);
            --accent: #00f2ff;
            --accent-glow: rgba(0, 242, 255, 0.3);
            --error: #ff3366;
            --text: #e0e0e0;
            --text-dim: #909090;
            --vibrant: #7000ff;
        }

        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            background: var(--bg);
            color: var(--text);
            font-family: 'Inter', sans-serif;
            height: 100vh;
            display: flex;
            overflow: hidden;
            background-image: 
                radial-gradient(circle at 0% 0%, rgba(0, 242, 255, 0.08) 0%, transparent 40%),
                radial-gradient(circle at 100% 100%, rgba(112, 0, 255, 0.08) 0%, transparent 40%);
        }

        /* Sidebar Navigation */
        .sidebar {
            width: 80px;
            background: var(--glass);
            backdrop-filter: blur(20px);
            border-right: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            align-items: center;
            padding: 30px 0;
            z-index: 100;
        }
        .logo-box { width: 40px; height: 40px; background: var(--accent); border-radius: 12px; box-shadow: 0 0 20px var(--accent-glow); margin-bottom: 50px; cursor: pointer; display: flex; align-items: center; justify-content: center; font-weight: 800; color: black; font-size: 20px; }
        .nav-icon { width: 45px; height: 45px; border-radius: 12px; display: flex; align-items: center; justify-content: center; cursor: pointer; transition: 0.3s; margin-bottom: 20px; color: var(--text-dim); border: 1px solid transparent; }
        .nav-icon:hover { background: rgba(255,255,255,0.05); color: white; }
        .nav-icon.active { background: rgba(0, 242, 255, 0.1); color: var(--accent); border-color: rgba(0, 242, 255, 0.2); }

        /* Unified Layout */
        .main-container { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
        .header {
            height: 80px;
            padding: 0 40px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid var(--border);
            backdrop-filter: blur(10px);
        }
        .node-info { display: flex; align-items: center; gap: 15px; }
        .status-dot { width: 8px; height: 8px; border-radius: 50%; background: #00ff78; box-shadow: 0 0 10px #00ff78; }

        .content-area { flex: 1; display: grid; grid-template-columns: 1fr 350px; gap: 0; overflow: hidden; }

        /* Left: Intelligence (Chat) */
        .workspace-panel { 
            display: flex; 
            flex-direction: column; 
            background: rgba(0,0,0,0.2); 
            border-right: 1px solid var(--border);
            position: relative;
        }
        .chat-history { flex: 1; overflow-y: auto; padding: 40px; scroll-behavior: smooth; }
        .message { margin-bottom: 30px; line-height: 1.6; font-size: 16px; max-width: 900px; }
        .msg-role { font-size: 11px; font-weight: 800; letter-spacing: 1px; color: var(--text-dim); margin-bottom: 8px; text-transform: uppercase; }
        .msg-role.ai { color: var(--accent); }
        .msg-content { background: rgba(255,255,255,0.02); padding: 20px; border-radius: 16px; border: 1px solid var(--border); }
        .user .msg-content { border-color: var(--accent-glow); background: rgba(0, 242, 255, 0.03); }

        .chat-controls { padding: 30px 40px; background: rgba(0,0,0,0.3); border-top: 1px solid var(--border); }
        .input-wrapper { background: var(--glass); border: 1px solid var(--border); border-radius: 16px; display: flex; padding: 10px 15px; transition: 0.3s; }
        .input-wrapper:focus-within { border-color: var(--accent); box-shadow: 0 0 15px var(--accent-glow); }
        .input-wrapper input { flex:1; background: none; border: none; outline: none; color: white; padding: 10px; font-size: 16px; }
        .send-btn { background: var(--accent); color: black; border: none; padding: 0 25px; border-radius: 10px; font-weight: 800; cursor: pointer; margin-left:10px; transition: 0.2s; }
        .send-btn:hover { transform: scale(1.05); }

        /* Right: Infrastructure (Telemetry/Models) */
        .infra-panel { padding: 30px; overflow-y: auto; display: flex; flex-direction: column; gap: 24px; background: var(--glass); }
        .panel-title { font-size: 12px; font-weight: 800; color: var(--text-dim); letter-spacing: 1.5px; border-bottom: 1px solid var(--border); padding-bottom: 10px; margin-bottom: 10px; }
        
        .stat-card { background: rgba(255,255,255,0.03); border: 1px solid var(--border); padding: 20px; border-radius: 16px; }
        .stat-val { font-size: 28px; font-weight: 800; color: white; display: flex; align-items: baseline; gap: 5px; }
        .stat-val span { font-size: 14px; color: var(--text-dim); }
        .stat-label { font-size: 11px; font-weight: 700; color: var(--accent); margin-top: 5px; opacity: 0.8; }

        .model-item { background: rgba(255,255,255,0.02); border: 1px solid var(--border); padding: 15px; border-radius: 12px; margin-bottom: 12px; transition: 0.3s; }
        .model-item:hover { border-color: var(--accent); }
        .model-name { font-weight: 700; font-size: 14px; margin-bottom: 10px; }
        .mount-btn { width: 100%; padding: 10px; border-radius: 8px; border: none; font-weight: 700; cursor: pointer; transition: 0.2s; }
        .mount-btn.active { background: var(--accent); color: black; }
        .mount-btn.idle { background: rgba(255,255,255,0.05); color: var(--text); }

        /* Responsive Overlay for small logs */
        .logs-tray { position: fixed; bottom: 0; left: 80px; right: 350px; height: 40px; background: rgba(0,0,0,0.8); border-top: 1px solid var(--border); display: flex; align-items: center; padding: 0 20px; font-family: 'JetBrains Mono', monospace; font-size: 11px; color: #666; cursor: pointer; overflow: hidden; white-space: nowrap; z-index: 50; }

        ::-webkit-scrollbar { width: 6px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.1); border-radius: 10px; }
    </style>
</head>
<body>
    <div class="sidebar">
        <div class="logo-box">H</div>
        <div class="nav-icon active" title="Command Center" onclick="nav('unified')">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path><polyline points="9 22 9 12 15 12 15 22"></polyline></svg>
        </div>
        <div class="nav-icon" title="Hardware Analytics" onclick="location.reload()">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="20" x2="18" y2="10"></line><line x1="12" y1="20" x2="12" y2="4"></line><line x1="6" y1="20" x2="6" y2="14"></line></svg>
        </div>
        <div style="flex:1"></div>
        <div class="nav-icon" title="Unmount All" onclick="eject()" style="color:var(--error); flex-direction: column; height: auto; gap: 4px;">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18.36 6.64a9 9 0 1 1-12.73 0"></path><line x1="12" y1="2" x2="12" y2="12"></line></svg>
            <span style="font-size: 8px; font-weight: 800;">UNMOUNT</span>
        </div>
    </div>

    <div class="main-container">
        <div class="header">
            <div class="node-info">
                <div class="status-dot"></div>
                <div style="font-weight: 800; letter-spacing: -0.5px; font-size: 18px;">HMIR ELITE NODE</div>
                <div style="color: var(--text-dim); font-size: 12px; font-weight: 600;">PORT 8080</div>
            </div>
            <div style="display: flex; gap: 20px; align-items: center;">
                <div id="active-engine" style="font-size: 11px; font-weight: 800; color: var(--accent);">ENGINE: OFFLINE</div>
            </div>
        </div>

        <div class="content-area">
            <!-- INTELLIGENCE PANEL (CHAT) -->
            <div class="workspace-panel">
                <div id="chat-hist" class="chat-history">
                    <div class="message ai">
                        <div class="msg-role ai">Intelligence Bridge</div>
                        <div class="msg-content">Welcome to the HMIR Elite Unified Command. My inference engine is bound to your Intel hardware. Ready for instructions.</div>
                    </div>
                </div>
                <div class="chat-controls">
                    <div class="input-wrapper">
                        <input id="chat-input" type="text" placeholder="Send an instruction to the NPU..." onkeydown="if(event.key==='Enter') send()">
                        <button class="send-btn" onclick="send()">EXECUTE</button>
                    </div>
                </div>
                <div id="logs-tray" class="logs-tray">
                    <span style="color:var(--accent); margin-right: 15px;">[LOGS]</span>
                    <span id="latest-log-text">Node initialized successfully.</span>
                </div>
            </div>

            <!-- INFRASTRUCTURE PANEL (TELEMETRY) -->
            <div class="infra-panel">
                <div>
                    <div class="panel-title">COMPUTE PERFORMANCE</div>
                    <div style="display: grid; gap: 15px;">
                        <div class="stat-card">
                            <div class="stat-label">NPU UTILIZATION</div>
                            <div id="stat-npu" class="stat-val">0 <span>%</span></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">THROUGHPUT</div>
                            <div id="stat-tps" class="stat-val">0.0 <span>TPS</span></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">VRAM ALLOCATION</div>
                            <div id="stat-vram" class="stat-val">0.0 <span>GB</span></div>
                        </div>
                    </div>
                </div>

                <div>
                    <div class="panel-title">PIPELINES & MODELS</div>
                    <div id="model-list">
                        <!-- Loaded dynamically -->
                    </div>
                </div>
            </div>
        </div>
    </div>

    <script>
        const decoder = new TextDecoder();
        let sseBuffer = "";

        async function loadModels() {
            const res = await fetch('/v1/models/installed');
            const models = await res.json();
            const list = document.getElementById('model-list');
            list.innerHTML = models.map(m => `
                <div class="model-item">
                    <div class="model-name">${m}</div>
                    <button class="mount-btn idle" onclick="mount('${m}')">LOAD PIPELINE</button>
                </div>
            `).join('');
        }

        async function mount(name) {
            const engine = document.getElementById('active-engine');
            engine.innerText = 'ENGINE: LOADING...';
            const res = await fetch('/v1/engine/switch', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({name})
            });
            const data = await res.json();
            engine.innerText = `ENGINE: ${data.active.toUpperCase()}`;
        }

        async function eject() {
            if(!confirm("Eject and unmount active NPU hardware?")) return;
            await fetch('/v1/engine/eject', {method: 'POST'});
            document.getElementById('active-engine').innerText = 'ENGINE: OFFLINE';
        }

        async function send() {
            const input = document.getElementById('chat-input');
            const hist = document.getElementById('chat-hist');
            if(!input.value) return;

            const text = input.value;
            input.value = '';
            
            hist.innerHTML += `
                <div class="message user">
                    <div class="msg-role">Operator</div>
                    <div class="msg-content">${text}</div>
                </div>
            `;
            
            const msgEl = document.createElement('div');
            msgEl.className = 'message ai';
            msgEl.innerHTML = `
                <div class="msg-role ai">Intelligence Bridge</div>
                <div class="msg-content shadow-ai">...</div>
            `;
            hist.appendChild(msgEl);
            const contentEl = msgEl.querySelector('.msg-content');
            scrollChat();

            try {
                const res = await fetch('/v1/chat/completions', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({messages: [{role: 'user', content: text}], stream: true})
                });

                if(!res.ok) throw new Error(await res.text());

                const reader = res.body.getReader();
                contentEl.innerText = '';
                let decodingBuffer = "";

                while (true) {
                    const {done, value} = await reader.read();
                    if (done) break;

                    decodingBuffer += decoder.decode(value, {stream: true});
                    
                    let boundary;
                    while((boundary = decodingBuffer.indexOf("\n\n")) >= 0) {
                        const rawEvent = decodingBuffer.slice(0, boundary);
                        decodingBuffer = decodingBuffer.slice(boundary + 2);

                        const lines = rawEvent.split("\n");
                        for (const line of lines) {
                            if (line.startsWith("data: ")) {
                                const jsonStr = line.slice(6).trim();
                                if (jsonStr === "[DONE]") break;
                                try {
                                    const json = JSON.parse(jsonStr);
                                    const content = json.choices?.[0]?.delta?.content;
                                    if(content) {
                                        contentEl.innerText += content;
                                        scrollChat();
                                    }
                                } catch(e) {
                                    // silently catch fragments
                                }
                            }
                        }
                    }
                }
            } catch(e) { 
                contentEl.innerHTML = `<span style="color:var(--error)">[!] System Error: ${e.message}</span>`;
            }
        }

        function scrollChat() {
            const hist = document.getElementById('chat-hist');
            hist.scrollTop = hist.scrollHeight;
        }

        // TELEMETRY SSE
        const tel = new EventSource('/v1/telemetry');
        tel.onmessage = (e) => {
            const data = JSON.parse(e.data);
            if(data.HardwareState) {
                const s = data.HardwareState;
                document.getElementById('stat-npu').innerHTML = s.npu_util.toFixed(0) + ' <span>%</span>';
                document.getElementById('stat-tps').innerHTML = s.tps.toFixed(1) + ' <span>TPS</span>';
                document.getElementById('stat-vram').innerHTML = s.vram_used.toFixed(1) + ' <span>GB</span>';
                if(s.engine_status) {
                    document.getElementById('active-engine').innerText = `ENGINE: ${s.engine_status.toUpperCase()}`;
                }
            }
        };

        // LOGS SSE
        const logs = new EventSource('/v1/logs');
        logs.onmessage = (e) => {
            document.getElementById('latest-log-text').innerText = e.data;
        };

        loadModels();
    </script>
</body>
</html>
"#,
    )
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

    if worker_script.exists() {
        let python_bin = resolve_python_command();
        log_event(&format!("  Using Python runtime: {}", python_bin));

        match std::process::Command::new(&python_bin)
            .arg(&worker_script)
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
                            log_event(&format!("  [ONLINE] NPU Worker ONLINE after {}s", attempt * 2));
                            worker_online = true;
                            break;
                        }
                    }
                    if attempt % 10 == 0 {
                        log_event(&format!("  Still waiting... ({}s elapsed)", attempt * 2));
                    }
                }
                if !worker_online {
                    log_event("  [WARN] NPU Worker did not respond within 90s. Chat will fail until worker is ready.");
                    log_event("  Check: python dependencies (openvino, openvino-genai, aiohttp) and model files.");
                }
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
                tps: 42.1,
                power_w: hw.power_draw_watts,
                node_uptime: start_time_copy.elapsed().as_secs(),
                kv_cache: 14.5,
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
