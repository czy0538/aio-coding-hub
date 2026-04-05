//! WSL config status validation and the main configure_clients orchestrator.

use crate::settings;
use crate::shared::error::AppResult;
use std::process::Stdio;

use super::config_claude::configure_wsl_claude;
use super::config_codex::configure_wsl_codex;
use super::config_gemini::configure_wsl_gemini;
use super::constants::{
    WSL_CODEX_API_KEY, WSL_CODEX_PREFERRED_AUTH_METHOD, WSL_CODEX_PROVIDER_KEY,
};
use super::detection::resolve_wsl_home_unc;
use super::manifest::{read_wsl_current_values, read_wsl_manifest, write_wsl_manifest};
use super::mcp_sync::{read_wsl_mcp_manifest, sync_wsl_mcp_for_cli, write_wsl_mcp_manifest};
use super::prompt_sync::sync_wsl_prompt_for_cli;
use super::shell::{decode_utf16_le, hide_window_cmd, wsl_resolve_codex_home_script};
use super::skills_sync::{
    read_wsl_skills_manifest, sync_wsl_skills_for_cli, write_wsl_skills_manifest,
};
use super::types::*;

fn wsl_target_enabled(targets: &settings::WslTargetCli, cli_key: &str) -> bool {
    match cli_key {
        "claude" => targets.claude,
        "codex" => targets.codex,
        "gemini" => targets.gemini,
        _ => false,
    }
}

pub fn get_config_status(distros: &[String]) -> Vec<WslDistroConfigStatus> {
    if !cfg!(windows) {
        return Vec::new();
    }

    let status_script = format!(
        r#"
# Normalize HOME: Windows environment may inject HOME=C:\Users\...
home_from_getent="$(getent passwd "$(whoami)" | cut -d: -f6 2>/dev/null || true)"
if [ -n "$home_from_getent" ]; then
  HOME="$home_from_getent"
fi
export HOME

claude=0
codex=0
gemini=0
claude_mcp=0
codex_mcp=0
gemini_mcp=0
claude_prompt=0
codex_prompt=0
gemini_prompt=0

[ -f "$HOME/.claude/settings.json" ] && claude=1

{resolver}

[ -f "$p/config.toml" ] && codex=1
[ -f "$HOME/.gemini/.env" ] && gemini=1

# Check MCP: claude uses .claude.json mcpServers, codex uses config.toml mcp_servers, gemini uses settings.json mcpServers
if [ -f "$HOME/.claude.json" ] && command -v grep >/dev/null 2>&1; then
  grep -q '"mcpServers"' "$HOME/.claude.json" 2>/dev/null && claude_mcp=1
fi
if [ -f "$p/config.toml" ] && command -v grep >/dev/null 2>&1; then
  grep -q '\[mcp_servers\.' "$p/config.toml" 2>/dev/null && codex_mcp=1
fi
if [ -f "$HOME/.gemini/settings.json" ] && command -v grep >/dev/null 2>&1; then
  grep -q '"mcpServers"' "$HOME/.gemini/settings.json" 2>/dev/null && gemini_mcp=1
fi

# Check Prompt files
[ -f "$HOME/.claude/CLAUDE.md" ] && claude_prompt=1
[ -f "$p/AGENTS.md" ] && codex_prompt=1
[ -f "$HOME/.gemini/GEMINI.md" ] && gemini_prompt=1

printf 'AIO_WSL_STATUS=%s%s%s%s%s%s%s%s%s\n' "$claude" "$codex" "$gemini" "$claude_mcp" "$codex_mcp" "$gemini_mcp" "$claude_prompt" "$codex_prompt" "$gemini_prompt"
"#,
        resolver = wsl_resolve_codex_home_script("p")
    );

    #[derive(Default)]
    struct StatusBits {
        claude: bool,
        codex: bool,
        gemini: bool,
        claude_mcp: bool,
        codex_mcp: bool,
        gemini_mcp: bool,
        claude_prompt: bool,
        codex_prompt: bool,
        gemini_prompt: bool,
    }

    fn parse_status_bits(text: &str) -> Option<StatusBits> {
        let slice = match text.split_once("AIO_WSL_STATUS=") {
            Some((_, tail)) => tail,
            None => text,
        };
        let mut bits = slice.chars().filter(|c| *c == '0' || *c == '1');
        Some(StatusBits {
            claude: bits.next()? == '1',
            codex: bits.next()? == '1',
            gemini: bits.next()? == '1',
            claude_mcp: bits.next()? == '1',
            codex_mcp: bits.next()? == '1',
            gemini_mcp: bits.next()? == '1',
            claude_prompt: bits.next()? == '1',
            codex_prompt: bits.next()? == '1',
            gemini_prompt: bits.next()? == '1',
        })
    }

    let mut out = Vec::new();
    for distro in distros {
        let bits: StatusBits = (|| -> Option<StatusBits> {
            let mut cmd = hide_window_cmd("wsl");
            cmd.args(["-d", distro, "bash"]);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(err) => {
                    tracing::warn!(distro = distro, error = %err, "WSL config status spawn failed");
                    return None;
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(status_script.as_bytes());
            }

            let output = match child.wait_with_output() {
                Ok(o) => o,
                Err(err) => {
                    tracing::warn!(distro = distro, error = %err, "WSL config status wait failed");
                    return None;
                }
            };

            if !output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = {
                    let utf8 = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    if utf8.contains('\0') {
                        let decoded = decode_utf16_le(&output.stderr);
                        let trimmed = decoded.trim().to_string();
                        if trimmed.is_empty() {
                            utf8
                        } else {
                            trimmed
                        }
                    } else {
                        utf8
                    }
                };
                tracing::warn!(
                    distro = distro,
                    code = ?output.status.code(),
                    stdout = stdout,
                    stderr = stderr,
                    "WSL config status script failed"
                );
                return None;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            match parse_status_bits(&stdout) {
                Some(v) => Some(v),
                None => {
                    tracing::warn!(
                        distro = distro,
                        stdout = stdout.trim().to_string(),
                        "WSL config status output parse failed"
                    );
                    None
                }
            }
        })()
        .unwrap_or_default();

        out.push(WslDistroConfigStatus {
            distro: distro.clone(),
            claude: bits.claude,
            codex: bits.codex,
            gemini: bits.gemini,
            claude_mcp: bits.claude_mcp,
            codex_mcp: bits.codex_mcp,
            gemini_mcp: bits.gemini_mcp,
            claude_prompt: bits.claude_prompt,
            codex_prompt: bits.codex_prompt,
            gemini_prompt: bits.gemini_prompt,
        });
    }

    out
}

pub fn configure_clients(
    app: &tauri::AppHandle,
    distros: &[String],
    targets: &settings::WslTargetCli,
    proxy_origin: &str,
    mcp_data: Option<&WslMcpSyncData>,
    prompt_data: Option<&WslPromptSyncData>,
    skills_data: Option<&WslSkillsSyncData>,
) -> WslConfigureReport {
    if !cfg!(windows) {
        return WslConfigureReport {
            ok: false,
            message: "WSL configuration is only available on Windows".to_string(),
            distros: Vec::new(),
        };
    }

    let mut distro_reports = Vec::new();
    let mut success_ops = 0usize;
    let mut error_ops = 0usize;

    for distro in distros {
        let mut results = Vec::new();
        let mut cli_backups = Vec::new();

        // Load existing manifest so we don't overwrite original_values on repeated calls
        let existing_manifest = read_wsl_manifest(app, distro).unwrap_or(None);
        let existing_backups: std::collections::HashMap<&str, &WslCliBackup> = existing_manifest
            .as_ref()
            .map(|m| {
                m.cli_backups
                    .iter()
                    .map(|b| (b.cli_key.as_str(), b))
                    .collect()
            })
            .unwrap_or_default();

        // -- Auth configuration (with original-value capture) --
        for (cli_key, enabled, configure_fn) in [
            (
                "claude",
                targets.claude,
                configure_wsl_claude as fn(&str, &str) -> AppResult<()>,
            ),
            (
                "codex",
                targets.codex,
                configure_wsl_codex as fn(&str, &str) -> AppResult<()>,
            ),
            (
                "gemini",
                targets.gemini,
                configure_wsl_gemini as fn(&str, &str) -> AppResult<()>,
            ),
        ] {
            if !enabled {
                continue;
            }
            // If we already have a backup for this CLI (from a prior call), preserve
            // the original_values; otherwise capture fresh ones now.
            let original_values = if let Some(prev) = existing_backups.get(cli_key) {
                prev.original_values.clone()
            } else {
                read_wsl_current_values(distro, cli_key).unwrap_or_default()
            };

            match configure_fn(distro, proxy_origin) {
                Ok(()) => {
                    // Record what we injected
                    let injected_keys = match cli_key {
                        "claude" => {
                            let mut m = std::collections::HashMap::new();
                            m.insert(
                                "ANTHROPIC_BASE_URL".to_string(),
                                format!("{proxy_origin}/claude"),
                            );
                            m.insert(
                                "ANTHROPIC_AUTH_TOKEN".to_string(),
                                "aio-coding-hub".to_string(),
                            );
                            m
                        }
                        "codex" => {
                            let mut m = std::collections::HashMap::new();
                            m.insert(
                                "preferred_auth_method".to_string(),
                                WSL_CODEX_PREFERRED_AUTH_METHOD.to_string(),
                            );
                            m.insert(
                                "model_provider".to_string(),
                                WSL_CODEX_PROVIDER_KEY.to_string(),
                            );
                            m.insert("OPENAI_API_KEY".to_string(), WSL_CODEX_API_KEY.to_string());
                            m
                        }
                        "gemini" => {
                            let mut m = std::collections::HashMap::new();
                            m.insert(
                                "GOOGLE_GEMINI_BASE_URL".to_string(),
                                format!("{proxy_origin}/gemini"),
                            );
                            m.insert("GEMINI_API_KEY".to_string(), "aio-coding-hub".to_string());
                            m
                        }
                        _ => std::collections::HashMap::new(),
                    };
                    cli_backups.push(WslCliBackup {
                        cli_key: cli_key.to_string(),
                        injected_keys,
                        original_values,
                    });
                    results.push(WslConfigureCliReport {
                        cli_key: cli_key.to_string(),
                        ok: true,
                        message: "ok".to_string(),
                    });
                }
                Err(err) => {
                    results.push(WslConfigureCliReport {
                        cli_key: cli_key.to_string(),
                        ok: false,
                        message: err.to_string(),
                    });
                }
            }
        }

        // -- MCP sync --
        if let Some(mcp) = mcp_data {
            for (cli_key, servers) in [
                ("claude", &mcp.claude),
                ("codex", &mcp.codex),
                ("gemini", &mcp.gemini),
            ] {
                if !wsl_target_enabled(targets, cli_key) {
                    continue;
                }
                let managed_keys = read_wsl_mcp_manifest(app, distro, cli_key);
                if servers.is_empty() && managed_keys.is_empty() {
                    continue;
                }
                match sync_wsl_mcp_for_cli(distro, cli_key, servers, &managed_keys) {
                    Ok(new_keys) => {
                        if let Err(e) = write_wsl_mcp_manifest(app, distro, cli_key, &new_keys) {
                            tracing::warn!(
                                distro = distro,
                                cli_key = cli_key,
                                "failed to write WSL MCP manifest: {e}"
                            );
                        }
                        results.push(WslConfigureCliReport {
                            cli_key: format!("{cli_key}_mcp"),
                            ok: true,
                            message: format!("ok ({} servers)", new_keys.len()),
                        });
                    }
                    Err(err) => {
                        results.push(WslConfigureCliReport {
                            cli_key: format!("{cli_key}_mcp"),
                            ok: false,
                            message: err.to_string(),
                        });
                    }
                }
            }
        }

        // -- Prompt sync --
        if let Some(prompts) = prompt_data {
            for (cli_key, content) in [
                ("claude", prompts.claude_content.as_deref()),
                ("codex", prompts.codex_content.as_deref()),
                ("gemini", prompts.gemini_content.as_deref()),
            ] {
                if !wsl_target_enabled(targets, cli_key) {
                    continue;
                }
                match sync_wsl_prompt_for_cli(app, distro, cli_key, content) {
                    Ok(()) => {
                        results.push(WslConfigureCliReport {
                            cli_key: format!("{cli_key}_prompt"),
                            ok: true,
                            message: "ok".to_string(),
                        });
                    }
                    Err(err) => {
                        results.push(WslConfigureCliReport {
                            cli_key: format!("{cli_key}_prompt"),
                            ok: false,
                            message: err.to_string(),
                        });
                    }
                }
            }
        }

        if let Some(skills) = skills_data {
            for (cli_key, entries) in [
                ("claude", &skills.claude),
                ("codex", &skills.codex),
                ("gemini", &skills.gemini),
            ] {
                if !wsl_target_enabled(targets, cli_key) {
                    continue;
                }
                let managed_keys = read_wsl_skills_manifest(app, distro, cli_key);
                if entries.is_empty() && managed_keys.is_empty() {
                    continue;
                }
                match sync_wsl_skills_for_cli(app, distro, cli_key, entries) {
                    Ok(new_keys) => {
                        if let Err(e) = write_wsl_skills_manifest(app, distro, cli_key, &new_keys) {
                            tracing::warn!(
                                distro = distro,
                                cli_key = cli_key,
                                "failed to write WSL skills manifest: {e}"
                            );
                        }
                        results.push(WslConfigureCliReport {
                            cli_key: format!("{cli_key}_skills"),
                            ok: true,
                            message: format!("ok ({} skills)", new_keys.len()),
                        });
                    }
                    Err(err) => {
                        results.push(WslConfigureCliReport {
                            cli_key: format!("{cli_key}_skills"),
                            ok: false,
                            message: err.to_string(),
                        });
                    }
                }
            }
        }

        // -- Write manifest for this distro --
        if !cli_backups.is_empty() {
            let wsl_home_unc = resolve_wsl_home_unc(distro)
                .ok()
                .map(|p| p.to_string_lossy().to_string());
            let manifest = WslDistroManifest {
                schema_version: 1,
                distro: distro.clone(),
                configured: true,
                proxy_origin: proxy_origin.to_string(),
                configured_at: crate::shared::time::now_unix_seconds(),
                wsl_home_unc,
                cli_backups,
            };
            if let Err(e) = write_wsl_manifest(app, distro, &manifest) {
                tracing::warn!("failed to write WSL manifest for {distro}: {e}");
            }
        }

        let distro_ok = results.iter().all(|r| r.ok);
        success_ops += results.iter().filter(|r| r.ok).count();
        error_ops += results.iter().filter(|r| !r.ok).count();

        distro_reports.push(WslConfigureDistroReport {
            distro: distro.clone(),
            ok: distro_ok,
            results,
        });
    }

    let message = if error_ops > 0 {
        format!(
            "已配置：{success_ops} 项；失败：{error_ops} 项（可展开查看每个 distro 的详细结果）"
        )
    } else {
        format!("配置成功：{success_ops} 项")
    };

    WslConfigureReport {
        ok: success_ops > 0,
        message,
        distros: distro_reports,
    }
}
