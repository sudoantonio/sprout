use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{AUTHORIZATION, RETRY_AFTER},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

use crate::AppState;

const RATE_WINDOW: Duration = Duration::from_secs(60);
const MAX_RATE_KEYS: usize = 10_000;

#[derive(Default)]
pub struct Metrics {
    started_at: Mutex<Option<Instant>>,
    http_requests: AtomicU64,
    http_errors: AtomicU64,
    rate_limit_rejections: AtomicU64,
    quota_rejections: AtomicU64,
}

impl Metrics {
    pub fn start(&self) {
        *self.started_at.lock().expect("metrics mutex poisoned") = Some(Instant::now());
    }

    pub fn render(&self, worker_lag_seconds: f64) -> String {
        let uptime = self
            .started_at
            .lock()
            .expect("metrics mutex poisoned")
            .as_ref()
            .map_or(0.0, |started| started.elapsed().as_secs_f64());
        format!(
            concat!(
                "# HELP sprout_uptime_seconds Process uptime.\n",
                "# TYPE sprout_uptime_seconds gauge\n",
                "sprout_uptime_seconds {uptime:.3}\n",
                "# HELP sprout_http_requests_total HTTP responses produced.\n",
                "# TYPE sprout_http_requests_total counter\n",
                "sprout_http_requests_total {requests}\n",
                "# HELP sprout_http_errors_total HTTP 5xx responses produced.\n",
                "# TYPE sprout_http_errors_total counter\n",
                "sprout_http_errors_total {errors}\n",
                "# HELP sprout_rate_limit_rejections_total Requests rejected by abuse controls.\n",
                "# TYPE sprout_rate_limit_rejections_total counter\n",
                "sprout_rate_limit_rejections_total {rate_limited}\n",
                "# HELP sprout_quota_rejections_total Body or file quota rejections.\n",
                "# TYPE sprout_quota_rejections_total counter\n",
                "sprout_quota_rejections_total {quota}\n",
                "# HELP sprout_worker_lag_seconds Age of the oldest due retention job.\n",
                "# TYPE sprout_worker_lag_seconds gauge\n",
                "sprout_worker_lag_seconds {worker_lag_seconds:.3}\n",
            ),
            uptime = uptime,
            requests = self.http_requests.load(Ordering::Relaxed),
            errors = self.http_errors.load(Ordering::Relaxed),
            rate_limited = self.rate_limit_rejections.load(Ordering::Relaxed),
            quota = self.quota_rejections.load(Ordering::Relaxed),
            worker_lag_seconds = worker_lag_seconds,
        )
    }

    fn observe(&self, status: StatusCode) {
        self.http_requests.fetch_add(1, Ordering::Relaxed);
        if status.is_server_error() {
            self.http_errors.fetch_add(1, Ordering::Relaxed);
        }
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            self.quota_rejections.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn rate_limited(&self) {
        self.rate_limit_rejections.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<RateKey, Bucket>>,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum RateClass {
    Authentication,
    Recovery,
    Session,
}

#[derive(Eq, Hash, PartialEq)]
struct RateKey {
    class: RateClass,
    subject_hash: [u8; 32],
}

struct Bucket {
    window_started: Instant,
    count: u32,
}

impl RateLimiter {
    fn allow(&self, class: RateClass, subject_hash: [u8; 32], limit: u32) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        if buckets.len() >= MAX_RATE_KEYS {
            buckets.retain(|_, bucket| now.duration_since(bucket.window_started) < RATE_WINDOW);
            if buckets.len() >= MAX_RATE_KEYS {
                return false;
            }
        }
        let bucket = buckets
            .entry(RateKey {
                class,
                subject_hash,
            })
            .or_insert(Bucket {
                window_started: now,
                count: 0,
            });
        if now.duration_since(bucket.window_started) >= RATE_WINDOW {
            bucket.window_started = now;
            bucket.count = 0;
        }
        if bucket.count >= limit {
            false
        } else {
            bucket.count += 1;
            true
        }
    }
}

pub async fn observe_requests(
    State(state): State<std::sync::Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let response = next.run(request).await;
    state.metrics.observe(response.status());
    response
}

pub async fn enforce_rate_limits(
    State(state): State<std::sync::Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(class) = rate_class(request.uri().path()) else {
        return next.run(request).await;
    };
    let limit = match class {
        RateClass::Authentication => state.config.auth_rate_limit_per_minute,
        RateClass::Recovery => state.config.recovery_rate_limit_per_minute,
        RateClass::Session => state.config.session_rate_limit_per_minute,
    };
    let subject_hash = request_subject_hash(&request);
    if state.rate_limiter.allow(class, subject_hash, limit) {
        next.run(request).await
    } else {
        state.metrics.rate_limited();
        let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from_static("60"));
        response
    }
}

fn rate_class(path: &str) -> Option<RateClass> {
    if path.contains("/recovery") {
        Some(RateClass::Recovery)
    } else if path.starts_with("/v1/auth/") {
        Some(RateClass::Authentication)
    } else if path.starts_with("/v1/") {
        Some(RateClass::Session)
    } else {
        None
    }
}

fn request_subject_hash(request: &Request) -> [u8; 32] {
    let mut hasher = Sha256::new();
    if let Some(peer) = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(peer)| peer)
    {
        hasher.update(peer.ip().to_string().as_bytes());
    } else {
        hasher.update(b"unknown-peer");
    }
    hasher.update([0]);
    if let Some(authorization) = request.headers().get(AUTHORIZATION) {
        hasher.update(authorization.as_bytes());
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_rate_limiter_rejects_after_limit() {
        let limiter = RateLimiter::default();
        let subject = [7; 32];
        assert!(limiter.allow(RateClass::Authentication, subject, 2));
        assert!(limiter.allow(RateClass::Authentication, subject, 2));
        assert!(!limiter.allow(RateClass::Authentication, subject, 2));
    }

    #[test]
    fn recovery_routes_use_the_stricter_dedicated_bucket() {
        assert!(matches!(
            rate_class("/v1/auth/email/recovery/start"),
            Some(RateClass::Recovery)
        ));
        assert!(matches!(
            rate_class("/v1/projects/id/recovery-requests"),
            Some(RateClass::Recovery)
        ));
    }

    #[test]
    fn metrics_have_fixed_labels_and_no_request_data() {
        let metrics = Metrics::default();
        metrics.start();
        metrics.observe(StatusCode::INTERNAL_SERVER_ERROR);
        let output = metrics.render(12.5);
        assert!(output.contains("sprout_http_errors_total 1"));
        assert!(!output.contains("authorization"));
        assert!(!output.contains("request_id"));
    }
}
