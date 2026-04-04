//! Usage: Tauri IPC commands for keyword review feature.

use crate::app_state::{ensure_db_ready, DbInitState, GatewayState};
use crate::domain::keyword_review as domain;
use crate::gateway::{PendingReviewSnapshot, ReviewDecision};
use crate::shared::blocking;
use crate::shared::mutex_ext::MutexExt;

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_keywords_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<Vec<domain::KeywordEntry>, String> {
    let db = ensure_db_ready(app, &db_state).await?;
    blocking::run("keyword_review_keywords_list", move || {
        domain::keywords_list(&db)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_keyword_add(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    keyword: String,
) -> Result<domain::KeywordEntry, String> {
    let db = ensure_db_ready(app, &db_state).await?;
    blocking::run("keyword_review_keyword_add", move || {
        domain::keyword_add(&db, &keyword)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_keyword_set_enabled(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
    enabled: bool,
) -> Result<domain::KeywordEntry, String> {
    let db = ensure_db_ready(app, &db_state).await?;
    blocking::run("keyword_review_keyword_set_enabled", move || {
        domain::keyword_set_enabled(&db, id, enabled)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_keyword_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
) -> Result<bool, String> {
    let db = ensure_db_ready(app, &db_state).await?;
    blocking::run("keyword_review_keyword_delete", move || {
        domain::keyword_delete(&db, id)?;
        Ok::<bool, crate::shared::error::AppError>(true)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_logs_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    limit: i64,
    offset: i64,
) -> Result<Vec<domain::KeywordReviewLog>, String> {
    let db = ensure_db_ready(app, &db_state).await?;
    blocking::run("keyword_review_logs_list", move || {
        domain::review_logs_list(&db, limit, offset)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_decide(
    state: tauri::State<'_, GatewayState>,
    trace_id: String,
    decision: String,
    allow_session: Option<bool>,
) -> Result<bool, String> {
    let review_decision = match decision.as_str() {
        "approve" => ReviewDecision::Approve,
        "reject" => ReviewDecision::Reject,
        other => {
            return Err(format!(
                "invalid decision: {other}, expected 'approve' or 'reject'"
            ))
        }
    };

    let manager = state.0.lock_or_recover();
    let registry = manager
        .keyword_review_registry()
        .ok_or_else(|| "gateway is not running".to_string())?;

    if allow_session.unwrap_or(false) && review_decision == ReviewDecision::Approve {
        registry.resolve_and_allow_session(&trace_id, review_decision)?;
    } else {
        registry.resolve(&trace_id, review_decision)?;
    }
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn keyword_review_pending_list(
    state: tauri::State<'_, GatewayState>,
) -> Result<Vec<PendingReviewSnapshot>, String> {
    let manager = state.0.lock_or_recover();
    let registry = match manager.keyword_review_registry() {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    Ok(registry.list_pending())
}
