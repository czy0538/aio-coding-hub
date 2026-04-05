//! WSL detection, path conversion, and host resolution.

use super::shell::{decode_utf16_le, hide_window_cmd, run_wsl_bash_script_capture};
use super::types::WslDetection;
use crate::settings;
use crate::shared::error::{AppError, AppResult};
use std::path::PathBuf;

/// Resolve the Codex home path inside WSL, returned as a Windows path.
pub(super) fn resolve_wsl_codex_home_host_path(distro: &str) -> AppResult<PathBuf> {
    let script = format!(
        r#"
set -euo pipefail
HOME="$(getent passwd "$(whoami)" | cut -d: -f6)"
export HOME
{resolver}
printf '%s\n' "$codex_home"
"#,
        resolver = super::shell::wsl_resolve_codex_home_script("codex_home")
    );
    let resolved = run_wsl_bash_script_capture(distro, &script)?;
    let resolved = resolved.trim();
    if resolved.is_empty() || !resolved.starts_with('/') {
        return Err(format!("failed to resolve CODEX_HOME in {distro}: {resolved}").into());
    }
    Ok(wsl_linux_path_to_windows_path(distro, resolved))
}

/// Resolve the user HOME directory inside a WSL distro, returned as a Windows UNC path.
///
/// Example: distro `"Ubuntu"` -> `\\wsl$\Ubuntu\home\diao`
pub fn resolve_wsl_home_unc(distro: &str) -> AppResult<PathBuf> {
    if !cfg!(windows) {
        return Err(AppError::new(
            "WSL_ERROR",
            "WSL is only available on Windows",
        ));
    }

    let output = hide_window_cmd("wsl")
        .args([
            "-d",
            distro,
            "--",
            "bash",
            "-lc",
            r#"getent passwd "$(whoami)" | cut -d: -f6"#,
        ])
        .output()
        .map_err(|e| AppError::new("WSL_ERROR", format!("failed to run wsl.exe: {e}")))?;

    if !output.status.success() {
        return Err(AppError::new(
            "WSL_ERROR",
            format!("wsl command failed for distro: {distro}"),
        ));
    }

    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() || !home.starts_with('/') {
        return Err(AppError::new(
            "WSL_ERROR",
            format!("invalid HOME for distro {distro}: {home}"),
        ));
    }

    // Build UNC path: \\wsl$\<distro><home_path_with_backslashes>
    let unc = format!(r"\\wsl$\{}{}", distro, home.replace('/', "\\"));
    Ok(PathBuf::from(unc))
}

/// Validate that a distro name is in the detected WSL distros list.
pub fn validate_distro(distro: &str) -> AppResult<()> {
    let trimmed = distro.trim();
    if trimmed.is_empty() {
        return Err(AppError::new("SEC_INVALID_INPUT", "distro is required"));
    }
    let detection = detect();
    if !detection.distros.iter().any(|d| d == trimmed) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            format!("unknown WSL distro: {trimmed}"),
        ));
    }
    Ok(())
}

pub fn detect() -> WslDetection {
    let mut out = WslDetection {
        detected: false,
        distros: Vec::new(),
    };

    if !cfg!(windows) {
        return out;
    }

    let output = hide_window_cmd("wsl").args(["--list", "--quiet"]).output();
    let Ok(output) = output else {
        return out;
    };
    if !output.status.success() {
        return out;
    }

    let decoded = decode_utf16_le(&output.stdout);
    for line in decoded.lines() {
        let mut distro = line.trim().to_string();
        distro = distro.trim_matches(&['\0', '\r'][..]).trim().to_string();
        if distro.is_empty() {
            continue;
        }
        if distro.starts_with("Windows") {
            continue;
        }
        out.distros.push(distro);
    }

    out.detected = !out.distros.is_empty();
    out
}

/// Resolve the host address that WSL distros should use to reach the gateway.
///
/// This is used by:
/// - Gateway listen mode `wsl_auto` (bind host)
/// - WSL client configuration (base origin host)
pub fn resolve_wsl_host(cfg: &settings::AppSettings) -> String {
    match cfg.wsl_host_address_mode {
        settings::WslHostAddressMode::Custom => {
            let addr = cfg.wsl_custom_host_address.trim();
            if addr.is_empty() {
                "127.0.0.1".to_string()
            } else {
                addr.to_string()
            }
        }
        settings::WslHostAddressMode::Auto => {
            host_ipv4_best_effort().unwrap_or_else(|| "127.0.0.1".to_string())
        }
    }
}

pub fn host_ipv4_best_effort() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }

    let output = hide_window_cmd("ipconfig").output().ok()?;
    let stdout = {
        let utf8 = String::from_utf8_lossy(&output.stdout).to_string();
        if utf8.contains('\0') {
            let decoded = decode_utf16_le(&output.stdout);
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
    use std::net::Ipv4Addr;

    let mut in_wsl_adapter = false;
    for raw_line in stdout.lines() {
        let line = raw_line.trim().trim_matches('\0');

        if line.contains("vEthernet (WSL)")
            || line.contains("vEthernet(WSL)")
            || line.contains("Ethernet adapter vEthernet (WSL)")
        {
            in_wsl_adapter = true;
            continue;
        }

        // Adapter section boundary (English + Chinese output). If localized, we keep scanning until we see IPv4.
        if in_wsl_adapter
            && line.ends_with(':')
            && (line.contains("adapter") || line.contains("适配器"))
            && !line.contains("WSL")
        {
            break;
        }

        if !in_wsl_adapter {
            continue;
        }

        if line.contains("IPv4") || line.contains("IP Address") {
            let Some((_, tail)) = line.rsplit_once(':').or_else(|| line.rsplit_once('：')) else {
                continue;
            };
            let ip = tail.trim();
            if ip.is_empty() || ip.contains(':') {
                continue;
            }
            if ip.parse::<Ipv4Addr>().is_ok() {
                return Some(ip.to_string());
            }
        }
    }

    None
}

pub(super) fn wsl_linux_path_to_windows_path(distro: &str, linux_path: &str) -> PathBuf {
    if let Some(rest) = linux_path.strip_prefix("/mnt/") {
        let mut parts = rest.splitn(2, '/');
        if let Some(drive) = parts.next() {
            if drive.len() == 1 && drive.chars().all(|value| value.is_ascii_alphabetic()) {
                let mut path = format!("{}:\\", drive.to_ascii_uppercase());
                if let Some(tail) = parts.next().filter(|value| !value.is_empty()) {
                    path.push_str(&tail.replace('/', "\\"));
                }
                return PathBuf::from(path);
            }
        }
    }

    PathBuf::from(format!(
        r"\\wsl$\{}{}",
        distro,
        linux_path.replace('/', "\\")
    ))
}
