//! WSL Gemini CLI configuration.

use super::shell::{bash_single_quote, run_wsl_bash_script};

pub(super) fn configure_wsl_gemini(
    distro: &str,
    proxy_origin: &str,
) -> crate::shared::error::AppResult<()> {
    let base_url = format!("{proxy_origin}/gemini");
    let base_url = bash_single_quote(&base_url);
    let api_key = bash_single_quote("aio-coding-hub");

    let script = format!(
        r#"
set -euo pipefail

HOME="$(getent passwd "$(whoami)" | cut -d: -f6)"
export HOME

mkdir -p "$HOME/.gemini"
env_path="$HOME/.gemini/.env"

if [ -L "$env_path" ]; then
  echo "Refusing to modify: $env_path is a symlink. Please manage it manually or remove the symlink first." >&2
  exit 2
fi

gemini_base_url={base_url}
api_key={api_key}

ts="$(date +%s)"
[ -f "$env_path" ] && cp -a "$env_path" "$env_path.bak.$ts"

tmp_path="$(mktemp "${{env_path}}.tmp.XXXXXX")"
cleanup() {{ rm -f "$tmp_path"; }}
trap cleanup EXIT

if [ -f "$env_path" ]; then
  awk -v gemini_base_url="$gemini_base_url" -v api_key="$api_key" '
    BEGIN {{ seen_base=0; seen_key=0 }}
    function ltrim(s) {{ sub(/^[[:space:]]+/, "", s); return s }}
    {{
      line=$0
      trimmed=ltrim(line)
      if (trimmed ~ /^#/) {{ print line; next }}

      prefix=""
      rest=trimmed
      if (rest ~ /^export[[:space:]]+/) {{
        prefix="export "
        sub(/^export[[:space:]]+/, "", rest)
      }}

      if (rest ~ /^GOOGLE_GEMINI_BASE_URL[[:space:]]*=/) {{
        if (!seen_base) {{
          print prefix "GOOGLE_GEMINI_BASE_URL=" gemini_base_url
          seen_base=1
        }}
        next
      }}
      if (rest ~ /^GEMINI_API_KEY[[:space:]]*=/) {{
        if (!seen_key) {{
          print prefix "GEMINI_API_KEY=" api_key
          seen_key=1
        }}
        next
      }}

      print line
    }}
    END {{
      if (!seen_base) print "GOOGLE_GEMINI_BASE_URL=" gemini_base_url
      if (!seen_key) print "GEMINI_API_KEY=" api_key
    }}
  ' "$env_path" > "$tmp_path"
else
  cat > "$tmp_path" <<EOF
GOOGLE_GEMINI_BASE_URL=$gemini_base_url
GEMINI_API_KEY=$api_key
EOF
fi

if [ ! -s "$tmp_path" ]; then
  echo "Sanity check failed: generated .env is empty" >&2
  exit 2
fi

count_base="$(awk '
  BEGIN{{c=0}}
  {{
    line=$0
    sub(/^[[:space:]]+/, "", line)
    if (line ~ /^#/) next
    if (line ~ /^export[[:space:]]+/) sub(/^export[[:space:]]+/, "", line)
    if (line ~ /^GOOGLE_GEMINI_BASE_URL[[:space:]]*=/) c++
  }}
  END{{print c}}
' "$tmp_path")"
if [ "$count_base" -ne 1 ]; then
  echo "Sanity check failed: expected exactly one GOOGLE_GEMINI_BASE_URL, got $count_base" >&2
  exit 2
fi

count_key="$(awk '
  BEGIN{{c=0}}
  {{
    line=$0
    sub(/^[[:space:]]+/, "", line)
    if (line ~ /^#/) next
    if (line ~ /^export[[:space:]]+/) sub(/^export[[:space:]]+/, "", line)
    if (line ~ /^GEMINI_API_KEY[[:space:]]*=/) c++
  }}
  END{{print c}}
' "$tmp_path")"
if [ "$count_key" -ne 1 ]; then
  echo "Sanity check failed: expected exactly one GEMINI_API_KEY, got $count_key" >&2
  exit 2
fi

actual_base="$(awk '
  {{
    line=$0
    sub(/^[[:space:]]+/, "", line)
    if (line ~ /^#/) next
    if (line ~ /^export[[:space:]]+/) sub(/^export[[:space:]]+/, "", line)
    if (line ~ /^GOOGLE_GEMINI_BASE_URL[[:space:]]*=/) {{
      sub(/^GOOGLE_GEMINI_BASE_URL[[:space:]]*=/, "", line)
      sub(/[[:space:]]+$/, "", line)
      print line
      exit
    }}
  }}
' "$tmp_path")"
if [ "$actual_base" != "$gemini_base_url" ]; then
  echo "Sanity check failed: GOOGLE_GEMINI_BASE_URL mismatch" >&2
  exit 2
fi

actual_key="$(awk '
  {{
    line=$0
    sub(/^[[:space:]]+/, "", line)
    if (line ~ /^#/) next
    if (line ~ /^export[[:space:]]+/) sub(/^export[[:space:]]+/, "", line)
    if (line ~ /^GEMINI_API_KEY[[:space:]]*=/) {{
      sub(/^GEMINI_API_KEY[[:space:]]*=/, "", line)
      sub(/[[:space:]]+$/, "", line)
      print line
      exit
    }}
  }}
' "$tmp_path")"
if [ "$actual_key" != "$api_key" ]; then
  echo "Sanity check failed: GEMINI_API_KEY mismatch" >&2
  exit 2
fi

if [ -f "$env_path" ]; then
  chmod --reference="$env_path" "$tmp_path" 2>/dev/null || true
fi

mv -f "$tmp_path" "$env_path"
trap - EXIT
"#
    );

    run_wsl_bash_script(distro, &script)
}
