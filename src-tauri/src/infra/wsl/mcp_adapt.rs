//! Path adaptation for MCP servers in WSL environments.

use crate::mcp_sync::McpServerForSync;

/// Strip Windows executable extensions (.cmd, .bat, .exe) from a command name.
pub(super) fn strip_win_exe_ext(cmd: &str) -> &str {
    for ext in &[".cmd", ".bat", ".exe", ".CMD", ".BAT", ".EXE"] {
        if let Some(stripped) = cmd.strip_suffix(ext) {
            return stripped;
        }
    }
    cmd
}

/// Check if a path looks like a Windows absolute path (e.g., `C:\...`).
fn is_windows_absolute_path(p: &str) -> bool {
    let bytes = p.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && (bytes[1] == b':')
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// Adapt MCP servers for WSL: strip .cmd/.bat/.exe extensions from commands,
/// and convert Windows absolute paths to WSL `/mnt/` mount paths in command,
/// args, cwd, and env values.
pub fn adapt_mcp_servers_for_wsl(servers: &[McpServerForSync]) -> Vec<McpServerForSync> {
    servers
        .iter()
        .map(|s| {
            let mut adapted = McpServerForSync {
                server_key: s.server_key.clone(),
                transport: s.transport.clone(),
                command: s.command.clone(),
                args: s.args.clone(),
                env: s.env.clone(),
                cwd: s.cwd.clone(),
                url: s.url.clone(),
                headers: s.headers.clone(),
            };

            // Adapt command
            if let Some(ref cmd) = adapted.command {
                let stripped = strip_win_exe_ext(cmd);
                if is_windows_absolute_path(stripped) {
                    // Cannot resolve at build time; skip this server's command conversion.
                    // WSL users will likely have the tool installed natively.
                    // Just use the basename without extension.
                    let basename = stripped.rsplit(['\\', '/']).next().unwrap_or(stripped);
                    adapted.command = Some(basename.to_string());
                } else {
                    adapted.command = Some(stripped.to_string());
                }
            }

            // Adapt args: convert any Windows absolute paths to WSL /mnt/ paths
            adapted.args = adapted
                .args
                .iter()
                .map(|arg| {
                    if is_windows_absolute_path(arg) {
                        win_path_to_wsl_mount(arg)
                    } else {
                        arg.clone()
                    }
                })
                .collect();

            // Adapt cwd
            if let Some(ref cwd) = adapted.cwd {
                if is_windows_absolute_path(cwd) {
                    // Convert backslashes to forward slashes for wslpath compatibility.
                    // Use /mnt/<drive>/... heuristic since we can't call wslpath at build time.
                    let converted = win_path_to_wsl_mount(cwd);
                    adapted.cwd = Some(converted);
                }
            }

            // Adapt env values: convert any Windows absolute paths to WSL /mnt/ paths
            adapted.env = adapted
                .env
                .iter()
                .map(|(k, v)| {
                    let converted = if is_windows_absolute_path(v) {
                        win_path_to_wsl_mount(v)
                    } else {
                        v.clone()
                    };
                    (k.clone(), converted)
                })
                .collect();

            adapted
        })
        .collect()
}

/// Best-effort Windows path to WSL /mnt/ path conversion.
/// E.g., `C:\Users\foo\bar` -> `/mnt/c/Users/foo/bar`
pub(super) fn win_path_to_wsl_mount(win_path: &str) -> String {
    let bytes = win_path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = &win_path[2..].replace('\\', "/");
        format!("/mnt/{drive}{rest}")
    } else {
        win_path.replace('\\', "/")
    }
}
