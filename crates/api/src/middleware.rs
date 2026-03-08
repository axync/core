use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Rate limiter state
#[derive(Clone)]
pub struct RateLimitState {
    max_requests: u32,
    window_seconds: u64,
    requests: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl RateLimitState {
    pub fn new(max_requests: u32, window_seconds: u64) -> Self {
        Self {
            max_requests,
            window_seconds,
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if request should be allowed
    pub fn check_rate_limit(&self, client_ip: &str) -> Result<(), StatusCode> {
        let now = Instant::now();
        let window = Duration::from_secs(self.window_seconds);

        let mut requests_map = self.requests.lock().unwrap();
        
        // Clean up old requests outside the window
        if let Some(timestamps) = requests_map.get_mut(client_ip) {
            timestamps.retain(|&timestamp| now.duration_since(timestamp) < window);
            
            // Check if limit exceeded
            if timestamps.len() >= self.max_requests as usize {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            
            // Add current request
            timestamps.push(now);
        } else {
            // First request from this IP
            requests_map.insert(client_ip.to_string(), vec![now]);
        }

        Ok(())
    }
}

/// Rate limit middleware function
pub async fn rate_limit_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Get rate limit state from request extensions
    let rate_limit_state = request
        .extensions()
        .get::<Arc<RateLimitState>>()
        .cloned();

    // If rate limiting is enabled, check it
    if let Some(state) = rate_limit_state {
        // Get client IP for rate limiting
        let client_ip = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .or_else(|| {
                request
                    .headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
            })
            .unwrap_or("unknown");

        // Check rate limit
        if let Err(_) = state.check_rate_limit(client_ip) {
            // Rate limit exceeded
            let body = serde_json::json!({
                "error": "RateLimitExceeded",
                "message": "Rate limit exceeded. Please try again later."
            });
            return (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
        }
    }

    // Allow request
    next.run(request).await
}

