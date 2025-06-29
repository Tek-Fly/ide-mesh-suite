use anyhow::Result;
use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
    trace::TraceLayer,
};
use tracing::{info, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod handlers;
mod llm;
mod models;
mod services;
mod state;
mod websocket;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    init_tracing()?;

    // Load configuration
    let config = Config::from_env()?;
    info!("Starting Chat Service v{}", env!("CARGO_PKG_VERSION"));

    // Initialize application state
    let state = Arc::new(AppState::new(config.clone()).await?);

    // Build router
    let app = Router::new()
        // Health & Metrics
        .route("/health", get(handlers::health::health_check))
        .route("/metrics", get(handlers::metrics::metrics_handler))
        // Chat endpoints
        .route("/api/v1/chat/completions", post(handlers::chat::chat_completion))
        .route("/api/v1/chat/stream", post(handlers::chat::chat_stream))
        .route("/api/v1/chat/ws", get(websocket_handler))
        // Conversation management
        .route("/api/v1/conversations", get(handlers::conversations::list_conversations))
        .route("/api/v1/conversations", post(handlers::conversations::create_conversation))
        .route("/api/v1/conversations/:id", get(handlers::conversations::get_conversation))
        .route("/api/v1/conversations/:id", delete(handlers::conversations::delete_conversation))
        .route("/api/v1/conversations/:id/messages", get(handlers::conversations::get_messages))
        // Model management
        .route("/api/v1/models", get(handlers::models::list_models))
        .route("/api/v1/models/:id/status", get(handlers::models::model_status))
        // Token usage
        .route("/api/v1/usage", get(handlers::usage::get_usage))
        .route("/api/v1/usage/limits", get(handlers::usage::get_limits))
        // State
        .with_state(state)
        // Middleware
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10MB limit
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Chat Service listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket::handle_socket(socket, state))
}

fn init_tracing() -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(true)
                .with_ansi(true)
                .json(),
        )
        .init();

    Ok(())
}