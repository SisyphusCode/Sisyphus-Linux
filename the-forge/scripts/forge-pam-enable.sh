#!/usr/bin/env bash
# Insert forge login PAM hook (safe to run multiple times).
set -euo pipefail

LOGIN="/etc/pam.d/login"
MARKER="pam-forge-login-session"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for SNIPPET in \
  "${SCRIPT_DIR}/login-forge-snippet" \
  "/usr/share/forge/login-forge-snippet"; do
  [[ -f "$SNIPPET" ]] && break
done
[[ -f "${SNIPPET:-}" ]] || { echo "forge-pam-enable: login-forge-snippet not found" >&2; exit 1; }

if [[ ! -f "$LOGIN" ]]; then
  echo "forge-pam-enable: $LOGIN not found" >&2
  exit 1
fi

if grep -q "$MARKER" "$LOGIN"; then
  echo "forge-pam-enable: already configured in $LOGIN"
  exit 0
fi

tmp="$(mktemp)"
python3 - "$LOGIN" "$SNIPPET" "$tmp" <<'PY'
import sys

login_path, snippet_path, out_path = sys.argv[1:4]
with open(login_path, encoding="utf-8") as fh:
    lines = fh.read().splitlines()
with open(snippet_path, encoding="utf-8") as fh:
    snippet = [line.rstrip() for line in fh if line.strip()]

out = []
inserted = False
for line in lines:
    out.append(line)
    if not inserted and "pam_loginuid.so" in line:
        out.extend(snippet)
        inserted = True

if not inserted:
    raise SystemExit("forge-pam-enable: pam_loginuid.so not found in login")

with open(out_path, "w", encoding="utf-8") as fh:
    fh.write("\n".join(out) + "\n")
PY
mv "$tmp" "$LOGIN"
echo "forge-pam-enable: inserted forge login hook into $LOGIN"