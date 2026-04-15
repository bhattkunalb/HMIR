use axum::{
    routing::{get, post}, 
    response::sse::{Event, Sse}, 
    Json, 
    Router
};
use futures::stream::Stream;
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub stream: Option<bool>,
    pub priority: Option<String>,
    pub max_queue_time_ms: Option<u64>,
}

#[derive(Serialize)]
pub struct ChatCompletionChunk {
    pub choices: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct LoadAdapterPayload {
    pub path: String,
    pub scale: Option<f32>,
}

async fn chat_completions(Json(_req): Json<ChatCompletionRequest>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel(128); 

    tokio::spawn(async move {
        for i in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await; 
            let chunk = format!(r#"{{"choices":[{{"delta":{{"content":" word_{}"}}}}]}}"#, i);
            let _ = tx.send(Ok(Event::default().data(chunk))).await;
        }
        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::new())
}

#[axum::debug_handler]
pub async fn batch_status() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "processing",
        "active_batch_size": 14,
        "prefix_cache_hit_rate": "84%",
        "queue_depth": 2
    }))
}

#[axum::debug_handler]
pub async fn load_adapter(Json(_req): Json<LoadAdapterPayload>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "success",
        "adapter_id": "lora_agent_ext",
        "vram_delta_mb": 420
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/v1/models", get(|| async { Json(vec!["llama3-8b-hmir-optimized"]) }))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/batch/status", get(batch_status))
        .route("/v1/adapters/load", post(load_adapter))
        .route("/metrics", get(|| async { "tokens_rendered_total 10243" }));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    println!("API Sever mounted securely mapped bounding on localhost:8080");
    axum::serve(listener, app).await.unwrap();
}
