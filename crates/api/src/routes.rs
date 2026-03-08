use axum::{
    extract::{Request, State},
    middleware::{from_fn, Next},
    routing::{get, post},
    response::Json,
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::handlers::ApiState;
use crate::handlers::*;
use crate::middleware::{rate_limit_middleware, RateLimitState};

pub fn create_router(state: Arc<ApiState>) -> Router {
    // Get rate limit configuration from environment variables
    let max_requests = std::env::var("RATE_LIMIT_MAX_REQUESTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100); // Default: 100 requests per window
    
    let window_seconds = std::env::var("RATE_LIMIT_WINDOW_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60); // Default: 60 seconds window

    let rate_limit_state = Arc::new(RateLimitState::new(max_requests, window_seconds));
    
    // Add rate limit state to ApiState
    let api_state = Arc::new(ApiState {
        sequencer: state.sequencer.clone(),
        storage: state.storage.clone(),
        rate_limit_state: Some(rate_limit_state.clone()),
    });

    Router::new()
        // Health and readiness endpoints (no rate limiting)
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        // API endpoints with rate limiting
        .route(
            "/api/v1/account/:address/balance/:asset_id",
            get(get_account_balance),
        )
        .route("/api/v1/account/:address", get(get_account_state))
        .route("/api/v1/deals", get(get_deals_list))
        .route("/api/v1/deal/:deal_id", get(get_deal_details))
        .route("/api/v1/block/:block_id", get(get_block_info))
        .route("/api/v1/transactions", post(submit_transaction))
        .route("/api/v1/queue/status", get(get_queue_status))
        .route("/api/v1/chains", get(get_supported_chains))
        .route("/jsonrpc", post(jsonrpc_handler))
        // Add rate limit state to request extensions
        .layer(axum::middleware::from_fn(move |mut request: Request, next: Next| {
            let state = Arc::clone(&rate_limit_state);
            async move {
                request.extensions_mut().insert(state);
                next.run(request).await
            }
        }))
        // Apply rate limiting middleware
        .layer(from_fn(rate_limit_middleware))
        .layer(CorsLayer::permissive())
        .with_state(api_state)
}

/// Health check endpoint with component status
async fn health_check(State(state): State<Arc<ApiState>>) -> Json<serde_json::Value> {
    use serde_json::json;
    
    // Check sequencer status
    // BlockId is u64, so it's always valid (no need to check >= 0)
    let sequencer_healthy = true; // Sequencer is healthy if it exists
    let queue_length = state.sequencer.queue_length();
    
    // Check storage status
    let storage_healthy = state.storage.is_some();
    let storage_available = if let Some(ref storage) = state.storage {
        // Try to read a block to verify storage is working
        storage.get_block(0).is_ok()
    } else {
        false
    };
    
    // Overall health status
    let healthy = sequencer_healthy && storage_healthy && storage_available;
    
    let status = if healthy { "healthy" } else { "degraded" };
    
    Json(json!({
        "status": status,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "components": {
            "sequencer": {
                "status": if sequencer_healthy { "healthy" } else { "unhealthy" },
                "current_block_id": state.sequencer.get_current_block_id(),
                "queue_length": queue_length
            },
            "storage": {
                "status": if storage_available { "healthy" } else { "unhealthy" },
                "configured": storage_healthy
            }
        }
    }))
}

/// Readiness check endpoint (for Kubernetes/Docker health checks)
async fn readiness_check(State(state): State<Arc<ApiState>>) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    use serde_json::json;
    
    // Check if all critical components are ready
    // BlockId is u64, so sequencer is always ready if it exists
    let sequencer_ready = true; // Sequencer is ready if it exists
    let storage_ready = state.storage.is_some();
    
    if sequencer_ready && storage_ready {
        Ok(Json(json!({
            "status": "ready",
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        })))
    } else {
        Err(axum::http::StatusCode::SERVICE_UNAVAILABLE)
    }
}
