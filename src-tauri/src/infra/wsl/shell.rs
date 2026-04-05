//! Low-level WSL shell execution and file I/O helpers.

use crate::shared::error::AppResult;
use std::process::{Command, Stdio};

#[cfg(windows)]
pub(super) fn hide_window_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
pub(super) fn hide_window_cmd(program: &str) -> Command {
    Command::new(program)
}

pub(super) fn decode_utf16_le(mut bytes: &[u8]) -> String {
    // BOM (FF FE)
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        bytes = &bytes[2..];
    }

    let len = bytes.len() - (bytes.len() % 2);
    let mut u16s = Vec::with_capacity(len / 2);
    for chunk in bytes[..len].chunks_exact(2) {
        u16s.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }

    String::from_utf16_lossy(&u16s)
}

pub(super) fn bash_single_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

pub(super) fn wsl_resolve_codex_home_script(var_name: &str) -> String {
    format!(
        r#"
codex_home_raw="${{CODEX_HOME:-$HOME/.codex}}"
{var_name}="$codex_home_raw"
if [ "$codex_home_raw" = "~" ]; then
  {var_name}="$HOME"
elif [ "${{codex_home_raw#~/}}" != "$codex_home_raw" ]; then
  {var_name}="$HOME/${{codex_home_raw#~/}}"
elif [ "${{codex_home_raw#~\\}}" != "$codex_home_raw" ]; then
  {var_name}="$HOME/${{codex_home_raw#~\\}}"
else
  case "$codex_home_raw" in
    [A-Za-z]:[\\/]*)
      if command -v wslpath >/dev/null 2>&1; then
        {var_name}="$(wslpath -u "$codex_home_raw")"
      else
        drive="$(printf '%s' "$codex_home_raw" | cut -c1 | tr '[:upper:]' '[:lower:]')"
        rest="$(printf '%s' "$codex_home_raw" | cut -c3- | sed 's#\\\\#/#g; s#^/##')"
        {var_name}="/mnt/$drive/$rest"
      fi
      ;;
    *)
      if [ "${{codex_home_raw#/}}" = "$codex_home_raw" ]; then
        {var_name}="$HOME/$codex_home_raw"
      fi
      ;;
  esac
fi
if [ "$(basename -- "${{{var_name}}}")" = "config.toml" ]; then
  {var_name}="$(dirname "${{{var_name}}}")"
fi
"#,
        var_name = var_name
    )
}

pub(super) fn run_wsl_bash_script(distro: &str, script: &str) -> AppResult<()> {
    let mut cmd = hide_window_cmd("wsl");
    cmd.args(["-d", distro, "bash"]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn wsl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("failed to write wsl stdin: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for wsl: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // wsl.exe on non-English Windows may emit UTF-16LE warnings on stderr;
    // bash/python errors inside the distro are UTF-8.  Try UTF-8 first and
    // fall back to UTF-16LE when null bytes are present (a strong indicator).
    let stderr_raw = &output.stderr;
    let stderr = {
        let utf8 = String::from_utf8_lossy(stderr_raw).trim().to_string();
        if utf8.contains('\0') {
            let decoded = decode_utf16_le(stderr_raw);
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
    let msg = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!(
        "WSL_ERROR: {}",
        if msg.is_empty() {
            "unknown error"
        } else {
            &msg
        }
    )
    .into())
}

/// Execute a bash script inside a WSL distro and capture its stdout.
pub(super) fn run_wsl_bash_script_capture(distro: &str, script: &str) -> AppResult<String> {
    let mut cmd = hide_window_cmd("wsl");
    cmd.args(["-d", distro, "bash"]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn wsl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("failed to write wsl stdin: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for wsl: {e}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr_raw = &output.stderr;
    let stderr = {
        let utf8 = String::from_utf8_lossy(stderr_raw).trim().to_string();
        if utf8.contains('\0') {
            let decoded = decode_utf16_le(stderr_raw);
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
    let msg = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!(
        "WSL_ERROR: {}",
        if msg.is_empty() {
            "unknown error"
        } else {
            &msg
        }
    )
    .into())
}

/// Read a file from WSL using base64 encoding. Returns None if file does not exist.
pub(super) fn read_wsl_file(distro: &str, path_expr: &str) -> AppResult<Option<Vec<u8>>> {
    use base64::Engine;

    let path_escaped = bash_single_quote(path_expr);
    let script = format!(
        r#"
set -euo pipefail
target={path_escaped}
if [ ! -f "$target" ]; then
  echo "AIO_WSL_FILE_NOT_FOUND"
  exit 0
fi
base64 -w0 "$target"
echo ""
"#
    );
    let stdout = run_wsl_bash_script_capture(distro, &script)?;
    let trimmed = stdout.trim();
    if trimmed == "AIO_WSL_FILE_NOT_FOUND" {
        return Ok(None);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|e| format!("WSL_ERROR: base64 decode failed: {e}"))?;
    Ok(Some(bytes))
}

/// Atomically write a file into WSL (backup + tmp + mv).
pub(super) fn write_wsl_file(distro: &str, path_expr: &str, content: &[u8]) -> AppResult<()> {
    use base64::Engine;

    let b64 = base64::engine::general_purpose::STANDARD.encode(content);
    let path_escaped = bash_single_quote(path_expr);
    let b64_escaped = bash_single_quote(&b64);

    let script = format!(
        r#"
set -euo pipefail
HOME="$(getent passwd "$(whoami)" | cut -d: -f6)"
export HOME

target={path_escaped}
dir="$(dirname "$target")"
mkdir -p "$dir"

ts="$(date +%s)"
if [ -f "$target" ]; then
  cp -a "$target" "$target.bak.$ts"
fi

tmp_path="$(mktemp "${{target}}.tmp.XXXXXX")"
cleanup() {{ rm -f "$tmp_path"; }}
trap cleanup EXIT

echo {b64_escaped} | base64 -d > "$tmp_path"

if [ -f "$target" ]; then
  chmod --reference="$target" "$tmp_path" 2>/dev/null || true
fi

mv -f "$tmp_path" "$target"
trap - EXIT
"#
    );
    run_wsl_bash_script(distro, &script)
}

pub(super) fn remove_wsl_file(distro: &str, path_expr: &str) -> AppResult<()> {
    let path_escaped = bash_single_quote(path_expr);
    let script = format!(
        r#"
set -euo pipefail
target={path_escaped}
rm -f -- "$target"
"#
    );
    run_wsl_bash_script(distro, &script)
}

pub(super) fn wsl_path_exists(distro: &str, path_expr: &str) -> AppResult<bool> {
    let path_escaped = bash_single_quote(path_expr);
    let script = format!(
        r#"
set -euo pipefail
target={path_escaped}
if [ -e "$target" ]; then
  echo "1"
else
  echo "0"
fi
"#
    );
    Ok(run_wsl_bash_script_capture(distro, &script)?.trim() == "1")
}

pub(super) fn remove_wsl_dir(distro: &str, path_expr: &str) -> AppResult<()> {
    if !path_expr.starts_with('/') {
        return Err(format!("refusing to remove non-absolute WSL path: {path_expr}").into());
    }
    let path_escaped = bash_single_quote(path_expr);
    let script = format!(
        r#"
set -euo pipefail
target={path_escaped}
rm -rf -- "$target"
"#
    );
    run_wsl_bash_script(distro, &script)
}

pub(super) fn wsl_has_managed_skill_dir(distro: &str, path_expr: &str) -> AppResult<bool> {
    let path_escaped = bash_single_quote(path_expr);
    let script = format!(
        r#"
set -euo pipefail
target={path_escaped}
if [ -f "$target/{marker}" ]; then
  echo "1"
else
  echo "0"
fi
"#,
        marker = super::skills_sync::WSL_SKILL_MANAGED_MARKER_FILE
    );
    Ok(run_wsl_bash_script_capture(distro, &script)?.trim() == "1")
}

/// Write file atomically and force sync to disk (critical for UNC/9P paths during exit).
///
/// Writes to a `.aio-tmp` sibling first, syncs, then renames over the target.
/// This prevents config corruption if the process is killed mid-write.
pub(super) fn write_file_synced(path: &std::path::Path, data: &[u8]) -> AppResult<()> {
    use std::io::Write;
    let file_name = path.file_name().and_then(|v| v.to_str()).unwrap_or("file");
    let tmp_path = path.with_file_name(format!("{file_name}.aio-tmp"));

    let mut file = std::fs::File::create(&tmp_path)
        .map_err(|e| format!("failed to create {}: {e}", tmp_path.display()))?;
    file.write_all(data)
        .map_err(|e| format!("failed to write {}: {e}", tmp_path.display()))?;
    file.sync_all()
        .map_err(|e| format!("failed to sync {}: {e}", tmp_path.display()))?;
    drop(file);

    // Windows rename requires target not to exist.
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("failed to finalize {}: {e}", path.display()))?;
    Ok(())
}
