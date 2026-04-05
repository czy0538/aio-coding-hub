//! Usage: Keyword review interception layer for the gateway proxy.
//!
//! When enabled, incoming requests are checked against a configured keyword list.
//! If sensitive keywords are found, the request is held with SSE comment heartbeats
//! until a human reviewer approves or rejects it via the desktop UI.
//!
//! On approval, the original request is replayed through the gateway via a loopback
//! HTTP request (with a bypass header to skip re-checking), and the upstream response
//! is piped back through the same SSE connection.

use crate::domain::keyword_review as domain;
use crate::gateway::manager::GatewayAppState;
use crate::settings;
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, HeaderValue, Method, Response, StatusCode};
use http_body_util::BodyExt;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

/// Header used to bypass keyword review on loopback requests after approval.
pub(super) const BYPASS_HEADER: &str = "x-aio-keyword-review-bypass";

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
    pub session_id: Option<String>,
    pub matched_keywords: Vec<String>,
    pub request_snippet: Option<String>,
    pub keyword_evidence: Option<Vec<domain::KeywordEvidenceSnippet>>,
    pub created_at: i64,
}

struct PendingReviewEntry {
    tx: oneshot::Sender<ReviewDecision>,
    cli_key: String,
    session_id: Option<String>,
    matched_keywords: Vec<String>,
    request_snippet: Option<String>,
    keyword_evidence: Option<Vec<domain::KeywordEvidenceSnippet>>,
    created_at: i64,
}

/// In-memory registry of pending reviews, keyed by trace_id.
/// Also maintains a session allowlist for "allow this conversation" approvals.
pub struct PendingReviewRegistry {
    inner: Mutex<HashMap<String, PendingReviewEntry>>,
    /// Sessions that have been approved with "allow this conversation".
    /// Key: "{cli_key}:{session_id}"
    allowed_sessions: Mutex<HashSet<String>>,
}

impl PendingReviewRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            allowed_sessions: Mutex::new(HashSet::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn insert(
        &self,
        trace_id: String,
        cli_key: String,
        session_id: Option<String>,
        matched_keywords: Vec<String>,
        request_snippet: Option<String>,
        keyword_evidence: Option<Vec<domain::KeywordEvidenceSnippet>>,
        created_at: i64,
    ) -> oneshot::Receiver<ReviewDecision> {
        let (tx, rx) = oneshot::channel();
        let entry = PendingReviewEntry {
            tx,
            cli_key,
            session_id,
            matched_keywords,
            request_snippet,
            keyword_evidence,
            created_at,
        };
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(trace_id, entry);
        rx
    }

    pub fn resolve(&self, trace_id: &str, decision: ReviewDecision) -> Result<(), String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map
            .remove(trace_id)
            .ok_or_else(|| format!("no pending review for trace_id={trace_id}"))?;
        let _ = entry.tx.send(decision);
        Ok(())
    }

    /// Resolve and optionally allow the session for future requests.
    pub fn resolve_and_allow_session(
        &self,
        trace_id: &str,
        decision: ReviewDecision,
    ) -> Result<(), String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map
            .remove(trace_id)
            .ok_or_else(|| format!("no pending review for trace_id={trace_id}"))?;

        // Add session to allowlist if approved and session_id is available.
        if decision == ReviewDecision::Approve {
            if let Some(session_id) = &entry.session_id {
                let key = format!("{}:{}", entry.cli_key, session_id);
                let mut allowed = self
                    .allowed_sessions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                allowed.insert(key.clone());
                tracing::info!(
                    session_key = %key,
                    "keyword review: session added to allowlist"
                );
            }
        }

        let _ = entry.tx.send(decision);
        Ok(())
    }

    /// Check if a session is in the allowlist.
    pub(super) fn is_session_allowed(&self, cli_key: &str, session_id: &str) -> bool {
        let key = format!("{cli_key}:{session_id}");
        let allowed = self
            .allowed_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        allowed.contains(&key)
    }

    pub(super) fn remove(&self, trace_id: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(trace_id);
    }

    pub fn list_pending(&self) -> Vec<PendingReviewSnapshot> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .map(|(trace_id, entry)| PendingReviewSnapshot {
                trace_id: trace_id.clone(),
                cli_key: entry.cli_key.clone(),
                session_id: entry.session_id.clone(),
                matched_keywords: entry.matched_keywords.clone(),
                request_snippet: entry.request_snippet.clone(),
                keyword_evidence: entry.keyword_evidence.clone(),
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
    pub(super) keyword_evidence: Option<Vec<domain::KeywordEvidenceSnippet>>,
    pub(super) created_at: i64,
}

// ── Saved request context for loopback replay ──

/// Original request data saved for replay after approval.
pub(super) struct SavedRequest {
    pub(super) method: Method,
    pub(super) cli_key: String,
    pub(super) forwarded_path: String,
    pub(super) query: Option<String>,
    pub(super) headers: HeaderMap,
    pub(super) body_bytes: Bytes,
}

// ── Session ID extraction ──

/// Extract a session identifier from request headers/body for the allowlist feature.
fn extract_session_id_for_review(
    headers: &HeaderMap,
    json: Option<&serde_json::Value>,
) -> Option<String> {
    // Check common session ID headers (Claude Code, Codex).
    for header_name in ["session_id", "x-session-id"] {
        if let Some(value) = headers.get(header_name).and_then(|v| v.to_str().ok()) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    // Check JSON body for session-related fields (OpenAI Responses API).
    if let Some(root) = json {
        for field in ["previous_response_id", "conversation_id", "thread_id"] {
            if let Some(id) = root.get(field).and_then(|v| v.as_str()) {
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
    }

    None
}

// ── Check and Intercept ──

/// Check if the request content matches any configured keywords.
/// If a match is found, returns an SSE response that sends heartbeats while awaiting review.
/// On approval, replays the original request via a loopback to the gateway.
#[allow(clippy::too_many_arguments)]
pub(super) async fn check_and_intercept(
    state: &GatewayAppState,
    trace_id: &str,
    cli_key: &str,
    introspection_json: Option<&serde_json::Value>,
    session_id: Option<&str>,
    created_at: i64,
    saved_request: SavedRequest,
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
        tracing::debug!("keyword review: no enabled keywords configured, skipping");
        return None;
    }

    // Extract session_id for allowlist check.
    let session_id_for_review =
        extract_session_id_for_review(&saved_request.headers, introspection_json);

    // Check if this session has been previously allowed ("allow this conversation").
    if let Some(ref sid) = session_id_for_review {
        if state
            .keyword_review_registry
            .is_session_allowed(cli_key, sid)
        {
            tracing::info!(
                trace_id = %trace_id,
                session_id = %sid,
                "keyword review: session in allowlist, skipping review"
            );
            return None;
        }
    }

    let searchable = domain::extract_searchable_content(introspection_json);

    tracing::info!(
        trace_id = %trace_id,
        keyword_count = keywords.len(),
        searchable_len = searchable.len(),
        searchable_preview = %searchable.chars().take(200).collect::<String>(),
        "keyword review: scanning content"
    );

    let matched = domain::match_keywords(&searchable, &keywords);

    if matched.is_empty() {
        return None;
    }

    let keyword_evidence = domain::build_keyword_evidence(&searchable, &matched);
    let keyword_evidence_opt = if keyword_evidence.is_empty() {
        None
    } else {
        Some(keyword_evidence)
    };

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

    if let Err(err) = domain::review_log_insert(
        &conn,
        trace_id,
        cli_key,
        session_id,
        &matched,
        snippet_opt.as_deref(),
        keyword_evidence_opt.as_deref(),
    ) {
        tracing::warn!("keyword review: failed to insert review log: {err}");
    }

    let rx = state.keyword_review_registry.insert(
        trace_id.to_string(),
        cli_key.to_string(),
        session_id_for_review,
        matched.clone(),
        snippet_opt.clone(),
        keyword_evidence_opt.clone(),
        created_at,
    );

    crate::app::heartbeat_watchdog::gated_emit(
        &state.app,
        KEYWORD_REVIEW_EVENT_NAME,
        KeywordReviewEvent {
            trace_id: trace_id.to_string(),
            cli_key: cli_key.to_string(),
            matched_keywords: matched,
            request_snippet: snippet_opt,
            keyword_evidence: keyword_evidence_opt,
            created_at,
        },
    );

    let (timeout_secs, timeout_action) = match settings::read(&state.app) {
        Ok(cfg) => (
            cfg.keyword_review_timeout_seconds,
            cfg.keyword_review_timeout_action,
        ),
        Err(_) => (300, settings::KeywordReviewTimeoutAction::Reject),
    };

    let response = build_review_sse_response(
        state.clone(),
        trace_id.to_string(),
        rx,
        timeout_secs,
        timeout_action,
        saved_request,
    );

    Some(response)
}

fn build_review_sse_response(
    state: GatewayAppState,
    trace_id: String,
    rx: oneshot::Receiver<ReviewDecision>,
    timeout_secs: u32,
    timeout_action: settings::KeywordReviewTimeoutAction,
    saved_request: SavedRequest,
) -> axum::response::Response {
    let (body_tx, body_rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);

    tokio::spawn(async move {
        review_stream_loop(
            state,
            trace_id,
            rx,
            body_tx,
            timeout_secs,
            timeout_action,
            saved_request,
        )
        .await;
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

#[allow(clippy::too_many_arguments)]
async fn review_stream_loop(
    state: GatewayAppState,
    trace_id: String,
    mut rx: oneshot::Receiver<ReviewDecision>,
    tx: tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
    timeout_secs: u32,
    timeout_action: settings::KeywordReviewTimeoutAction,
    saved_request: SavedRequest,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs as u64);

    loop {
        let time_remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let sleep_duration = time_remaining.min(HEARTBEAT_INTERVAL);

        if sleep_duration.is_zero() {
            handle_timeout(&state, &trace_id, timeout_action, &tx, saved_request).await;
            return;
        }

        tokio::select! {
            biased;

            result = &mut rx => {
                match result {
                    Ok(ReviewDecision::Approve) => {
                        handle_approved(&state, &trace_id, &tx, saved_request).await;
                    }
                    Ok(ReviewDecision::Reject) | Err(_) => {
                        handle_rejected(&state, &trace_id, &tx).await;
                    }
                }
                return;
            }

            _ = tokio::time::sleep(sleep_duration) => {
                if tokio::time::Instant::now() >= deadline {
                    handle_timeout(&state, &trace_id, timeout_action, &tx, saved_request).await;
                    return;
                }

                if tx.send(Ok(Bytes::from_static(HEARTBEAT_BYTES))).await.is_err() {
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
    saved_request: SavedRequest,
) {
    tracing::info!(trace_id = %trace_id, "keyword review: approved, replaying request through proxy");
    update_review_status(state, trace_id, "approved");

    // Reconstruct an axum Request and call proxy_impl directly.
    // This replays the original request through the full gateway pipeline
    // (provider selection, failover, upstream forwarding, response fixing, etc.)
    // without a network round-trip.
    let uri = match &saved_request.query {
        Some(q) if !q.is_empty() => format!(
            "/{}{forwarded}?{q}",
            saved_request.cli_key,
            forwarded = saved_request.forwarded_path,
        ),
        _ => format!(
            "/{}{forwarded}",
            saved_request.cli_key,
            forwarded = saved_request.forwarded_path,
        ),
    };

    let req_builder = axum::http::Request::builder()
        .method(saved_request.method)
        .uri(&uri);

    // Set headers on the builder (we can't set HeaderMap directly on builder,
    // so we build first then replace headers).
    let mut req = match req_builder.body(Body::from(saved_request.body_bytes)) {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(trace_id = %trace_id, "keyword review: failed to build replay request: {err}");
            return;
        }
    };

    *req.headers_mut() = saved_request.headers;
    // Add bypass header so the replayed request skips keyword review.
    req.headers_mut()
        .insert(BYPASS_HEADER, HeaderValue::from_static("1"));

    // Call proxy_impl directly (it's pub(in crate::gateway)).
    let response = super::handler::proxy_impl(
        state.clone(),
        saved_request.cli_key,
        saved_request.forwarded_path,
        req,
    )
    .await;

    tracing::info!(
        trace_id = %trace_id,
        status = %response.status(),
        "keyword review: replay response received, piping to client"
    );

    // Pipe the response body through our SSE stream.
    let mut body = response.into_body();
    while let Some(frame_result) = body.frame().await {
        match frame_result {
            Ok(frame) => {
                if let Ok(data) = frame.into_data() {
                    if tx.send(Ok(data)).await.is_err() {
                        tracing::info!(trace_id = %trace_id, "keyword review: client disconnected during replay pipe");
                        return;
                    }
                }
            }
            Err(err) => {
                tracing::warn!(trace_id = %trace_id, "keyword review: replay stream error: {err}");
                return;
            }
        }
    }
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
    saved_request: SavedRequest,
) {
    state.keyword_review_registry.remove(trace_id);

    match timeout_action {
        settings::KeywordReviewTimeoutAction::Approve => {
            tracing::info!(trace_id = %trace_id, "keyword review: timeout -> auto-approve");
            handle_approved(state, trace_id, tx, saved_request).await;
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
            Some("sess-1".to_string()),
            vec!["password".to_string()],
            Some("snippet".to_string()),
            None,
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
            None,
            vec!["secret".to_string()],
            None,
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
            Some("sess-a".to_string()),
            vec!["kw1".to_string()],
            Some("s1".to_string()),
            None,
            100,
        );
        let _rx2 = registry.insert(
            "t2".to_string(),
            "codex".to_string(),
            None,
            vec!["kw2".to_string()],
            None,
            None,
            200,
        );

        let pending = registry.list_pending();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn resolve_and_allow_session_adds_to_allowlist() {
        let registry = PendingReviewRegistry::new();
        let _rx = registry.insert(
            "t3".to_string(),
            "claude".to_string(),
            Some("sess-x".to_string()),
            vec!["pw".to_string()],
            None,
            None,
            300,
        );

        assert!(!registry.is_session_allowed("claude", "sess-x"));
        registry
            .resolve_and_allow_session("t3", ReviewDecision::Approve)
            .unwrap();
        assert!(registry.is_session_allowed("claude", "sess-x"));
        assert!(!registry.is_session_allowed("codex", "sess-x"));
    }
}
