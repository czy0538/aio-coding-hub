//! Usage: Keyword review interception layer for the gateway proxy.
//!
//! When enabled, incoming requests are checked against a configured keyword list.
//! If sensitive keywords are found, the request is held with SSE comment heartbeats
//! until a human reviewer approves or rejects it via the desktop UI.

use crate::domain::keyword_review as domain;
use crate::gateway::manager::GatewayAppState;
use crate::settings;
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderValue, Response, StatusCode};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

// ── Review Decision ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Approve,
    Reject,
}

// ── Pending Review Registry ──

/// Snapshot of a pending review for listing in the UI (no channel).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PendingReviewSnapshot {
    pub trace_id: String,
    pub cli_key: String,
    pub matched_keywords: Vec<String>,
    pub request_snippet: Option<String>,
    pub created_at: i64,
}

struct PendingReviewEntry {
    tx: oneshot::Sender<ReviewDecision>,
    cli_key: String,
    matched_keywords: Vec<String>,
    request_snippet: Option<String>,
    created_at: i64,
}

/// In-memory registry of pending reviews, keyed by trace_id.
/// Bridges the gateway handler (waiting) and the Tauri command (signaling).
pub struct PendingReviewRegistry {
    inner: Mutex<HashMap<String, PendingReviewEntry>>,
}

impl PendingReviewRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Insert a new pending review. Returns the receiver the handler will await.
    pub(super) fn insert(
        &self,
        trace_id: String,
        cli_key: String,
        matched_keywords: Vec<String>,
        request_snippet: Option<String>,
        created_at: i64,
    ) -> oneshot::Receiver<ReviewDecision> {
        let (tx, rx) = oneshot::channel();
        let entry = PendingReviewEntry {
            tx,
            cli_key,
            matched_keywords,
            request_snippet,
            created_at,
        };
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(trace_id, entry);
        rx
    }

    /// Signal a decision for a pending review.
    pub fn resolve(&self, trace_id: &str, decision: ReviewDecision) -> Result<(), String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map
            .remove(trace_id)
            .ok_or_else(|| format!("no pending review for trace_id={trace_id}"))?;
        // If the receiver is already dropped (client disconnected), send returns Err but we
        // don't treat that as a fatal error.
        let _ = entry.tx.send(decision);
        Ok(())
    }

    /// Remove a review entry (used on timeout cleanup).
    pub(super) fn remove(&self, trace_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(trace_id);
    }

    /// List all pending reviews (for the frontend).
    pub fn list_pending(&self) -> Vec<PendingReviewSnapshot> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .map(|(trace_id, entry)| PendingReviewSnapshot {
                trace_id: trace_id.clone(),
                cli_key: entry.cli_key.clone(),
                matched_keywords: entry.matched_keywords.clone(),
                request_snippet: entry.request_snippet.clone(),
                created_at: entry.created_at,
            })
            .collect()
    }
}

// ── Event payload ──

pub(super) const KEYWORD_REVIEW_EVENT_NAME: &str = "gateway:keyword_review";

#[derive(Debug, Clone, Serialize)]
pub(super) struct KeywordReviewEvent {
    pub(super) trace_id: String,
    pub(super) cli_key: String,
    pub(super) matched_keywords: Vec<String>,
    pub(super) request_snippet: Option<String>,
    pub(super) created_at: i64,
}

// ── Check and Intercept ──

/// Check if the request content matches any configured keywords.
/// If a match is found, returns an SSE response that sends heartbeats while awaiting review.
/// If no match or feature disabled, returns None and the caller should proceed normally.
#[allow(clippy::too_many_arguments)]
pub(super) async fn check_and_intercept(
    state: &GatewayAppState,
    trace_id: &str,
    cli_key: &str,
    introspection_json: Option<&serde_json::Value>,
    session_id: Option<&str>,
    created_at: i64,
) -> Option<axum::response::Response> {
    // Load enabled keywords from DB.
    let conn = match state.db.open_connection() {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!("keyword review: failed to open DB connection: {err}");
            return None;
        }
    };

    let keywords = match domain::keywords_list_enabled(&conn) {
        Ok(kws) => kws,
        Err(err) => {
            tracing::warn!("keyword review: failed to load keywords: {err}");
            return None;
        }
    };

    if keywords.is_empty() {
        return None;
    }

    // Extract searchable content and match.
    let searchable = domain::extract_searchable_content(introspection_json);
    let matched = domain::match_keywords(&searchable, &keywords);

    if matched.is_empty() {
        return None;
    }

    // Content snippet for review (first 500 chars).
    let snippet: String = searchable.chars().take(500).collect();
    let snippet_opt = if snippet.is_empty() {
        None
    } else {
        Some(snippet)
    };

    tracing::info!(
        trace_id = trace_id,
        cli_key = cli_key,
        matched = ?matched,
        "keyword review: request intercepted"
    );

    // Insert review log in DB.
    if let Err(err) = domain::review_log_insert(
        &conn,
        trace_id,
        cli_key,
        session_id,
        &matched,
        snippet_opt.as_deref(),
    ) {
        tracing::warn!("keyword review: failed to insert review log: {err}");
    }

    // Insert pending review in registry.
    let rx = state.keyword_review_registry.insert(
        trace_id.to_string(),
        cli_key.to_string(),
        matched.clone(),
        snippet_opt.clone(),
        created_at,
    );

    // Emit event to frontend.
    let event_payload = KeywordReviewEvent {
        trace_id: trace_id.to_string(),
        cli_key: cli_key.to_string(),
        matched_keywords: matched.clone(),
        request_snippet: snippet_opt.clone(),
        created_at,
    };
    crate::app::heartbeat_watchdog::gated_emit(
        &state.app,
        KEYWORD_REVIEW_EVENT_NAME,
        event_payload,
    );

    // Read timeout settings.
    let (timeout_secs, timeout_action) = match settings::read(&state.app) {
        Ok(cfg) => (
            cfg.keyword_review_timeout_seconds,
            cfg.keyword_review_timeout_action,
        ),
        Err(_) => (300, settings::KeywordReviewTimeoutAction::Reject),
    };

    // Build SSE heartbeat stream response.
    let response = build_review_sse_response(
        state.clone(),
        trace_id.to_string(),
        rx,
        timeout_secs,
        timeout_action,
    );

    Some(response)
}

/// Build an SSE response that sends heartbeat comments while waiting for a review decision.
fn build_review_sse_response(
    state: GatewayAppState,
    trace_id: String,
    rx: oneshot::Receiver<ReviewDecision>,
    timeout_secs: u32,
    timeout_action: settings::KeywordReviewTimeoutAction,
) -> axum::response::Response {
    let (body_tx, body_rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);

    // Spawn the heartbeat + decision loop.
    tokio::spawn(async move {
        review_stream_loop(state, trace_id, rx, body_tx, timeout_secs, timeout_action).await;
    });

    let body_stream = tokio_stream::wrappers::ReceiverStream::new(body_rx);
    let body = Body::from_stream(body_stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        )
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .header(
            "x-aio-intercepted",
            HeaderValue::from_static("keyword_review"),
        )
        .body(body)
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const HEARTBEAT_BYTES: &[u8] = b": aio-heartbeat\n\n";

async fn review_stream_loop(
    state: GatewayAppState,
    trace_id: String,
    mut rx: oneshot::Receiver<ReviewDecision>,
    tx: tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
    timeout_secs: u32,
    timeout_action: settings::KeywordReviewTimeoutAction,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs as u64);

    loop {
        let time_remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let sleep_duration = time_remaining.min(HEARTBEAT_INTERVAL);

        if sleep_duration.is_zero() {
            // Timeout reached.
            handle_timeout(&state, &trace_id, timeout_action, &tx).await;
            return;
        }

        tokio::select! {
            biased;

            result = &mut rx => {
                match result {
                    Ok(ReviewDecision::Approve) => {
                        handle_approved(&state, &trace_id, &tx).await;
                    }
                    Ok(ReviewDecision::Reject) | Err(_) => {
                        handle_rejected(&state, &trace_id, &tx).await;
                    }
                }
                return;
            }

            _ = tokio::time::sleep(sleep_duration) => {
                // Check if we've passed the deadline.
                if tokio::time::Instant::now() >= deadline {
                    handle_timeout(&state, &trace_id, timeout_action, &tx).await;
                    return;
                }

                // Send heartbeat.
                if tx.send(Ok(Bytes::from_static(HEARTBEAT_BYTES))).await.is_err() {
                    // Client disconnected.
                    tracing::info!(trace_id = %trace_id, "keyword review: client disconnected during review wait");
                    state.keyword_review_registry.remove(&trace_id);
                    update_review_status(&state, &trace_id, "rejected");
                    return;
                }
            }
        }
    }
}

async fn handle_approved(
    state: &GatewayAppState,
    trace_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    tracing::info!(trace_id = %trace_id, "keyword review: approved");
    update_review_status(state, trace_id, "approved");

    // Send an SSE comment indicating approval, then close the stream.
    // The CLI will need to retry the request, which will now pass through without
    // interception (the review log status is no longer pending).
    //
    // Note: For true seamless forwarding, we would need to reconstruct the full
    // RequestContext and call forwarder::forward(), then pipe its response body
    // through this stream. However, this is complex because the RequestContext
    // requires all the data that proxy_impl computes. Instead, we send a
    // standardized error that the CLI will retry automatically.
    let approved_event = b"data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Request approved, retrying...\"}}\n\n";
    let _ = tx.send(Ok(Bytes::from_static(approved_event))).await;
}

async fn handle_rejected(
    state: &GatewayAppState,
    trace_id: &str,
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    tracing::info!(trace_id = %trace_id, "keyword review: rejected");
    update_review_status(state, trace_id, "rejected");

    let error_event = b"data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Request rejected by keyword review.\"}}\n\n";
    let _ = tx.send(Ok(Bytes::from_static(error_event))).await;
}

async fn handle_timeout(
    state: &GatewayAppState,
    trace_id: &str,
    timeout_action: settings::KeywordReviewTimeoutAction,
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    state.keyword_review_registry.remove(trace_id);

    match timeout_action {
        settings::KeywordReviewTimeoutAction::Approve => {
            tracing::info!(trace_id = %trace_id, "keyword review: timeout -> auto-approve");
            handle_approved(state, trace_id, tx).await;
        }
        settings::KeywordReviewTimeoutAction::Reject => {
            tracing::info!(trace_id = %trace_id, "keyword review: timeout -> auto-reject");
            update_review_status(state, trace_id, "timeout");

            let error_event = b"data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Request rejected: keyword review timed out.\"}}\n\n";
            let _ = tx.send(Ok(Bytes::from_static(error_event))).await;
        }
    }
}

fn update_review_status(state: &GatewayAppState, trace_id: &str, status: &str) {
    if let Ok(conn) = state.db.open_connection() {
        if let Err(err) = domain::review_log_update_status(&conn, trace_id, status) {
            tracing::warn!(
                trace_id = %trace_id,
                "keyword review: failed to update review log status to {status}: {err}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_review_registry_insert_and_resolve() {
        let registry = PendingReviewRegistry::new();
        let mut rx = registry.insert(
            "trace-1".to_string(),
            "claude".to_string(),
            vec!["password".to_string()],
            Some("snippet".to_string()),
            1000,
        );

        assert_eq!(registry.list_pending().len(), 1);

        registry
            .resolve("trace-1", ReviewDecision::Approve)
            .unwrap();

        let decision = rx.try_recv().unwrap();
        assert_eq!(decision, ReviewDecision::Approve);
        assert_eq!(registry.list_pending().len(), 0);
    }

    #[test]
    fn pending_review_registry_resolve_missing_returns_err() {
        let registry = PendingReviewRegistry::new();
        let result = registry.resolve("nonexistent", ReviewDecision::Reject);
        assert!(result.is_err());
    }

    #[test]
    fn pending_review_registry_remove() {
        let registry = PendingReviewRegistry::new();
        let _rx = registry.insert(
            "trace-2".to_string(),
            "codex".to_string(),
            vec!["secret".to_string()],
            None,
            2000,
        );

        assert_eq!(registry.list_pending().len(), 1);
        registry.remove("trace-2");
        assert_eq!(registry.list_pending().len(), 0);
    }

    #[test]
    fn pending_review_registry_list_pending_returns_snapshots() {
        let registry = PendingReviewRegistry::new();
        let _rx1 = registry.insert(
            "t1".to_string(),
            "claude".to_string(),
            vec!["kw1".to_string()],
            Some("s1".to_string()),
            100,
        );
        let _rx2 = registry.insert(
            "t2".to_string(),
            "codex".to_string(),
            vec!["kw2".to_string()],
            None,
            200,
        );

        let pending = registry.list_pending();
        assert_eq!(pending.len(), 2);
    }
}
