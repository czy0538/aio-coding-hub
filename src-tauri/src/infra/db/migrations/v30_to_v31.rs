//! Usage: SQLite migration v30->v31 - Add keyword review tables.

use crate::shared::time::now_unix_seconds;
use rusqlite::Connection;

pub(super) fn migrate_v30_to_v31(conn: &mut Connection) -> Result<(), String> {
    const VERSION: i64 = 31;
    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to start sqlite transaction: {e}"))?;

    // Create keyword_review_keywords table.
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS keyword_review_keywords (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    keyword TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_krk_keyword
    ON keyword_review_keywords(keyword);
"#,
    )
    .map_err(|e| format!("failed to create keyword_review_keywords table: {e}"))?;

    // Create keyword_review_logs table.
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS keyword_review_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trace_id TEXT NOT NULL,
    cli_key TEXT NOT NULL,
    session_id TEXT,
    matched_keywords TEXT NOT NULL,
    request_snippet TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    reviewer_action_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_krl_status
    ON keyword_review_logs(status);
CREATE INDEX IF NOT EXISTS idx_krl_created_at
    ON keyword_review_logs(created_at);
"#,
    )
    .map_err(|e| format!("failed to create keyword_review_logs table: {e}"))?;

    // Record migration.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)",
    )
    .map_err(|e| format!("failed to create schema_migrations table: {e}"))?;
    let now = now_unix_seconds();
    tx.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?, ?)",
        [VERSION, now],
    )
    .map_err(|e| format!("failed to insert schema_migrations row for v{VERSION}: {e}"))?;

    super::set_user_version(&tx, VERSION)?;

    tx.commit()
        .map_err(|e| format!("failed to commit sqlite transaction: {e}"))?;

    Ok(())
}
