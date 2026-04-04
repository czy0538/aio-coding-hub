//! Usage: Keyword review domain - keyword CRUD, review log management, and content matching.

use crate::db;
use crate::shared::error::db_err;
use crate::shared::time::now_unix_seconds;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ── Types ──

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct KeywordEntry {
    pub id: i64,
    pub keyword: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct KeywordReviewLog {
    pub id: i64,
    pub trace_id: String,
    pub cli_key: String,
    pub session_id: Option<String>,
    pub matched_keywords: Vec<String>,
    pub request_snippet: Option<String>,
    pub status: String,
    pub reviewer_action_at: Option<i64>,
    pub created_at: i64,
}

// ── Row mappers ──

fn row_to_keyword(row: &rusqlite::Row<'_>) -> Result<KeywordEntry, rusqlite::Error> {
    Ok(KeywordEntry {
        id: row.get("id")?,
        keyword: row.get("keyword")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_review_log(row: &rusqlite::Row<'_>) -> Result<KeywordReviewLog, rusqlite::Error> {
    let matched_json: String = row.get("matched_keywords")?;
    let matched: Vec<String> =
        serde_json::from_str(&matched_json).unwrap_or_else(|_| vec![matched_json.clone()]);
    Ok(KeywordReviewLog {
        id: row.get("id")?,
        trace_id: row.get("trace_id")?,
        cli_key: row.get("cli_key")?,
        session_id: row.get("session_id")?,
        matched_keywords: matched,
        request_snippet: row.get("request_snippet")?,
        status: row.get("status")?,
        reviewer_action_at: row.get("reviewer_action_at")?,
        created_at: row.get("created_at")?,
    })
}

// ── Keyword CRUD ──

pub fn keywords_list(db: &db::Db) -> crate::shared::error::AppResult<Vec<KeywordEntry>> {
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, keyword, enabled, created_at, updated_at FROM keyword_review_keywords ORDER BY id DESC",
        )
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = stmt
        .query_map([], row_to_keyword)
        .map_err(|e| db_err!("failed to list keywords: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read keyword row: {e}"))?);
    }
    Ok(items)
}

pub fn keywords_list_enabled(
    conn: &Connection,
) -> crate::shared::error::AppResult<Vec<KeywordEntry>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, keyword, enabled, created_at, updated_at FROM keyword_review_keywords WHERE enabled = 1 ORDER BY id",
        )
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = stmt
        .query_map([], row_to_keyword)
        .map_err(|e| db_err!("failed to list enabled keywords: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read keyword row: {e}"))?);
    }
    Ok(items)
}

pub fn keyword_add(db: &db::Db, keyword: &str) -> crate::shared::error::AppResult<KeywordEntry> {
    let normalized = keyword.trim().to_lowercase();
    if normalized.is_empty() {
        return Err("SEC_INVALID_INPUT: keyword is required".to_string().into());
    }

    let conn = db.open_connection()?;
    let now = now_unix_seconds();

    conn.execute(
        "INSERT INTO keyword_review_keywords (keyword, enabled, created_at, updated_at) VALUES (?1, 1, ?2, ?2)",
        params![normalized, now],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            crate::shared::error::AppError::new(
                "DB_CONSTRAINT",
                format!("keyword already exists: {normalized}"),
            )
        }
        other => db_err!("failed to insert keyword: {other}"),
    })?;

    let id = conn.last_insert_rowid();
    conn.query_row(
        "SELECT id, keyword, enabled, created_at, updated_at FROM keyword_review_keywords WHERE id = ?1",
        params![id],
        row_to_keyword,
    )
    .map_err(|e| db_err!("failed to read inserted keyword: {e}"))
}

pub fn keyword_set_enabled(
    db: &db::Db,
    id: i64,
    enabled: bool,
) -> crate::shared::error::AppResult<KeywordEntry> {
    let conn = db.open_connection()?;
    let now = now_unix_seconds();
    let enabled_int: i64 = if enabled { 1 } else { 0 };

    let changed = conn
        .execute(
            "UPDATE keyword_review_keywords SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![enabled_int, now, id],
        )
        .map_err(|e| db_err!("failed to update keyword: {e}"))?;

    if changed == 0 {
        return Err("DB_NOT_FOUND: keyword not found".to_string().into());
    }

    conn.query_row(
        "SELECT id, keyword, enabled, created_at, updated_at FROM keyword_review_keywords WHERE id = ?1",
        params![id],
        row_to_keyword,
    )
    .map_err(|e| db_err!("failed to read updated keyword: {e}"))
}

pub fn keyword_delete(db: &db::Db, id: i64) -> crate::shared::error::AppResult<()> {
    let conn = db.open_connection()?;
    let changed = conn
        .execute(
            "DELETE FROM keyword_review_keywords WHERE id = ?1",
            params![id],
        )
        .map_err(|e| db_err!("failed to delete keyword: {e}"))?;

    if changed == 0 {
        return Err("DB_NOT_FOUND: keyword not found".to_string().into());
    }
    Ok(())
}

// ── Review Logs ──

pub fn review_log_insert(
    conn: &Connection,
    trace_id: &str,
    cli_key: &str,
    session_id: Option<&str>,
    matched_keywords: &[String],
    request_snippet: Option<&str>,
) -> crate::shared::error::AppResult<i64> {
    let now = now_unix_seconds();
    let matched_json = serde_json::to_string(matched_keywords).unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        r#"
INSERT INTO keyword_review_logs (trace_id, cli_key, session_id, matched_keywords, request_snippet, status, created_at)
VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)
"#,
        params![trace_id, cli_key, session_id, matched_json, request_snippet, now],
    )
    .map_err(|e| db_err!("failed to insert review log: {e}"))?;

    Ok(conn.last_insert_rowid())
}

pub fn review_log_update_status(
    conn: &Connection,
    trace_id: &str,
    status: &str,
) -> crate::shared::error::AppResult<()> {
    let now = now_unix_seconds();
    conn.execute(
        "UPDATE keyword_review_logs SET status = ?1, reviewer_action_at = ?2 WHERE trace_id = ?3 AND status = 'pending'",
        params![status, now, trace_id],
    )
    .map_err(|e| db_err!("failed to update review log status: {e}"))?;
    Ok(())
}

pub fn review_logs_list(
    db: &db::Db,
    limit: i64,
    offset: i64,
) -> crate::shared::error::AppResult<Vec<KeywordReviewLog>> {
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare_cached(
            r#"
SELECT id, trace_id, cli_key, session_id, matched_keywords, request_snippet, status, reviewer_action_at, created_at
FROM keyword_review_logs
ORDER BY id DESC
LIMIT ?1 OFFSET ?2
"#,
        )
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = stmt
        .query_map(params![limit, offset], row_to_review_log)
        .map_err(|e| db_err!("failed to list review logs: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read review log row: {e}"))?);
    }
    Ok(items)
}

/// Mark stale pending reviews (older than `cutoff_unix`) as "timeout".
pub fn review_log_timeout_stale(
    conn: &Connection,
    cutoff_unix: i64,
) -> crate::shared::error::AppResult<usize> {
    let now = now_unix_seconds();
    let changed = conn
        .execute(
            "UPDATE keyword_review_logs SET status = 'timeout', reviewer_action_at = ?1 WHERE status = 'pending' AND created_at < ?2",
            params![now, cutoff_unix],
        )
        .map_err(|e| db_err!("failed to timeout stale review logs: {e}"))?;
    Ok(changed)
}

// ── Matching Engine ──

/// Match content against enabled keywords (case-insensitive substring match).
/// Returns the list of matched keyword strings.
pub fn match_keywords(content: &str, keywords: &[KeywordEntry]) -> Vec<String> {
    if content.is_empty() || keywords.is_empty() {
        return Vec::new();
    }

    let content_lower = content.to_lowercase();
    let mut matched = Vec::new();

    for kw in keywords {
        if !kw.enabled {
            continue;
        }
        if content_lower.contains(&kw.keyword) {
            matched.push(kw.keyword.clone());
        }
    }

    matched.dedup();
    matched
}

/// Extract searchable text content from a request body JSON.
///
/// Supports two API formats:
/// - **Anthropic Messages API**: `system` + `messages[role=user].content`
/// - **OpenAI Responses API**: `instructions` + `input` (string or message array with `input_text` blocks)
pub fn extract_searchable_content(json: Option<&serde_json::Value>) -> String {
    let Some(root) = json else {
        return String::new();
    };

    let mut parts: Vec<&str> = Vec::new();

    // ── Anthropic Messages API ──

    // Extract system prompt (string or content blocks)
    if let Some(system) = root.get("system") {
        extract_text_from_content(system, &mut parts);
    }

    // Extract user messages
    if let Some(messages) = root.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "user" {
                if let Some(content) = msg.get("content") {
                    extract_text_from_content(content, &mut parts);
                }
            }
        }
    }

    // ── OpenAI Responses API ──

    // Extract instructions (system prompt equivalent)
    if let Some(instructions) = root.get("instructions").and_then(|v| v.as_str()) {
        parts.push(instructions);
    }

    // Extract input (string or array of message items)
    if let Some(input) = root.get("input") {
        extract_openai_input(input, &mut parts);
    }

    parts.join("\n")
}

fn extract_openai_input<'a>(value: &'a serde_json::Value, parts: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s) => {
            parts.push(s.as_str());
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                // Each item can be a message object with role + content
                if let Some(content) = item.get("content") {
                    extract_openai_content(content, parts);
                }
                // Or a simple text string
                if let Some(text) = item.as_str() {
                    parts.push(text);
                }
            }
        }
        _ => {}
    }
}

fn extract_openai_content<'a>(value: &'a serde_json::Value, parts: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s) => {
            parts.push(s.as_str());
        }
        serde_json::Value::Array(arr) => {
            for block in arr {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match block_type {
                    "input_text" | "text" => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            parts.push(text);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn extract_text_from_content<'a>(value: &'a serde_json::Value, parts: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s) => {
            parts.push(s.as_str());
        }
        serde_json::Value::Array(arr) => {
            for block in arr {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            parts.push(text);
                        }
                    }
                    "document" | "file" => {
                        // Extract document/file content if available as inline text
                        if let Some(source) = block.get("source") {
                            if let Some(data) = source.get("data").and_then(|v| v.as_str()) {
                                parts.push(data);
                            }
                        }
                        if let Some(content) = block.get("content") {
                            extract_text_from_content(content, parts);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_keywords_case_insensitive() {
        let keywords = vec![KeywordEntry {
            id: 1,
            keyword: "password".to_string(),
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }];

        let matched = match_keywords("My PASSWORD is secret", &keywords);
        assert_eq!(matched, vec!["password"]);
    }

    #[test]
    fn match_keywords_no_match() {
        let keywords = vec![KeywordEntry {
            id: 1,
            keyword: "secret".to_string(),
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }];

        let matched = match_keywords("Hello world", &keywords);
        assert!(matched.is_empty());
    }

    #[test]
    fn match_keywords_disabled_ignored() {
        let keywords = vec![KeywordEntry {
            id: 1,
            keyword: "password".to_string(),
            enabled: false,
            created_at: 0,
            updated_at: 0,
        }];

        let matched = match_keywords("my password is here", &keywords);
        assert!(matched.is_empty());
    }

    #[test]
    fn match_keywords_multiple() {
        let keywords = vec![
            KeywordEntry {
                id: 1,
                keyword: "password".to_string(),
                enabled: true,
                created_at: 0,
                updated_at: 0,
            },
            KeywordEntry {
                id: 2,
                keyword: "secret".to_string(),
                enabled: true,
                created_at: 0,
                updated_at: 0,
            },
        ];

        let matched = match_keywords("my password is a secret", &keywords);
        assert_eq!(matched, vec!["password", "secret"]);
    }

    #[test]
    fn match_keywords_empty_content() {
        let keywords = vec![KeywordEntry {
            id: 1,
            keyword: "test".to_string(),
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }];

        let matched = match_keywords("", &keywords);
        assert!(matched.is_empty());
    }

    #[test]
    fn extract_searchable_content_string_system() {
        let json = serde_json::json!({
            "system": "You are a helpful assistant",
            "messages": [
                {"role": "user", "content": "Hello world"}
            ]
        });

        let content = extract_searchable_content(Some(&json));
        assert!(content.contains("You are a helpful assistant"));
        assert!(content.contains("Hello world"));
    }

    #[test]
    fn extract_searchable_content_block_system() {
        let json = serde_json::json!({
            "system": [
                {"type": "text", "text": "System prompt here"}
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "User message here"}
                    ]
                }
            ]
        });

        let content = extract_searchable_content(Some(&json));
        assert!(content.contains("System prompt here"));
        assert!(content.contains("User message here"));
    }

    #[test]
    fn extract_searchable_content_ignores_assistant() {
        let json = serde_json::json!({
            "messages": [
                {"role": "user", "content": "user text"},
                {"role": "assistant", "content": "assistant text"},
                {"role": "user", "content": "more user text"}
            ]
        });

        let content = extract_searchable_content(Some(&json));
        assert!(content.contains("user text"));
        assert!(content.contains("more user text"));
        assert!(!content.contains("assistant text"));
    }

    #[test]
    fn extract_searchable_content_none() {
        let content = extract_searchable_content(None);
        assert!(content.is_empty());
    }

    // ── OpenAI Responses API tests ──

    #[test]
    fn extract_searchable_content_openai_string_input() {
        let json = serde_json::json!({
            "model": "gpt-4",
            "input": "tell me the 敏感词",
            "instructions": "You are helpful"
        });

        let content = extract_searchable_content(Some(&json));
        assert!(content.contains("tell me the 敏感词"));
        assert!(content.contains("You are helpful"));
    }

    #[test]
    fn extract_searchable_content_openai_input_text_blocks() {
        let json = serde_json::json!({
            "model": "gpt-4",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "message with 敏感词"}
                    ]
                }
            ],
            "instructions": "system prompt here"
        });

        let content = extract_searchable_content(Some(&json));
        assert!(content.contains("message with 敏感词"));
        assert!(content.contains("system prompt here"));
    }

    #[test]
    fn extract_searchable_content_openai_string_content() {
        let json = serde_json::json!({
            "input": [
                {"role": "user", "content": "plain string content"}
            ]
        });

        let content = extract_searchable_content(Some(&json));
        assert!(content.contains("plain string content"));
    }
}
