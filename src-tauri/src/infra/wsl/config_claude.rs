//! WSL Claude CLI configuration.

use super::shell::{bash_single_quote, run_wsl_bash_script};

pub(super) fn configure_wsl_claude(
    distro: &str,
    proxy_origin: &str,
) -> crate::shared::error::AppResult<()> {
    let base_url = format!("{proxy_origin}/claude");
    let base_url = bash_single_quote(&base_url);
    let auth_token = bash_single_quote("aio-coding-hub");

    let script = format!(
        r#"
set -euo pipefail

HOME="$(getent passwd "$(whoami)" | cut -d: -f6)"
export HOME

mkdir -p "$HOME/.claude"
config_path="$HOME/.claude/settings.json"

if [ -L "$config_path" ]; then
  echo "Refusing to modify: $config_path is a symlink. Please manage it manually or remove the symlink first." >&2
  exit 2
fi

base_url={base_url}
auth_token={auth_token}

ts="$(date +%s)"
if [ -f "$config_path" ]; then
  cp -a "$config_path" "$config_path.bak.$ts"
fi

tmp_path="$(mktemp "${{config_path}}.tmp.XXXXXX")"
cleanup() {{ rm -f "$tmp_path"; }}
trap cleanup EXIT

if command -v jq >/dev/null 2>&1; then
  if [ -s "$config_path" ]; then
    if ! jq -e 'type=="object" and (.env==null or (.env|type)=="object")' "$config_path" >/dev/null; then
      echo "Refusing to modify: $config_path must be a JSON object and env must be an object (or null)." >&2
      exit 2
    fi

    jq --arg base_url "$base_url" --arg auth_token "$auth_token" '
      .env = (.env // {{}})
      | .env.ANTHROPIC_BASE_URL = $base_url
      | .env.ANTHROPIC_AUTH_TOKEN = $auth_token
    ' "$config_path" > "$tmp_path"
  else
    jq -n --arg base_url "$base_url" --arg auth_token "$auth_token" '{{env:{{ANTHROPIC_BASE_URL:$base_url, ANTHROPIC_AUTH_TOKEN:$auth_token}}}}' > "$tmp_path"
  fi

  jq -e --arg base_url "$base_url" --arg auth_token "$auth_token" '
    .env.ANTHROPIC_BASE_URL == $base_url and .env.ANTHROPIC_AUTH_TOKEN == $auth_token
  ' "$tmp_path" >/dev/null
elif command -v python3 >/dev/null 2>&1; then
  python3 - "$base_url" "$auth_token" "$config_path" "$tmp_path" <<'PY'
import json
import sys
from pathlib import Path

base_url, auth_token, src, dst = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]

data = {{}}
try:
    text = Path(src).read_text(encoding="utf-8")
    if text.strip():
        data = json.loads(text)
except FileNotFoundError:
    data = {{}}
except Exception as e:
    sys.stderr.write(f"Failed to parse existing settings.json: {{e}}\n")
    sys.exit(2)

if not isinstance(data, dict):
    sys.stderr.write("settings.json must be a JSON object\\n")
    sys.exit(2)

env = data.get("env")
if env is None:
    env = {{}}
if not isinstance(env, dict):
    sys.stderr.write("settings.json env must be a JSON object\\n")
    sys.exit(2)

env["ANTHROPIC_BASE_URL"] = base_url
env["ANTHROPIC_AUTH_TOKEN"] = auth_token
data["env"] = env

Path(dst).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY

  python3 - "$base_url" "$auth_token" "$tmp_path" <<'PY'
import json
import sys
from pathlib import Path

base_url, auth_token, path = sys.argv[1], sys.argv[2], sys.argv[3]
payload = json.loads(Path(path).read_text(encoding="utf-8"))
ok = (
    isinstance(payload, dict)
    and isinstance(payload.get("env"), dict)
    and payload["env"].get("ANTHROPIC_BASE_URL") == base_url
    and payload["env"].get("ANTHROPIC_AUTH_TOKEN") == auth_token
)
if not ok:
    sys.stderr.write("Sanity check failed for generated settings.json\\n")
    sys.exit(2)
PY
else
  if [ -s "$config_path" ]; then
    echo "Missing jq/python3; cannot safely merge existing $config_path" >&2
    exit 2
  fi

  cat > "$tmp_path" <<EOF
{{
  "env": {{
    "ANTHROPIC_BASE_URL": "$base_url",
    "ANTHROPIC_AUTH_TOKEN": "$auth_token"
  }}
}}
EOF
fi

if [ ! -s "$tmp_path" ]; then
  echo "Sanity check failed: generated settings.json is empty" >&2
  exit 2
fi

if [ -f "$config_path" ]; then
  chmod --reference="$config_path" "$tmp_path" 2>/dev/null || true
fi

mv -f "$tmp_path" "$config_path"
trap - EXIT

claude_json_path="$HOME/.claude.json"

if [ -L "$claude_json_path" ]; then
  echo "Skipping: $claude_json_path is a symlink." >&2
else
  cj_tmp="$(mktemp "${{claude_json_path}}.tmp.XXXXXX")"
  cj_cleanup() {{ rm -f "$cj_tmp"; }}
  trap cj_cleanup EXIT

  if command -v jq >/dev/null 2>&1; then
    if [ -s "$claude_json_path" ]; then
      jq '.hasCompletedOnboarding = true' "$claude_json_path" > "$cj_tmp"
    else
      jq -n '{{hasCompletedOnboarding: true}}' > "$cj_tmp"
    fi
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "$claude_json_path" "$cj_tmp" <<'PY'
import json, sys
from pathlib import Path
src, dst = sys.argv[1], sys.argv[2]
data = {{}}
try:
    text = Path(src).read_text(encoding="utf-8")
    if text.strip():
        data = json.loads(text)
except FileNotFoundError:
    pass
if not isinstance(data, dict):
    data = {{}}
data["hasCompletedOnboarding"] = True
Path(dst).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY
  else
    if [ -s "$claude_json_path" ]; then
      echo "Missing jq/python3; cannot safely merge existing $claude_json_path" >&2
    else
      echo '{{"hasCompletedOnboarding":true}}' > "$cj_tmp"
    fi
  fi

  if [ -s "$cj_tmp" ]; then
    if [ -f "$claude_json_path" ]; then
      chmod --reference="$claude_json_path" "$cj_tmp" 2>/dev/null || true
    fi
    mv -f "$cj_tmp" "$claude_json_path"
  fi
  trap - EXIT
fi
"#
    );

    run_wsl_bash_script(distro, &script)
}
