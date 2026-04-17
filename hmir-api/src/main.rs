use tower_http::cors::{Any, CorsLayer};
use axum::{
    routing::{get, post}, 
    response::{sse::{Event, Sse}, Html}, 
    Json, 
    Router,
    extract::State
};
use async_stream;
use futures::stream::Stream;
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub name: String,
}

#[derive(Clone)]
pub struct AppState {
    pub active_model: Arc<Mutex<String>>,
    pub telemetry: Arc<hmir_core::telemetry::TelemetrySink>,
    pub start_time: std::time::Instant,
    pub client: reqwest::Client,
}

#[derive(Deserialize)]
pub struct SwitchModelPayload {
    pub name: String,
}

fn log_event(msg: &str) {
    let mut path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("hmir");
    path.push("logs");
    let _ = std::fs::create_dir_all(&path);
    path.push("api.log");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] {}", timestamp, msg);
    }
}

pub async fn list_installed_models() -> Json<Vec<String>> {
    let mut models = Vec::new();
    let mut path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("hmir");
    path.push("models");

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
    Json(payload): Json<SwitchModelPayload>
) -> Json<serde_json::Value> {
    let mut active = state.active_model.lock().unwrap();
    *active = payload.name.clone();
    
    let engine = if payload.name.to_lowercase().contains("ov") || payload.name.to_lowercase().contains("openvino") {
        "OPENVINO/NPU"
    } else {
        "LLAMA.CPP/CUDA"
    };

    log_event(&format!("MOUNTING ENGINE: {} ({})", payload.name, engine));
    
    let _ = state.telemetry.emit(hmir_core::telemetry::TelemetryEvent::ModelMounted {
        name: payload.name.clone(),
        engine: engine.to_string(),
    });

    Json(serde_json::json!({ "status": "switched", "active": *active, "engine": engine }))
}

#[derive(Deserialize)]
pub struct DownloadModelPayload {
    pub repo_id: String,
    pub folder_name: String,
}

pub async fn download_model(
    State(state): State<AppState>,
    Json(payload): Json<DownloadModelPayload>
) -> Json<serde_json::Value> {
    log_event(&format!("STARTING DOWNLOAD: {} into {}", payload.repo_id, payload.folder_name));
    
    let tel_clone = state.telemetry.clone();
    let repo = payload.repo_id.clone();
    let folder = payload.folder_name.clone();

    tokio::spawn(async move {
        let _ = tel_clone.emit(hmir_core::telemetry::TelemetryEvent::DownloadStatus {
            model: repo.clone(),
            status: "Starting".to_string(),
            progress: 0.0,
        });

        // Resolve absolute path to script
        let mut script_path = std::env::current_dir().unwrap_or_default();
        script_path.push("scripts");
        script_path.push("download_npu_model.py");

        let status = tokio::process::Command::new("python")
            .arg(script_path)
            .arg(&repo)
            .arg(&folder)
            .status()
            .await;

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
    Json(req): Json<ChatCompletionRequest>
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    log_event("INCOMING CHAT REQUEST -> NPU PROXY");

    // Standard NPU Worker port is 8089 (defined in scripts/hmir_npu_worker.py)
    let url = "http://127.0.0.1:8089/v1/chat/completions";
    let (tx, rx) = mpsc::channel(128);

    tokio::spawn(async move {
        let resp = match state.client.post(url).json(&req).send().await {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Ok(Event::default().data(format!(r#"{{"error": "NPU Proxy Error: {}"}}"#, e)))).await;
                return;
            }
        };

        let mut stream = resp.bytes_stream();
        use futures::StreamExt;
        
        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    // Each line in our worker SSE is "data: {...}\n\n"
                    for line in text.lines() {
                        if line.starts_with("data: ") {
                            let data = line.strip_prefix("data: ").unwrap_or("");
                            if !data.is_empty() {
                                let _ = tx.send(Ok(Event::default().data(data.to_string()))).await;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::new())
}

#[derive(Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<serde_json::Value>,
    pub temperature: Option<f32>,
    pub stream: Option<bool>,
}

async fn telemetry_stream(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
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
    Html(r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>HMIR Elite | Portal</title>
            <style>
                :root { --bg: #0a0a0c; --card: #141417; --accent: #00f2ff; --text: #e0e0e0; }
                body { background: var(--bg); color: var(--text); font-family: 'Segoe UI', system-ui, sans-serif; margin: 0; display: flex; height: 100vh; overflow: hidden; }
                .sidebar { width: 240px; background: #000; display: flex; flex-direction: column; border-right: 1px solid #222; }
                .nav-item { padding: 20px; cursor: pointer; border-left: 4px solid transparent; transition: 0.2s; }
                .nav-item:hover { background: #111; }
                .nav-item.active { border-left-color: var(--accent); background: #1a1a1c; }
                .content { flex: 1; display: flex; flex-direction: column; }
                .header { background: #1a1a1c; padding: 15px 30px; display: flex; align-items: center; border-bottom: 1px solid #222; justify-content: space-between; }
                .view { flex: 1; padding: 40px; overflow-y: auto; display: none; }
                .view.active { display: block; }
                
                /* Grid */
                .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; }
                .card { background: var(--card); padding: 25px; border-radius: 12px; border: 1px solid #333; }
                .stat { font-size: 36px; font-weight: 800; color: var(--accent); margin-top: 5px; }
                
                /* Chat */
                .chat-container { height: 100%; display: flex; flex-direction: column; max-width: 900px; margin: 0 auto; position: relative; }
                .messages { flex: 1; overflow-y: auto; padding: 20px; display: flex; flex-direction: column; gap: 15px; }
                .bubble { padding: 12px 18px; border-radius: 18px; max-width: 80%; line-height: 1.5; font-size: 15px; }
                .user { background: var(--accent); color: black; align-self: flex-end; }
                .ai { background: #2a2a2e; align-self: flex-start; }
                .input-area { padding: 20px; background: #1a1a1c; display: flex; gap: 10px; border-top: 1px solid #333; }
                input, select { background: #000; border: 1px solid #444; color: white; padding: 12px; border-radius: 6px; outline: none; }
                button { background: var(--accent); color: black; border: none; padding: 10px 20px; border-radius: 6px; font-weight: bold; cursor: pointer; }
                
                #model-status { font-size: 12px; color: #888; }
            </style>
        </head>
        <body>
            <div class="sidebar">
                <div style="padding: 30px; font-weight: bold; font-size: 20px; color: var(--accent);">HMIR NODE</div>
                <div class="nav-item active" onclick="show('monitor', this)">📊 PERFORMANCE</div>
                <div class="nav-item" onclick="show('chat', this)">💬 LLAMA CHAT</div>
                <div style="flex:1"></div>
                <div style="padding:20px">
                    <div style="margin-bottom:8px; font-size:11px">SELECT ENGINE</div>
                    <select id="model-select" style="width:100%" onchange="mount()">
                        <option>Loading...</option>
                    </select>
                </div>
            </div>
            <div class="content">
                <div class="header">
                    <h2 id="view-title">Health & Telemetry</h2>
                    <div id="model-status">ACTIVE: NONE</div>
                </div>
                
                <div id="monitor" class="view active">
                    <div class="grid">
                        <div class="card"><div>THROUGHPUT</div><div id="stat-tps" class="stat">0.0 T/s</div></div>
                        <div class="card"><div>AI BOOST</div><div id="stat-npu" class="stat">0%</div></div>
                        <div class="card"><div>VRAM ALLOC</div><div id="stat-vram" class="stat">0.0 GB</div></div>
                    </div>
                </div>

                <div id="chat" class="view">
                    <div class="chat-container">
                        <div class="messages" id="chat-hist">
                            <div class="bubble ai">Hello! I am the Llama-powered HMIR Interface. Type a message to begin.</div>
                        </div>
                        <div class="input-area">
                            <input type="text" id="chat-input" placeholder="Ask Llama anything..." onkeydown="if(event.key==='Enter') send()">
                            <button onclick="send()">SEND</button>
                        </div>
                    </div>
                </div>
            </div>

            <script>
                function show(id, el) {
                    document.querySelectorAll('.view').forEach(v => v.classList.remove('active'));
                    document.querySelectorAll('.nav-item').forEach(v => v.classList.remove('active'));
                    document.getElementById(id).classList.add('active');
                    el.classList.add('active');
                    document.getElementById('view-title').innerText = id === 'monitor' ? 'Health & Telemetry' : 'Llama.cpp Interface';
                }

                async function loadModels() {
                    const res = await fetch('http://localhost:8080/v1/models/installed');
                    const models = await res.json();
                    const sel = document.getElementById('model-select');
                    sel.innerHTML = models.map(m => `<option value="${m}">${m}</option>`).join('');
                }

                async function mount() {
                    const name = document.getElementById('model-select').value;
                    const res = await fetch('http://localhost:8080/v1/engine/switch', {
                        method: 'POST',
                        headers: {'Content-Type': 'application/json'},
                        body: JSON.stringify({name})
                    });
                    const data = await res.json();
                    document.getElementById('model-status').innerText = `ACTIVE: ${data.active.toUpperCase()}`;
                }

                async function send() {
                    const input = document.getElementById('chat-input');
                    const hist = document.getElementById('chat-hist');
                    if(!input.value) return;
                    
                    const userMsg = input.value;
                    input.value = '';
                    hist.innerHTML += `<div class="bubble user">${userMsg}</div>`;
                    
                    const aiBubble = document.createElement('div');
                    aiBubble.className = 'bubble ai';
                    aiBubble.innerText = '...';
                    hist.appendChild(aiBubble);
                    hist.scrollTop = hist.scrollHeight;

                    try {
                        const res = await fetch('http://localhost:8080/v1/chat/completions', {
                            method: 'POST',
                            headers: {'Content-Type': 'application/json'},
                            body: JSON.stringify({messages: [{role: 'user', content: userMsg}], stream: true})
                        });
                        
                        const reader = res.body.getReader();
                        aiBubble.innerText = '';
                        while (true) {
                            const {done, value} = await reader.read();
                            if (done) break;
                            const text = new TextDecoder().decode(value);
                            const lines = text.split('\n');
                            for (const line of lines) {
                                if (line.startsWith('data:')) {
                                    const jsonStr = line.slice(5).trim();
                                    if (jsonStr === '[DONE]') break;
                                    const json = JSON.parse(jsonStr);
                                    aiBubble.innerText += json.choices[0].delta.content;
                                }
                            }
                        }
                    } catch(e) { aiBubble.innerText = 'Error: Ensure API is running on 8080.'; }
                }

                // Live Telemetry
                const evtSource = new EventSource('http://localhost:8080/v1/telemetry');
                evtSource.onmessage = (e) => {
                   const data = JSON.parse(e.data);
                   if(data.HardwareState) {
                       const s = data.HardwareState;
                       document.getElementById('stat-tps').innerText = `${s.tps.toFixed(1)} T/s`;
                       document.getElementById('stat-npu').innerText = `${s.npu_util.toFixed(0)}%`;
                       document.getElementById('stat-vram').innerText = `${s.vram_used.toFixed(1)} GB`;
                   }
                };

                loadModels();
            </script>
        </body>
        </html>
    "#)
}

#[tokio::main]
async fn main() {
    let port = 8080;
    let web_port = 8081;

    let telemetry = hmir_core::telemetry::TelemetrySink::new(1024);
    let telemetry_arc = std::sync::Arc::new(telemetry);
    let active_model = Arc::new(Mutex::new("None".to_string()));

    let state = AppState {
        active_model: active_model.clone(),
        telemetry: telemetry_arc.clone(),
        start_time: std::time::Instant::now(),
        client: reqwest::Client::new(),
    };

    log_event("HMIR API v1.6.1 STARTING (CORS Enabled)...");

    // Background Hardware Polling
    let tel_clone = telemetry_arc.clone();
    let start_time_copy = state.start_time; 
    tokio::spawn(async move {
        loop {
            let hw = hmir_hardware_prober::os_polling::poll_hardware().await;
            let ram_gb = hw.ram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let vram_gb = hw.vram_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

            let _ = tel_clone.emit(hmir_core::telemetry::TelemetryEvent::HardwareState {
                cpu_util: hw.cpu_util_pct, 
                gpu_util: hw.gpu_util_pct,
                npu_util: hw.npu_util_pct,
                cpu_temp: hw.cpu_temp_c,
                gpu_temp: hw.gpu_temp_c,
                vram_used: vram_gb,
                vram_total: hw.vram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                ram_used: ram_gb,
                ram_total: hw.ram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                tps: 42.1,
                power_w: hw.power_draw_watts,
                node_uptime: start_time_copy.elapsed().as_secs(),
                kv_cache: 14.5,
                cpu_name: hw.cpu_name.clone(),
                gpu_name: hw.gpu_name.clone(),
                npu_name: hw.npu_name.clone(),
                disk_free: hw.disk_free_gb,
            });
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
        }
    });

    // CORS MiddleWare
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Main API (8080)
    let api_app = Router::new()
        .route("/v1/models/installed", get(list_installed_models))
        .route("/v1/models/download", post(download_model))
        .route("/v1/engine/switch", post(switch_model))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/telemetry", get(telemetry_stream))
        .route("/v1/health", get(health_check))
        .layer(cors)
        .with_state(state.clone());

    // Web UI (8081)
    let web_app = Router::new().route("/", get(serve_web_ui));

    let api_listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    let web_listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", web_port)).await.unwrap();

    println!("🚀 HMIR API: http://localhost:{}", port);
    println!("🌐 HMIR Web UI: http://localhost:{}", web_port);

    tokio::select! {
        res = axum::serve(api_listener, api_app) => { res.unwrap(); },
        res = axum::serve(web_listener, web_app) => { res.unwrap(); },
    }
}
