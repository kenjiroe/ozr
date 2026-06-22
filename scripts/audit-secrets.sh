#!/usr/bin/env bash
# Scan the ozr git repository for common secret leaks.
# Usage: ./scripts/audit-secrets.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

echo "==> Checking tracked env files (should be templates only)"
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  case "$path" in
    .env.example|.env.stack.example|tests/fixtures/integration.env.example)
      echo "  ok  $path (template)"
      ;;
    *)
      echo "  FAIL tracked env-like file: $path"
      fail=1
      ;;
  esac
done < <(git ls-files | grep -E '^\.env' || true)

echo "==> Checking git history for private .env commits"
if git log --all --name-only --pretty=format: -- .env .env.local 2>/dev/null | grep -q .; then
  echo "  FAIL .env or .env.local found in git history"
  fail=1
else
  echo "  ok  no .env / .env.local in history"
fi

echo "==> Pattern scan across all commits"
patterns=(
  'sk-[a-zA-Z0-9]\{16,\}'
  'ghp_[a-zA-Z0-9]\{20,\}'
  'AIzaSy[a-zA-Z0-9_-]\{20,\}'
  'xox[baprs]-[a-zA-Z0-9-]\{10,\}'
  '/Users/[a-zA-Z]\+/'
)
for pattern in "${patterns[@]}"; do
  if git log --all -p 2>/dev/null | grep -E -q "$pattern"; then
    echo "  FAIL matched pattern: $pattern"
    fail=1
  fi
done
if [[ "$fail" -eq 0 ]]; then
  echo "  ok  no common secret patterns in history"
fi

if command -v gitleaks >/dev/null 2>&1; then
  echo "==> gitleaks detect"
  gitleaks detect --source "$ROOT" --verbose --redact
elif command -v trufflehog >/dev/null 2>&1; then
  echo "==> trufflehog git"
  trufflehog git "file://$ROOT" --since-commit HEAD~500
else
  echo "  Optional: install gitleaks or trufflehog for deeper scans"
fi

if [[ "$fail" -ne 0 ]]; then
  echo
  echo "Secret audit FAILED — fix findings before publishing."
  exit 1
fi

echo
echo "Secret audit passed."
