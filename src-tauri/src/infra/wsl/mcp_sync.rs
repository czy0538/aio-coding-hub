//! WSL MCP sync: sync MCP server configurations to WSL distros.

use crate::mcp_sync::McpServerForSync;
use crate::shared::error::AppResult;

use super::shell::{
    bash_single_quote, read_wsl_file, run_wsl_bash_script_capture, write_wsl_file,
    wsl_resolve_codex_home_script,
};

// ── WSL MCP manifest ──

/// Tracks which MCP server keys were synced to a WSL distro for a specific CLI,
/// so we can properly remove them on the next sync.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WslMcpManifest {
    distro: String,
    cli_key: String,
    managed_keys: Vec<String>,
    updated_at: i64,
}

fn wsl_mcp_manifest_path(
    app: &tauri::AppHandle,
    distro: &str,
    cli_key: &str,
) -> AppResult<std::path::PathBuf> {
    let dir = crate::app_paths::app_data_dir(app)?
        .join("wsl-mcp-sync")
        .join(distro)
        .join(cli_key);
    Ok(dir.join("manifest.json"))
}

pub(super) fn read_wsl_mcp_manifest(
    app: &tauri::AppHandle,
    distro: &str,
    cli_key: &str,
) -> Vec<String> {
    let path = match wsl_mcp_manifest_path(app, distro, cli_key) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_slice::<WslMcpManifest>(&bytes) {
        Ok(m) => m.managed_keys,
        Err(_) => Vec::new(),
    }
}

pub(super) fn write_wsl_mcp_manifest(
    app: &tauri::AppHandle,
    distro: &str,
    cli_key: &str,
    managed_keys: &[String],
) -> AppResult<()> {
    let path = wsl_mcp_manifest_path(app, distro, cli_key)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create wsl-mcp-sync dir: {e}"))?;
    }
    let manifest = WslMcpManifest {
        distro: distro.to_string(),
        cli_key: cli_key.to_string(),
        managed_keys: managed_keys.to_vec(),
        updated_at: crate::shared::time::now_unix_seconds(),
    };
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("failed to serialize wsl mcp manifest: {e}"))?;
    std::fs::write(&path, json.as_bytes())
        .map_err(|e| format!("failed to write wsl mcp manifest: {e}"))?;
    Ok(())
}

// ── WSL MCP sync ──

/// Sync MCP configuration for a single CLI to a WSL distro.
/// Uses the existing `build_next_bytes` to merge servers into the config.
pub(super) fn sync_wsl_mcp_for_cli(
    distro: &str,
    cli_key: &str,
    servers: &[McpServerForSync],
    managed_keys: &[String],
) -> AppResult<Vec<String>> {
    if !matches!(cli_key, "claude" | "codex" | "gemini") {
        return Err(format!("unknown cli_key: {cli_key}").into());
    }

    // Resolve the config file path inside WSL (handles CODEX_HOME, etc.)
    let resolve_script = format!(
        r#"
set -euo pipefail
HOME="$(getent passwd "$(whoami)" | cut -d: -f6)"
export HOME
{resolver}
case {cli_key} in
  claude) echo "$HOME/.claude.json" ;;
  codex) echo "$p/config.toml" ;;
  gemini) echo "$HOME/.gemini/settings.json" ;;
esac
"#,
        resolver = wsl_resolve_codex_home_script("p"),
        cli_key = bash_single_quote(cli_key)
    );

    let resolved_path = run_wsl_bash_script_capture(distro, &resolve_script)?;
    let resolved_path = resolved_path.trim();

    // Read current config from WSL
    let current = read_wsl_file(distro, resolved_path)?;

    // Build merged config using existing infrastructure
    let next_bytes = crate::mcp_sync::build_next_bytes(cli_key, current, managed_keys, servers)
        .map_err(|e| format!("WSL MCP build failed for {cli_key}: {e}"))?;

    // Write back to WSL
    write_wsl_file(distro, resolved_path, &next_bytes)?;

    // Return list of keys we now manage
    let mut keys: Vec<String> = servers.iter().map(|s| s.server_key.clone()).collect();
    keys.sort();
    keys.dedup();
    Ok(keys)
}
