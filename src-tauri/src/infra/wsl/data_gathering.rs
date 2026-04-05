//! Data gathering functions: collect MCP, prompt, and skills sync data from the database.

use crate::mcp_sync::McpServerForSync;
use crate::shared::error::AppResult;
use rusqlite::OptionalExtension;

use super::mcp_adapt::adapt_mcp_servers_for_wsl;
use super::skills_sync::export_wsl_skill_dir;
use super::types::*;

/// Gather MCP sync data from the database for all CLIs.
pub fn gather_mcp_sync_data(conn: &rusqlite::Connection) -> AppResult<WslMcpSyncData> {
    let gather_for_cli = |cli_key: &str| -> AppResult<Vec<McpServerForSync>> {
        let servers = crate::mcp::list_enabled_for_cli(conn, cli_key)?;
        Ok(adapt_mcp_servers_for_wsl(&servers))
    };

    Ok(WslMcpSyncData {
        claude: gather_for_cli("claude")?,
        codex: gather_for_cli("codex")?,
        gemini: gather_for_cli("gemini")?,
    })
}

/// Gather prompt sync data from the database for all CLIs.
pub fn gather_prompt_sync_data(conn: &rusqlite::Connection) -> AppResult<WslPromptSyncData> {
    let get_for_cli = |cli_key: &str| -> AppResult<Option<String>> {
        let Some(workspace_id) = crate::workspaces::active_id_by_cli(conn, cli_key)? else {
            return Ok(None);
        };
        let content: Option<String> = conn
            .query_row(
                r#"
SELECT content
FROM prompts
WHERE workspace_id = ?1 AND enabled = 1
ORDER BY updated_at DESC, id DESC
LIMIT 1
"#,
                rusqlite::params![workspace_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("DB_ERROR: failed to query enabled prompt for {cli_key}: {e}"))?;
        Ok(content)
    };

    Ok(WslPromptSyncData {
        claude_content: get_for_cli("claude")?,
        codex_content: get_for_cli("codex")?,
        gemini_content: get_for_cli("gemini")?,
    })
}

/// Gather skills sync data from the database and SSOT files for all CLIs.
pub fn gather_skills_sync_data<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    conn: &rusqlite::Connection,
) -> AppResult<WslSkillsSyncData> {
    let ssot_root = crate::app_paths::app_data_dir(app)?.join("skills");

    let get_for_cli = |cli_key: &str| -> AppResult<Vec<WslSkillSyncEntry>> {
        let Some(workspace_id) = crate::workspaces::active_id_by_cli(conn, cli_key)? else {
            return Ok(Vec::new());
        };

        let mut stmt = conn
            .prepare_cached(
                r#"
SELECT s.skill_key
FROM skills s
JOIN workspace_skill_enabled e
  ON e.skill_id = s.id
WHERE e.workspace_id = ?1
ORDER BY s.skill_key ASC
"#,
            )
            .map_err(|e| format!("DB_ERROR: failed to prepare enabled skills query: {e}"))?;
        let rows = stmt
            .query_map(rusqlite::params![workspace_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| format!("DB_ERROR: failed to query enabled skills: {e}"))?;

        let mut items = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let skill_key =
                row.map_err(|e| format!("DB_ERROR: failed to read enabled skill row: {e}"))?;
            if !seen.insert(skill_key.clone()) {
                continue;
            }

            let skill_dir = ssot_root.join(&skill_key);
            if !skill_dir.is_dir() {
                return Err(format!(
                    "WSL_SKILL_SYNC_MISSING_SSOT_DIR: missing installed skill dir {}",
                    skill_dir.display()
                )
                .into());
            }

            items.push(WslSkillSyncEntry {
                skill_key,
                files: export_wsl_skill_dir(&skill_dir)?,
            });
        }

        Ok(items)
    };

    Ok(WslSkillsSyncData {
        claude: get_for_cli("claude")?,
        codex: get_for_cli("codex")?,
        gemini: get_for_cli("gemini")?,
    })
}
