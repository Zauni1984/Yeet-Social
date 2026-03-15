use axum::{Router, routing::get, Json};
use serde_json::{json, Value};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/feed",   get(feed));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Yeet API running on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "yeet-api" }))
}

async fn feed() -> Json<Value> {
    Json(json!({
        "success": true,
        "data": [],
        "message": "Yeet API is live! Full backend coming soon."
    }))
}
